//! CLI Path B: deterministic merge package + outer-agent prose + submit/apply.
//!
//! Avoids nested `execute-task consolidate_cluster` under a weak global provider
//! (which drafts via nested `extract_structured` in `draft_patch.rs`). The
//! session agent receives full MDX bodies for keep + redirects, writes the
//! merged MDX to `keeper_file`, then calls `submit_merge` for validation +
//! redirects + link rewrite + depublish + sync.
//!
//! No LLM calls live in this module.

use std::path::Path;

use chrono::{Duration, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::content::slug::{format_blog_link, normalize_url_slug};
use crate::engine::merge_apply;
use crate::models::article::Article;
use crate::models::task::{Task, TaskStatus};

/// Minimum keeper word count after merge (fail-closed on submit).
pub const MIN_KEEPER_WORDS: usize = 400;

/// Clicks in the GSC window that require human confirm before submit.
/// Documented skill rail: high-traffic keepers need `--confirm` / `-y`.
pub const HUMAN_CONFIRM_CLICKS: f64 = 50.0;

/// Impressions in the GSC window that require human confirm before submit.
pub const HUMAN_CONFIRM_IMPRESSIONS: f64 = 1000.0;

/// Default GSC window for package metrics (days).
const GSC_WINDOW_DAYS: i64 = 28;

/// Skill directory name for merge craft rules.
pub const MERGE_CONTENT_SKILL: &str = "merge-content";

// ─── Types ───────────────────────────────────────────────────────────────────

/// Deterministic package for the outer (session) agent — no LLM in this module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergePackage {
    pub project_id: String,
    pub plan: MergePlan,
    pub keep: MergePage,
    pub redirects: Vec<MergePage>,
    /// Absolute path the session agent must write the merged MDX to.
    pub keeper_file: String,
    /// Project-relative form of the keeper path (when under project root).
    pub keeper_path: String,
    pub skill_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_content: Option<String>,
    pub constraints: MergeConstraints,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consolidate_task_id: Option<String>,
    /// High-traffic merges need human confirm before submit (skill rail).
    pub requires_human_confirm: bool,
    /// Path B steps for the session agent.
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergePlan {
    /// `/blog/slug` form.
    pub keep_url: String,
    pub redirect_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub merge_instructions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergePage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub article_id: Option<i64>,
    pub slug: String,
    pub url: String,
    pub title: String,
    /// Absolute path to the MDX file.
    pub file: String,
    /// Project-relative path when under project root.
    pub path: String,
    pub word_count: usize,
    /// FULL MDX including frontmatter.
    pub content: String,
    pub outline: Vec<MergeOutlineHeading>,
    /// From `gsc_page_daily` window if available, else 0.
    pub impressions: f64,
    pub clicks: f64,
    pub queries: Vec<MergeQueryMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeOutlineHeading {
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeQueryMetric {
    pub query: String,
    pub impressions: f64,
    pub clicks: f64,
    pub avg_position: f64,
    pub ctr: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConstraints {
    pub min_keeper_words: usize,
    pub keep_url: String,
    pub redirect_urls: Vec<String>,
    pub require_valid_mdx: bool,
}

/// Options for [`submit_merge`].
#[derive(Debug, Clone, Default)]
pub struct MergeSubmitOpts {
    pub consolidate_task_id: Option<String>,
    pub keep_url: Option<String>,
    pub redirect_urls: Option<Vec<String>>,
    /// If true, skip `requires_human_confirm` gate (operator already confirmed).
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeSubmitResult {
    pub ok: bool,
    pub checks: Vec<MergeCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keeper_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_url: Option<String>,
    pub redirect_urls: Vec<String>,
    pub redirects_written: bool,
    pub inbound_links_rewritten: usize,
    pub sources_depublished: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consolidate_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consolidate_task_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// Input source for [`build_merge_package`].
#[derive(Debug, Clone)]
pub enum MergeContextSource {
    ConsolidateTask { task_id: String },
    ArticleIds { keep_id: i64, redirect_ids: Vec<i64> },
    Urls {
        keep_url: String,
        redirect_urls: Vec<String>,
    },
}

// ─── Internal resolved plan ──────────────────────────────────────────────────

struct ResolvedPlan {
    keep_url: String,
    redirect_urls: Vec<String>,
    cluster_id: Option<String>,
    reason: Option<String>,
    merge_instructions: Vec<String>,
    consolidate_task_id: Option<String>,
}

// ─── build_merge_package ─────────────────────────────────────────────────────

/// Build a deterministic merge package for the session agent. No LLM.
pub fn build_merge_package(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    source: MergeContextSource,
) -> Result<MergePackage, String> {
    if project_id.trim().is_empty() {
        return Err("project_id is required".to_string());
    }

    let resolved = resolve_plan_from_source(conn, project_id, project_path, &source)?;
    let keep_slug = normalize_url_slug(&resolved.keep_url);
    if keep_slug.is_empty() {
        return Err("keep_url is empty after normalization".to_string());
    }
    if resolved.redirect_urls.is_empty() {
        return Err("redirect_urls must not be empty".to_string());
    }

    let redirect_slugs: Vec<String> = resolved
        .redirect_urls
        .iter()
        .map(|u| normalize_url_slug(u))
        .filter(|s| !s.is_empty())
        .collect();
    if redirect_slugs.is_empty() {
        return Err("redirect_urls normalize to empty set".to_string());
    }
    // Preflight: cycle — keep must not appear in redirects.
    if redirect_slugs.iter().any(|s| s == &keep_slug) {
        return Err(format!(
            "cycle: keep slug '{keep_slug}' appears in redirect_urls"
        ));
    }

    // Load article catalog once (avoid N+1 list_articles in page loaders).
    let articles =
        crate::engine::task_store::list_articles(conn, project_id).unwrap_or_default();

    let keep_page = load_merge_page(conn, project_id, project_path, &keep_slug, &articles)?;
    let mut redirect_pages = Vec::with_capacity(redirect_slugs.len());
    for slug in &redirect_slugs {
        redirect_pages.push(load_merge_page(
            conn,
            project_id,
            project_path,
            slug,
            &articles,
        )?);
    }

    let keep_url = format_blog_link(&keep_slug);
    let redirect_urls: Vec<String> = redirect_slugs.iter().map(|s| format_blog_link(s)).collect();

    let min_keeper_words = MIN_KEEPER_WORDS;
    let requires_human_confirm = keep_page.clicks >= HUMAN_CONFIRM_CLICKS
        || keep_page.impressions >= HUMAN_CONFIRM_IMPRESSIONS;

    let (skill_name, skill_content) =
        match crate::engine::skills::load_skill(project_path, MERGE_CONTENT_SKILL) {
            Some(skill) => (skill.name, Some(skill.content)),
            None => (MERGE_CONTENT_SKILL.to_string(), None),
        };

    let plan = MergePlan {
        keep_url: keep_url.clone(),
        redirect_urls: redirect_urls.clone(),
        cluster_id: resolved.cluster_id,
        reason: resolved.reason,
        merge_instructions: resolved.merge_instructions,
    };

    let constraints = MergeConstraints {
        min_keeper_words,
        keep_url: keep_url.clone(),
        redirect_urls: redirect_urls.clone(),
        require_valid_mdx: true,
    };

    let keeper_file = keep_page.file.clone();
    let keeper_path = keep_page.path.clone();
    let instructions = build_path_b_instructions(
        &keeper_file,
        min_keeper_words,
        requires_human_confirm,
        resolved.consolidate_task_id.as_deref(),
    );

    Ok(MergePackage {
        project_id: project_id.to_string(),
        plan,
        keep: keep_page,
        redirects: redirect_pages,
        keeper_file,
        keeper_path,
        skill_name,
        skill_content,
        constraints,
        consolidate_task_id: resolved.consolidate_task_id,
        requires_human_confirm,
        instructions,
    })
}

// ─── submit_merge ────────────────────────────────────────────────────────────

/// Validate merged keeper MDX on disk, then apply redirects / rewrites /
/// depublish / sync. On validation failure returns `ok: false` with checks —
/// **does not** write redirects, rewrite links, or depublish.
///
/// Domain errors (missing plan, wrong task type) return `Err`.
pub fn submit_merge(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    opts: MergeSubmitOpts,
) -> Result<MergeSubmitResult, String> {
    if project_id.trim().is_empty() {
        return Err("project_id is required".to_string());
    }

    let plan = resolve_submit_plan(conn, project_id, project_path, &opts)?;
    let keep_slug = normalize_url_slug(&plan.keep_url);
    if keep_slug.is_empty() {
        return Err("keep_url is empty after normalization".to_string());
    }
    let redirect_slugs: Vec<String> = plan
        .redirect_urls
        .iter()
        .map(|u| normalize_url_slug(u))
        .filter(|s| !s.is_empty())
        .collect();
    if redirect_slugs.is_empty() {
        return Err("redirect_urls must not be empty".to_string());
    }

    let keep_url = format_blog_link(&keep_slug);
    let redirect_urls: Vec<String> = redirect_slugs.iter().map(|s| format_blog_link(s)).collect();

    let bound_task = resolve_bound_consolidate_task(conn, project_id, &opts)?;

    // 1. Resolve keeper file and read content.
    let keeper_file = crate::content::ops::find_file_by_slug(project_path, &keep_slug)
        .map_err(|e| e)?
        .ok_or_else(|| format!("Keeper file not found for slug '{keep_slug}'"))?;

    let content = std::fs::read_to_string(&keeper_file)
        .map_err(|e| format!("Failed to read keeper {}: {e}", keeper_file.display()))?;

    let mut checks: Vec<MergeCheck> = Vec::new();

    // 2. Validate MDX structure.
    let mdx_ok = match crate::content::cleaner::validate_mdx_structure(&content) {
        Ok(()) => {
            checks.push(MergeCheck {
                name: "valid_mdx".into(),
                ok: true,
                detail: "MDX structure ok".into(),
            });
            true
        }
        Err(e) => {
            checks.push(MergeCheck {
                name: "valid_mdx".into(),
                ok: false,
                detail: e,
            });
            false
        }
    };

    // 3. Word floor.
    let word_count = crate::content::ops::count_words(&content);
    let words_ok = word_count >= MIN_KEEPER_WORDS;
    checks.push(MergeCheck {
        name: "min_keeper_words".into(),
        ok: words_ok,
        detail: format!("{word_count} words (min {MIN_KEEPER_WORDS})"),
    });

    // 4. Preflight: redirect files exist, no cycle.
    let mut preflight_ok = true;
    if redirect_slugs.iter().any(|s| s == &keep_slug) {
        preflight_ok = false;
        checks.push(MergeCheck {
            name: "no_cycle".into(),
            ok: false,
            detail: format!("keep slug '{keep_slug}' is also in redirect_urls"),
        });
    } else {
        checks.push(MergeCheck {
            name: "no_cycle".into(),
            ok: true,
            detail: "keeper not in redirect list".into(),
        });
    }

    let mut missing = Vec::new();
    for slug in &redirect_slugs {
        if slug == &keep_slug {
            continue;
        }
        match crate::content::ops::find_file_by_slug(project_path, slug) {
            Ok(Some(p)) if p.is_file() => {}
            Ok(_) => missing.push(slug.clone()),
            Err(e) => return Err(e),
        }
    }
    if missing.is_empty() {
        checks.push(MergeCheck {
            name: "redirect_files_exist".into(),
            ok: true,
            detail: format!("{} redirect source file(s) found", redirect_slugs.len()),
        });
    } else {
        preflight_ok = false;
        checks.push(MergeCheck {
            name: "redirect_files_exist".into(),
            ok: false,
            detail: format!("missing redirect source file(s): {}", missing.join(", ")),
        });
    }

    // 5. High-traffic human confirm (recheck GSC at submit time).
    let (keep_clicks, keep_impressions) =
        soft_gsc_metrics(conn, project_id, &keep_slug);
    let needs_confirm =
        keep_clicks >= HUMAN_CONFIRM_CLICKS || keep_impressions >= HUMAN_CONFIRM_IMPRESSIONS;
    let confirm_ok = !needs_confirm || opts.confirmed;
    if needs_confirm {
        checks.push(MergeCheck {
            name: "human_confirm".into(),
            ok: confirm_ok,
            detail: if confirm_ok {
                format!(
                    "confirmed (clicks={keep_clicks:.0}, impressions={keep_impressions:.0})"
                )
            } else {
                format!(
                    "high-traffic keep (clicks={keep_clicks:.0}, impressions={keep_impressions:.0}) requires --confirm / -y"
                )
            },
        });
    } else {
        checks.push(MergeCheck {
            name: "human_confirm".into(),
            ok: true,
            detail: "below high-traffic threshold".into(),
        });
    }

    let all_ok = mdx_ok && words_ok && preflight_ok && confirm_ok;
    if !all_ok {
        return Ok(MergeSubmitResult {
            ok: false,
            checks,
            keeper_file: Some(keeper_file.to_string_lossy().to_string()),
            keep_url: Some(keep_url),
            redirect_urls,
            redirects_written: false,
            inbound_links_rewritten: 0,
            sources_depublished: 0,
            consolidate_task_id: bound_task.as_ref().map(|t| t.id.clone()),
            consolidate_task_status: bound_task
                .as_ref()
                .map(|t| t.status.as_str().to_string()),
            message: Some(
                "Validation failed — fix keeper MDX / confirm high-traffic and resubmit. No redirects or depublish applied."
                    .to_string(),
            ),
        });
    }

    // ── Apply (only after all gates pass) ────────────────────────────────────
    // Shared primitives with desktop consolidate_cluster (engine::merge_apply).

    // 6. Write/merge redirects.csv
    merge_apply::upsert_redirects_csv(project_path, &keep_url, &redirect_urls)?;
    let redirects_written = true;

    // 7. Rewrite inbound links (fail-closed if content dir missing)
    let (inbound_links_rewritten, _) =
        merge_apply::rewrite_inbound_links_to_keeper(project_path, &keep_url, &redirect_slugs)?;

    // 8. Depublish redirect sources (fail-closed on missing file or articles row)
    let sources_depublished = merge_apply::depublish_redirect_slugs(
        conn,
        project_id,
        project_path,
        &keep_slug,
        &redirect_slugs,
    )?;

    // 9. Sync + export
    let automation_dir = project_path.join(".github").join("automation");
    crate::content::ops::sync_and_validate(
        &automation_dir,
        project_path,
        true,
        conn,
        project_id,
    )
    .map_err(|e| format!("sync_and_validate failed: {e}"))?;
    crate::db::export::write_articles_to_repo(conn, project_id, project_path)
        .map_err(|e| format!("export articles.json failed: {e}"))?;

    // 10. Complete bound consolidate_cluster task
    let (consolidate_task_id, consolidate_task_status) =
        complete_consolidate_task_if_bound(conn, bound_task.as_ref());

    Ok(MergeSubmitResult {
        ok: true,
        checks,
        keeper_file: Some(keeper_file.to_string_lossy().to_string()),
        keep_url: Some(keep_url),
        redirect_urls,
        redirects_written,
        inbound_links_rewritten,
        sources_depublished,
        consolidate_task_id,
        consolidate_task_status,
        message: Some(format!(
            "Merge applied: redirects written, {inbound_links_rewritten} inbound link(s) rewritten, {sources_depublished} source(s) depublished."
        )),
    })
}

// ─── Plan resolution ─────────────────────────────────────────────────────────

fn resolve_plan_from_source(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    source: &MergeContextSource,
) -> Result<ResolvedPlan, String> {
    match source {
        MergeContextSource::ConsolidateTask { task_id } => {
            let task = crate::engine::task_store::get_task(conn, task_id)
                .map_err(|e| format!("Consolidate task not found ({task_id}): {e}"))?;
            if task.project_id != project_id {
                return Err(format!(
                    "Consolidate task {task_id} does not belong to this project"
                ));
            }
            if task.task_type != "consolidate_cluster" {
                return Err(format!(
                    "Task {task_id} has type '{}', expected consolidate_cluster",
                    task.task_type
                ));
            }
            let plan_json = merge_apply::load_plan_json_from_task(&task, project_path)?;
            let mut plan = parse_plan_json(&plan_json)?;
            plan.consolidate_task_id = Some(task_id.clone());
            if plan.cluster_id.is_none() {
                plan.cluster_id = merge_apply::cluster_id_from_title(task.title.as_deref());
            }
            Ok(plan)
        }
        MergeContextSource::ArticleIds {
            keep_id,
            redirect_ids,
        } => {
            if redirect_ids.is_empty() {
                return Err("redirect_ids must not be empty".to_string());
            }
            let articles = crate::engine::task_store::list_articles(conn, project_id)
                .map_err(|e| e.to_string())?;
            let keep = articles
                .iter()
                .find(|a| a.id == *keep_id)
                .ok_or_else(|| format!("Keep article id {keep_id} not found in project"))?;
            let mut redirect_urls = Vec::new();
            for id in redirect_ids {
                let a = articles
                    .iter()
                    .find(|a| a.id == *id)
                    .ok_or_else(|| format!("Redirect article id {id} not found in project"))?;
                redirect_urls.push(format_blog_link(&a.url_slug));
            }
            Ok(ResolvedPlan {
                keep_url: format_blog_link(&keep.url_slug),
                redirect_urls,
                cluster_id: None,
                reason: None,
                merge_instructions: vec![],
                consolidate_task_id: None,
            })
        }
        MergeContextSource::Urls {
            keep_url,
            redirect_urls,
        } => {
            if keep_url.trim().is_empty() {
                return Err("keep_url is required".to_string());
            }
            if redirect_urls.is_empty() {
                return Err("redirect_urls must not be empty".to_string());
            }
            Ok(ResolvedPlan {
                keep_url: keep_url.clone(),
                redirect_urls: redirect_urls.clone(),
                cluster_id: None,
                reason: None,
                merge_instructions: vec![],
                consolidate_task_id: None,
            })
        }
    }
}

fn resolve_submit_plan(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    opts: &MergeSubmitOpts,
) -> Result<ResolvedPlan, String> {
    let has_urls = opts
        .keep_url
        .as_ref()
        .map(|u| !u.trim().is_empty())
        .unwrap_or(false)
        && opts
            .redirect_urls
            .as_ref()
            .map(|r| !r.is_empty())
            .unwrap_or(false);

    if has_urls {
        return Ok(ResolvedPlan {
            keep_url: opts.keep_url.clone().unwrap_or_default(),
            redirect_urls: opts.redirect_urls.clone().unwrap_or_default(),
            cluster_id: None,
            reason: None,
            merge_instructions: vec![],
            consolidate_task_id: opts.consolidate_task_id.clone(),
        });
    }

    if let Some(ref task_id) = opts.consolidate_task_id {
        let task = crate::engine::task_store::get_task(conn, task_id)
            .map_err(|e| format!("Consolidate task not found ({task_id}): {e}"))?;
        if task.project_id != project_id {
            return Err(format!(
                "Consolidate task {task_id} does not belong to this project"
            ));
        }
        if task.task_type != "consolidate_cluster" {
            return Err(format!(
                "Task {task_id} has type '{}', expected consolidate_cluster",
                task.task_type
            ));
        }
        let plan_json = merge_apply::load_plan_json_from_task(&task, project_path)?;
        let mut plan = parse_plan_json(&plan_json)?;
        plan.consolidate_task_id = Some(task_id.clone());
        return Ok(plan);
    }

    Err(
        "merge-submit requires -K/--keep-url + -R/--redirect-urls, or -I consolidate-task-id"
            .to_string(),
    )
}

fn parse_plan_json(plan_json: &str) -> Result<ResolvedPlan, String> {
    let plan: serde_json::Value =
        serde_json::from_str(plan_json).map_err(|e| format!("Invalid merge plan JSON: {e}"))?;

    let keep_url = plan["keep_url"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if keep_url.is_empty() {
        return Err("Merge plan missing keep_url".to_string());
    }

    let redirect_urls: Vec<String> = plan["redirect_urls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .filter(|s| !s.trim().is_empty())
                .collect()
        })
        .unwrap_or_default();
    if redirect_urls.is_empty() {
        return Err("Merge plan missing redirect_urls".to_string());
    }

    let cluster_id = plan["cluster_id"]
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let reason = plan["reason"]
        .as_str()
        .or_else(|| plan["rationale"].as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let merge_instructions: Vec<String> = plan["merge_instructions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(ResolvedPlan {
        keep_url,
        redirect_urls,
        cluster_id,
        reason,
        merge_instructions,
        consolidate_task_id: None,
    })
}

// ─── Page loading ────────────────────────────────────────────────────────────

fn load_merge_page(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    slug: &str,
    articles: &[Article],
) -> Result<MergePage, String> {
    let file = crate::content::ops::find_file_by_slug(project_path, slug)
        .map_err(|e| e)?
        .ok_or_else(|| format!("Content file not found for slug '{slug}'"))?;

    if !file.is_file() {
        return Err(format!("Content file missing for slug '{slug}': {}", file.display()));
    }

    let content = std::fs::read_to_string(&file)
        .map_err(|e| format!("Failed to read {}: {e}", file.display()))?;

    let word_count = crate::content::ops::count_words(&content);
    let outline = extract_outline(&content);
    let title = extract_title(&content).unwrap_or_else(|| slug.to_string());

    let article = articles.iter().find(|a| {
        a.url_slug == slug || normalize_url_slug(&a.url_slug) == slug
    });
    let article_id = article.map(|a| a.id);
    let title = article
        .map(|a| a.title.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or(title);

    let (impressions, clicks) = soft_gsc_metrics(conn, project_id, slug);
    let queries = soft_top_queries(conn, project_id, article_id);

    Ok(MergePage {
        article_id,
        slug: slug.to_string(),
        url: format_blog_link(slug),
        title,
        file: file.to_string_lossy().to_string(),
        path: path_relative_to_project(project_path, &file),
        word_count,
        content,
        outline,
        impressions,
        clicks,
        queries,
    })
}

fn extract_outline(content: &str) -> Vec<MergeOutlineHeading> {
    let body = crate::content::frontmatter::split_mdx(content)
        .map(|(_, b)| b)
        .unwrap_or(content);
    body.lines()
        .filter_map(|line| {
            let t = line.trim_start();
            if t.starts_with("## ") || t.starts_with("### ") || t.starts_with("#### ") {
                let level = t.chars().take_while(|&c| c == '#').count() as u8;
                let text = t.trim_start_matches('#').trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(MergeOutlineHeading { level, text })
                }
            } else {
                None
            }
        })
        .collect()
}

fn extract_title(content: &str) -> Option<String> {
    if let Some((fm, _)) = crate::content::frontmatter::split_mdx(content) {
        if let Ok(parsed) = crate::content::frontmatter::parse(fm) {
            if let Some(s) = parsed.parsed.get("title").and_then(|v| v.as_str()) {
                let clean = s.trim().trim_matches('"').trim_matches('\'');
                if !clean.is_empty() {
                    return Some(clean.to_string());
                }
            }
        }
    }
    None
}

/// Soft-fail GSC window metrics (0 when unavailable / no page match).
fn soft_gsc_metrics(conn: &Connection, project_id: &str, slug: &str) -> (f64, f64) {
    let (start, end) = gsc_window_dates(GSC_WINDOW_DAYS);
    let pages = crate::db::list_gsc_page_daily_pages(conn, project_id).unwrap_or_default();
    let page = pages.into_iter().find(|p| {
        let s = crate::content::slug::extract_slug_from_url(p);
        s == slug || normalize_url_slug(&s) == slug
    });
    let Some(page) = page else {
        return (0.0, 0.0);
    };
    match crate::db::gsc_page_daily_window_metrics(conn, project_id, &page, &start, &end) {
        Ok(Some(m)) => (m.impressions, m.clicks),
        _ => (0.0, 0.0),
    }
}

fn soft_top_queries(
    conn: &Connection,
    project_id: &str,
    article_id: Option<i64>,
) -> Vec<MergeQueryMetric> {
    let Some(id) = article_id else {
        return vec![];
    };
    crate::db::get_ctr_query_metrics(conn, project_id, id)
        .unwrap_or_default()
        .into_iter()
        .take(10)
        .map(|q| MergeQueryMetric {
            query: q.query,
            impressions: q.impressions,
            clicks: q.clicks,
            avg_position: q.avg_position,
            ctr: q.ctr,
        })
        .collect()
}

fn gsc_window_dates(period_days: i64) -> (String, String) {
    let end = Utc::now().date_naive() - Duration::days(1);
    let start = end - Duration::days(period_days - 1);
    (
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    )
}

fn resolve_bound_consolidate_task(
    conn: &Connection,
    project_id: &str,
    opts: &MergeSubmitOpts,
) -> Result<Option<Task>, String> {
    let Some(ref id) = opts.consolidate_task_id else {
        return Ok(None);
    };
    let id = id.trim();
    if id.is_empty() {
        return Ok(None);
    }
    let task = crate::engine::task_store::get_task(conn, id)
        .map_err(|e| format!("Consolidate task not found ({id}): {e}"))?;
    if task.project_id != project_id {
        return Err(format!(
            "Consolidate task {id} does not belong to this project"
        ));
    }
    if task.task_type != "consolidate_cluster" {
        return Err(format!(
            "Task {id} has type '{}', expected consolidate_cluster — not marking done",
            task.task_type
        ));
    }
    if task.status == TaskStatus::Cancelled {
        return Err(format!(
            "Consolidate task {id} is cancelled and cannot be completed via merge-submit"
        ));
    }
    Ok(Some(task))
}

fn complete_consolidate_task_if_bound(
    conn: &Connection,
    task: Option<&Task>,
) -> (Option<String>, Option<String>) {
    let Some(task) = task else {
        return (None, None);
    };
    if task.status == TaskStatus::Done {
        return (
            Some(task.id.clone()),
            Some(TaskStatus::Done.as_str().to_string()),
        );
    }
    match crate::engine::task_store::update_task_status(conn, &task.id, TaskStatus::Done) {
        Ok(updated) => (
            Some(updated.id),
            Some(updated.status.as_str().to_string()),
        ),
        Err(e) => {
            log::warn!(
                "[merge_package] failed to mark consolidate task {} done: {}",
                task.id,
                e
            );
            (
                Some(task.id.clone()),
                Some(task.status.as_str().to_string()),
            )
        }
    }
}

fn path_relative_to_project(project_path: &Path, abs: &Path) -> String {
    abs.strip_prefix(project_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| abs.to_string_lossy().to_string())
}

fn build_path_b_instructions(
    keeper_file: &str,
    min_words: usize,
    requires_human_confirm: bool,
    consolidate_task_id: Option<&str>,
) -> String {
    let confirm_note = if requires_human_confirm {
        "\n4. High-traffic keep: obtain human confirmation, then call merge-submit with --confirm / -y."
    } else {
        "\n4. Call merge-submit (no --confirm needed for this keep traffic level)."
    };
    let task_flag = consolidate_task_id
        .map(|id| format!(" -I {id}"))
        .unwrap_or_default();

    format!(
        "Path B merge (session agent — do NOT nested draft_patch / execute-task consolidate_cluster):\n\
         1. Read keep + redirect FULL MDX bodies in this package.\n\
         2. Write the complete merged MDX (preserve/improve frontmatter; fold unique tables/FAQs/examples from redirects) to:\n\
            {keeper_file}\n\
         3. Ensure ≥{min_words} words and valid MDX structure.{confirm_note}\n\
         5. Submit:\n\
            pageseeds-cli merge-submit -i <project-id> -p <project-path>{task_flag}\n\
            (or pass -K keep-url -R redirect-urls if no consolidate task)\n\
         On ok:false, expand/fix the keeper file and resubmit — redirects are not applied until validation passes."
    )
}


// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
