//! CLI Path B: deterministic write package + outer-agent prose + submit/verify.
//!
//! Avoids nested `execute-task write_article` under a weak global provider.
//! The session agent receives a fully structured package (brief, target path,
//! skill body, word floors), writes MDX to `target_file`, then submits via
//! `submit_written_article` for structural validation + ingest + follow-ups.
//!
//! No LLM calls live in this module.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::content::validate_article::{
    validate_article_content, ArticleCheck, ValidateArticleInput, ValidateArticleResult,
    DEFAULT_MIN_WORD_COUNT,
};
use crate::engine::content_brief::{
    build_content_brief, extract_article_keyword_meta, load_content_brief_context, ContentBrief,
};
use crate::engine::keyword_selection::{
    extract_keyword_metrics, extract_selectable_keywords, normalize_keyword,
};
use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::models::task::{Priority, Task, TaskArtifact, TaskRunPolicy, TaskStatus};

/// Target body length for Path B writers (guidance; floor is [`DEFAULT_MIN_WORD_COUNT`]).
pub const DEFAULT_TARGET_WORD_COUNT: usize = 1200;

/// Skill directory name for the content writer craft rules.
pub const CONTENT_WRITE_SKILL: &str = "content-write";

// ─── Types ───────────────────────────────────────────────────────────────────

/// Deterministic package handed to the outer (session) agent for one keyword.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WritePackage {
    pub keyword: String,
    pub research_task_id: String,
    pub project_id: String,
    pub content_brief: ContentBrief,
    /// Absolute path the agent should write the MDX file to.
    pub target_file: String,
    /// Project-relative form of the target (when under project root).
    pub target_path: String,
    pub publish_date: Option<String>,
    pub skill_name: String,
    /// Full skill body so offline agents get craft rules without a second fetch.
    pub skill_content: Option<String>,
    pub min_words: usize,
    pub target_words: usize,
    pub constraints: WriteConstraints,
    /// Existing `write_article` task from `select-keywords` (provenance), if any.
    pub write_task_id: Option<String>,
}

/// Structural constraints the agent must satisfy before submit will pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConstraints {
    pub min_word_count: usize,
    pub target_word_count: usize,
    /// Internal link href pattern, e.g. `/blog/{slug}`.
    pub link_format: String,
    pub frontmatter_fields: Vec<String>,
}

/// Options for [`submit_written_article`].
#[derive(Debug, Clone, Default)]
pub struct SubmitOpts {
    /// Existing write_article task to mark done (from package or select-keywords).
    pub write_task_id: Option<String>,
    /// Keyword for article tagging when no write task description is available.
    pub keyword: Option<String>,
}

/// Result of validate + ingest + follow-up spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validation: Option<ValidateArticleResult>,
    pub checks: Vec<ArticleCheck>,
    pub ingested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_task_status: Option<String>,
    pub follow_up_task_ids: Vec<String>,
    /// Catalog `articles.status` after registration (`"draft"` for new Path B writes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_status: Option<String>,
    /// Why catalog status is what it is (stable string for operators/CLI JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_status_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Outcome of Path B article registration (ingest + draft demote + keyword tag).
struct RegisterOutcome {
    registered: bool,
    newly_ingested: bool,
    catalog_status: Option<String>,
}

/// Stable reason when Path B forces new catalog rows to draft (issue #168 / #257).
const CATALOG_DRAFT_UNTIL_PUBLISH_REASON: &str =
    "new articles stay draft until publish-content -S <slug>";

// ─── build_write_package ─────────────────────────────────────────────────────

/// Build a deterministic write package for one keyword from a research task.
///
/// Validates the keyword against the research selection list using the same
/// normalizer / extractors as [`crate::engine::keyword_selection`]. No LLM.
pub fn build_write_package(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    research_task_id: &str,
    keyword: &str,
) -> Result<WritePackage, String> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return Err("Keyword is required".to_string());
    }

    let research_task = crate::engine::task_store::get_task(conn, research_task_id)
        .map_err(|e| e.to_string())?;

    if research_task.project_id != project_id {
        return Err("Research task does not belong to this project".to_string());
    }

    // Same selection validation as build_content_tasks_from_keywords.
    let allowed_keywords = extract_selectable_keywords(&research_task);
    if allowed_keywords.is_empty() {
        return Err(
            "No selectable keywords found on the research task. Re-run keyword research first."
                .to_string(),
        );
    }
    let allowed_set: std::collections::HashSet<String> = allowed_keywords
        .iter()
        .map(|k| normalize_keyword(k))
        .collect();
    let normalized = normalize_keyword(keyword);
    if !allowed_set.contains(&normalized) {
        return Err(format!(
            "Keyword is outside the workflow selection list: {keyword}"
        ));
    }

    let brief_ctx = load_content_brief_context(conn, project_id, &research_task);
    let metrics = extract_keyword_metrics(&research_task);
    let article_meta = extract_article_keyword_meta(&research_task);
    let metric = metrics.get(&normalized);
    let am = article_meta.get(&normalized);
    let content_brief = build_content_brief(keyword, metric, None, am, &brief_ctx);

    let content_dir = resolve_content_dir_for_package(conn, project_id, project_path)?;
    let style = crate::content::naming::detect_numbered_mdx_style(&content_dir);
    // Keyword is the stem (same role as task_topic_stem for write_article).
    let target_abs = crate::content::naming::next_article_path(&content_dir, style, keyword);
    let target_file = target_abs.to_string_lossy().to_string();
    let target_path = path_relative_to_project(project_path, &target_abs);

    let publish_date =
        crate::engine::exec::agentic::compute_next_publish_date(conn, project_id);

    let (skill_name, skill_content) = match crate::engine::skills::load_skill(
        project_path,
        CONTENT_WRITE_SKILL,
    ) {
        Some(skill) => (skill.name, Some(skill.content)),
        None => (CONTENT_WRITE_SKILL.to_string(), None),
    };

    let write_task_id = find_active_write_task_id(conn, project_id, &normalized);

    Ok(WritePackage {
        keyword: keyword.to_string(),
        research_task_id: research_task_id.to_string(),
        project_id: project_id.to_string(),
        content_brief,
        target_file,
        target_path,
        publish_date,
        skill_name,
        skill_content,
        min_words: DEFAULT_MIN_WORD_COUNT,
        target_words: DEFAULT_TARGET_WORD_COUNT,
        constraints: WriteConstraints {
            min_word_count: DEFAULT_MIN_WORD_COUNT,
            target_word_count: DEFAULT_TARGET_WORD_COUNT,
            link_format: "/blog/{slug}".to_string(),
            frontmatter_fields: vec![
                "title".into(),
                "description".into(),
                "slug".into(),
                "date".into(),
                "status".into(),
            ],
        },
        write_task_id,
    })
}

// ─── submit_written_article ──────────────────────────────────────────────────

/// Validate MDX on disk, register the article, complete the write task, spawn
/// cluster_and_link. On validation failure returns `ok: false` with checks —
/// the file is left in place for the agent to expand and resubmit.
///
/// Success is atomic: write-task Done + follow-ups run only after the article
/// is registered (`ingested` or already tracked under the content dir).
pub fn submit_written_article(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    path_or_slug: &str,
    opts: SubmitOpts,
) -> Result<SubmitResult, String> {
    if project_id.trim().is_empty() {
        return Err("project_id is required".to_string());
    }
    let path_or_slug = path_or_slug.trim();
    if path_or_slug.is_empty() {
        return Err("--file or --slug is required".to_string());
    }

    let file_path = crate::content::ops::resolve_slug_or_path(project_path, path_or_slug)
        .map_err(|e| e)?;

    if !file_path.is_file() {
        return Err(format!("File not found: {}", file_path.display()));
    }

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {e}", file_path.display()))?;

    let slug = extract_slug_from_mdx_or_path(&content, &file_path);

    // Strict write-task binding: explicit -I validates type/status; keyword
    // auto-lookup only binds active write_article tasks.
    let write_task = resolve_bound_write_task(conn, project_id, &opts)?;

    let target_keyword = opts
        .keyword
        .clone()
        .or_else(|| {
            write_task
                .as_ref()
                .and_then(crate::engine::post_actions::content_task_target_keyword)
        })
        .filter(|k| !k.trim().is_empty());

    // Fail-closed: if the valid-target catalog cannot load, do not auto-pass links
    // (CONTRACTS §13). Propagate as submit hard error rather than Option::None.
    let valid_link_targets = crate::engine::task_store::load_valid_link_targets(
        conn,
        project_id,
        &project_path.to_string_lossy(),
    )
    .map_err(|e| {
        format!("Failed to load valid link targets for write-submit (fail-closed): {e}")
    })?;

    let input = ValidateArticleInput {
        target_keyword: target_keyword.clone(),
        valid_link_targets: Some(valid_link_targets),
        min_word_count: Some(DEFAULT_MIN_WORD_COUNT),
    };
    let validation = validate_article_content(&slug, &content, &input);
    let checks = validation.checks.clone();

    if !validation.ok {
        return Ok(SubmitResult {
            ok: false,
            slug: Some(slug),
            path: Some(file_path.to_string_lossy().to_string()),
            validation: Some(validation),
            checks,
            ingested: false,
            write_task_id: write_task.as_ref().map(|t| t.id.clone()),
            write_task_status: write_task.as_ref().map(|t| t.status.as_str().to_string()),
            follow_up_task_ids: vec![],
            catalog_status: None,
            catalog_status_reason: None,
            message: Some(
                "Validation failed — expand the article (min 800 words, structure, meta) and resubmit."
                    .to_string(),
            ),
        });
    }

    // Hard-require: submitted file must live under the resolved content dir.
    let content_dir = resolve_content_dir_for_package(conn, project_id, project_path)?;
    if !path_is_under_dir(&file_path, &content_dir) {
        return Ok(SubmitResult {
            ok: false,
            slug: Some(slug),
            path: Some(file_path.to_string_lossy().to_string()),
            validation: Some(validation),
            checks,
            ingested: false,
            write_task_id: write_task.as_ref().map(|t| t.id.clone()),
            write_task_status: write_task.as_ref().map(|t| t.status.as_str().to_string()),
            follow_up_task_ids: vec![],
            catalog_status: None,
            catalog_status_reason: None,
            message: Some(format!(
                "Submitted file is outside the project content dir ({}). Write to the package target_file and resubmit.",
                content_dir.display()
            )),
        });
    }

    // Exact target_keyword collision hard-fail (issue #272) — before register so
    // a twin URL is never committed. Empty keyword skips the gate. Exclude self
    // by slug so re-submit of the same article does not false-positive.
    if let Some(ref kw) = target_keyword {
        let colliders = crate::content::article_index::find_target_keyword_collisions(
            conn,
            project_id,
            kw,
            Some(slug.as_str()),
            None,
        )
        .map_err(|e| format!("Failed to check target_keyword collisions: {e}"))?;
        if !colliders.is_empty() {
            let message =
                crate::content::article_index::format_keyword_collision_message(kw, &colliders);
            let mut checks = checks;
            checks.push(ArticleCheck {
                id: "target_keyword_unique".into(),
                pass: false,
                detail: Some(format!(
                    "colliders={}",
                    colliders
                        .iter()
                        .map(|c| format!(
                            "id={} slug={} page_type={}",
                            c.id,
                            c.url_slug,
                            c.page_type.as_deref().unwrap_or("unknown")
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                )),
            });
            return Ok(SubmitResult {
                ok: false,
                slug: Some(slug),
                path: Some(file_path.to_string_lossy().to_string()),
                validation: Some(validation),
                checks,
                ingested: false,
                write_task_id: write_task.as_ref().map(|t| t.id.clone()),
                write_task_status: write_task.as_ref().map(|t| t.status.as_str().to_string()),
                follow_up_task_ids: vec![],
                catalog_status: None,
                catalog_status_reason: None,
                message: Some(message),
            });
        }
    }

    // Single registration path (ingest → draft demote → keyword meta → export → tracked check).
    let outcome = register_submitted_article(
        conn,
        project_id,
        project_path,
        &file_path,
        write_task.as_ref(),
        target_keyword.as_deref(),
    )?;

    if !outcome.registered {
        return Ok(SubmitResult {
            ok: false,
            slug: Some(slug),
            path: Some(file_path.to_string_lossy().to_string()),
            validation: Some(validation),
            checks,
            ingested: false,
            write_task_id: write_task.as_ref().map(|t| t.id.clone()),
            write_task_status: write_task.as_ref().map(|t| t.status.as_str().to_string()),
            follow_up_task_ids: vec![],
            catalog_status: None,
            catalog_status_reason: None,
            message: Some(
                "Article validated but registration failed — file was not ingested and is not tracked. Check content dir and resubmit."
                    .to_string(),
            ),
        });
    }

    // Write-task completion + follow-ups only after successful registration.
    let (final_write_task_id, write_task_status) =
        complete_write_task_if_bound(conn, write_task.as_ref());

    let mut follow_up_task_ids = Vec::new();
    if let Some(ref task) = write_task {
        // Reload after status change so create_* sees done parent.
        let parent = crate::engine::task_store::get_task(conn, &task.id).unwrap_or_else(|_| {
            let mut t = task.clone();
            t.status = TaskStatus::Done;
            t
        });
        if let Some(id) = crate::engine::exec::content::create_cluster_and_link_task(
            conn,
            &parent,
            &project_path.to_string_lossy(),
        ) {
            follow_up_task_ids.push(id);
        }
        // Path B closed-loop: +30d content_outcome_review (issue #203).
        if let Some(id) = crate::engine::post_actions::spawn_content_outcome_review_for_slug(
            conn,
            &parent,
            &slug,
        ) {
            follow_up_task_ids.push(id);
        }
    } else {
        // Path B unbound: no write_task — synthesize a minimal parent so the
        // single spawn entrypoint owns focus attachment + idempotency.
        let topic = target_keyword
            .as_deref()
            .filter(|k| !k.is_empty())
            .unwrap_or(slug.as_str());
        let focus_slug = crate::content::slug::normalize_url_slug(&slug);
        let synthetic_parent = Task {
            id: format!("path-b:{project_id}:{focus_slug}"),
            task_type: "write_article".to_string(),
            phase: "implementation".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            title: Some(format!("Write article: {topic}")),
            description: Some(format!("Path B submit for {focus_slug}")),
            project_id: project_id.to_string(),
            artifacts: vec![
                crate::engine::exec::content::focus_slug_artifact(&focus_slug),
                TaskArtifact {
                    key: "url_slug".to_string(),
                    path: None,
                    artifact_type: Some("text".to_string()),
                    source: Some("write_spawn".to_string()),
                    content: Some(focus_slug.clone()),
                },
            ],
            ..Default::default()
        };
        if let Some(id) = crate::engine::exec::content::create_cluster_and_link_task(
            conn,
            &synthetic_parent,
            &project_path.to_string_lossy(),
        ) {
            follow_up_task_ids.push(id);
        }
        // Path B closed-loop: +30d content_outcome_review (issue #203).
        // Stable synthetic parent id keeps re-submit idempotent.
        if let Some(id) = crate::engine::post_actions::spawn_content_outcome_review_for_slug(
            conn,
            &synthetic_parent,
            &focus_slug,
        ) {
            follow_up_task_ids.push(id);
        }
    }

    let catalog_status_reason = if outcome.newly_ingested {
        Some(CATALOG_DRAFT_UNTIL_PUBLISH_REASON.to_string())
    } else {
        None
    };
    let message = if outcome.newly_ingested {
        "Article validated and registered. Catalog status is draft until publish-content -S <slug>."
            .to_string()
    } else {
        "Article validated and registered.".to_string()
    };

    Ok(SubmitResult {
        ok: true,
        slug: Some(slug),
        path: Some(file_path.to_string_lossy().to_string()),
        validation: Some(validation),
        checks,
        ingested: true,
        write_task_id: final_write_task_id,
        write_task_status,
        follow_up_task_ids,
        catalog_status: outcome.catalog_status,
        catalog_status_reason,
        message: Some(message),
    })
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn resolve_content_dir_for_package(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<PathBuf, String> {
    let content_dir_override = crate::engine::task_store::get_project(conn, project_id)
        .ok()
        .and_then(|p| p.content_dir)
        .filter(|s| !s.trim().is_empty());

    // Match agentic write path: locator first (optional project content_dir).
    let resolved =
        crate::content::locator::resolve(project_path, content_dir_override.as_deref());
    if let Some(dir) = resolved.selected {
        // Ensure directory exists for next_article_path / agent writes.
        if !dir.exists() {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("Failed to create content dir {}: {e}", dir.display()))?;
        }
        return Ok(dir);
    }

    // Fall back to seo_workspace.json / setup_check resolution.
    let automation_dir = project_path.join(".github").join("automation");
    match crate::content::ops::resolve_content_dir(&automation_dir, project_path) {
        Ok(dir) => {
            if !dir.exists() {
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("Failed to create content dir {}: {e}", dir.display()))?;
            }
            Ok(dir)
        }
        Err(e) => Err(e),
    }
}

fn path_relative_to_project(project_path: &Path, abs: &Path) -> String {
    abs.strip_prefix(project_path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| abs.to_string_lossy().to_string())
}

/// Look up an *active* write_article task via the select-keywords idempotency key.
///
/// Active = todo / queued / in_progress / failed / review.
/// Never returns Done or Cancelled (auto-bind must not resurrect those).
fn find_active_write_task_id(
    conn: &Connection,
    project_id: &str,
    normalized_keyword: &str,
) -> Option<String> {
    let key = format!("write_article:{project_id}:{normalized_keyword}");
    let task_id: String = conn
        .query_row(
            "SELECT task_id FROM task_idempotency_keys WHERE key = ?1",
            [&key],
            |r| r.get(0),
        )
        .ok()?;
    let task = crate::engine::task_store::get_task(conn, &task_id).ok()?;
    if task.project_id != project_id || task.task_type != "write_article" {
        return None;
    }
    match task.status {
        TaskStatus::Todo
        | TaskStatus::Queued
        | TaskStatus::InProgress
        | TaskStatus::Review
        | TaskStatus::Failed => Some(task.id),
        TaskStatus::Done | TaskStatus::Cancelled => None,
    }
}

/// Resolve write-task binding for submit.
///
/// - Explicit `-I`: must be `write_article` on this project; Cancelled errors;
///   Done is allowed (no-op completion later); wrong type errors.
/// - Keyword auto-lookup: only active write_article statuses.
fn resolve_bound_write_task(
    conn: &Connection,
    project_id: &str,
    opts: &SubmitOpts,
) -> Result<Option<Task>, String> {
    if let Some(ref id) = opts.write_task_id {
        let id = id.trim();
        if id.is_empty() {
            return Err("write task id is empty".to_string());
        }
        let task = crate::engine::task_store::get_task(conn, id)
            .map_err(|e| format!("Write task not found ({id}): {e}"))?;
        if task.project_id != project_id {
            return Err(format!(
                "Write task {id} does not belong to this project"
            ));
        }
        if task.task_type != "write_article" {
            return Err(format!(
                "Task {id} has type '{}', expected write_article — not marking done",
                task.task_type
            ));
        }
        if task.status == TaskStatus::Cancelled {
            return Err(format!(
                "Write task {id} is cancelled and cannot be completed via write-submit"
            ));
        }
        // Done: allow no-op completion; active statuses: complete after register.
        return Ok(Some(task));
    }

    if let Some(ref kw) = opts.keyword {
        let normalized = normalize_keyword(kw);
        if let Some(id) = find_active_write_task_id(conn, project_id, &normalized) {
            let task = crate::engine::task_store::get_task(conn, &id).ok();
            return Ok(task.filter(|t| {
                t.project_id == project_id && t.task_type == "write_article"
            }));
        }
    }
    Ok(None)
}

/// Unified article registration for Path B submit.
///
/// Always: `ingest_orphans` → force catalog `draft` for newly ingested files
/// (FM status is not catalog SoT — issue #168) → keyword meta when known →
/// export → tracked check.
fn register_submitted_article(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    file_path: &Path,
    write_task: Option<&Task>,
    target_keyword: Option<&str>,
) -> Result<RegisterOutcome, String> {
    let summary = crate::content::article_index::ingest_orphans(conn, project_id, project_path)
        .map_err(|e| format!("Article registration failed: {e}"))?;

    // Prefer full KD/volume from write-task description; fall back to bare keyword.
    let (keyword, kd_str, vol) = if let Some(task) = write_task {
        let (k, kd, v) = crate::engine::post_actions::parse_content_task_keyword_meta(task);
        if k.is_some() {
            (k, kd, v)
        } else if let Some(kw) = target_keyword {
            (Some(kw.to_string()), kd, v)
        } else {
            (None, kd, v)
        }
    } else if let Some(kw) = target_keyword {
        (Some(kw.to_string()), None, 0i64)
    } else {
        (None, None, 0i64)
    };

    // Keyword meta applies only to the submitted file — co-ingested orphans
    // (e.g. locator seed MDX) must not inherit K, or re-submit would false-
    // positive the exact-keyword collision gate (issue #272). Shared with
    // nested `ingest_content_write_files` so the rule cannot drift.
    let submitted_basename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // NOTE: catalog draft until publish; FM status ignored here (issue #168).
    // Always demote newly ingested Path B articles to draft until CLI
    // `publish-content` (#257) — not only when keyword meta is present.
    if summary.ingested > 0 {
        crate::engine::post_actions::demote_ingested_to_draft_with_optional_keyword(
            conn,
            project_id,
            &summary.files,
            if submitted_basename.is_empty() {
                None
            } else {
                Some(submitted_basename)
            },
            keyword.as_deref(),
            kd_str.as_deref(),
            vol,
        );
    }

    let _ = crate::content::article_index::export_projection(conn, project_id, project_path);

    let newly_ingested = summary.ingested > 0;
    let registered = newly_ingested || article_tracked(conn, project_id, file_path);
    let catalog_status = if registered {
        lookup_article_status(conn, project_id, file_path)
    } else {
        None
    };

    Ok(RegisterOutcome {
        registered,
        newly_ingested,
        catalog_status,
    })
}

/// Read `articles.status` for the submitted file basename (cheap post-register read).
fn lookup_article_status(
    conn: &Connection,
    project_id: &str,
    file_path: &Path,
) -> Option<String> {
    let basename = file_path.file_name().and_then(|n| n.to_str())?;
    if basename.is_empty() {
        return None;
    }
    conn.query_row(
        "SELECT status FROM articles WHERE project_id = ?1 AND file LIKE ?2 LIMIT 1",
        rusqlite::params![project_id, format!("%{basename}")],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

/// Mark bound write_article Done after successful registration.
/// Explicit Done is a no-op (status already done). Never called for Cancelled.
fn complete_write_task_if_bound(
    conn: &Connection,
    write_task: Option<&Task>,
) -> (Option<String>, Option<String>) {
    let Some(task) = write_task else {
        return (None, None);
    };
    debug_assert_eq!(task.task_type, "write_article");
    if task.status == TaskStatus::Done {
        return (Some(task.id.clone()), Some(TaskStatus::Done.as_str().to_string()));
    }
    match crate::engine::task_store::update_task_status(conn, &task.id, TaskStatus::Done) {
        Ok(updated) => (
            Some(updated.id),
            Some(updated.status.as_str().to_string()),
        ),
        Err(e) => {
            log::warn!(
                "[write_package] failed to mark write task {} done: {}",
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

fn path_is_under_dir(file: &Path, dir: &Path) -> bool {
    let file_c = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let dir_c = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    file_c.starts_with(&dir_c)
}

fn extract_slug_from_mdx_or_path(content: &str, file_path: &Path) -> String {
    if let Some((fm, _)) = crate::content::frontmatter::split_mdx(content) {
        if let Ok(parsed) = crate::content::frontmatter::parse(fm) {
            if let Some(s) = parsed.parsed.get("slug").and_then(|v| v.as_str()) {
                let clean = s.trim().trim_matches('"').trim_matches('\'');
                if !clean.is_empty() {
                    return crate::content::slug::normalize_url_slug(clean);
                }
            }
        }
    }
    let stem = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("article");
    crate::content::slug::normalize_url_slug(stem)
}

fn article_tracked(conn: &Connection, project_id: &str, file_path: &Path) -> bool {
    let basename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if basename.is_empty() {
        return false;
    }
    conn.query_row(
        "SELECT 1 FROM articles WHERE project_id = ?1 AND file LIKE ?2 LIMIT 1",
        rusqlite::params![project_id, format!("%{basename}")],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::TaskStatus;
    use std::fs;
    use uuid::Uuid;

    struct TempProjectDir {
        path: PathBuf,
    }

    impl TempProjectDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("pageseeds-write-pkg-{}", Uuid::new_v4()));
            fs::create_dir_all(path.join(".github").join("automation")).unwrap();
            fs::create_dir_all(path.join("content").join("blog")).unwrap();
            fs::write(
                path.join(".github")
                    .join("automation")
                    .join("seo_workspace.json"),
                r#"{"content_dir":"content/blog"}"#,
            )
            .unwrap();
            // Seed one MDX so locator auto-discovery would also work.
            fs::write(
                path.join("content").join("blog").join("000_seed.mdx"),
                "---\ntitle: Seed\ndescription: A seed article used only so content dir discovery works for tests in this suite.\nslug: seed\ndate: \"2024-01-01\"\n---\n\n# Seed\n\nseed body for discovery.\n",
            )
            .unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempProjectDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn in_memory_db(project_path: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                content_dir TEXT,
                site_url TEXT,
                site_id TEXT,
                sitemap_url TEXT,
                project_mode TEXT NOT NULL DEFAULT 'workspace',
                active INTEGER DEFAULT 1,
                agent_provider TEXT,
                seo_provider TEXT,
                clarity_project_id TEXT
            );
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
                review_surface TEXT NOT NULL DEFAULT 'none',
                follow_up_policy TEXT NOT NULL DEFAULT 'none',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT,
                description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER DEFAULT 0,
                run_last_error TEXT,
                run_provider TEXT,
                not_before TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE task_idempotency_keys (
                key TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT
            );
            CREATE TABLE task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                attempt INTEGER NOT NULL,
                provider TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                success INTEGER,
                error TEXT,
                prompt_tokens INTEGER,
                completion_tokens INTEGER
            );
            CREATE TABLE articles (
                id INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                url_slug TEXT NOT NULL DEFAULT '',
                file TEXT NOT NULL DEFAULT '',
                target_keyword TEXT,
                keyword_difficulty TEXT,
                target_volume INTEGER DEFAULT 0,
                published_date TEXT,
                word_count INTEGER DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'draft',
                review_status TEXT,
                review_started_at TEXT,
                last_reviewed_at TEXT,
                review_count INTEGER NOT NULL DEFAULT 0,
                content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
                estimated_traffic_monthly TEXT,
                page_type TEXT,
                content_hash TEXT,
                last_edited_at TEXT,
                project_id TEXT NOT NULL,
                PRIMARY KEY (id, project_id)
            );
            CREATE TABLE articles_meta (
                project_id TEXT PRIMARY KEY,
                next_article_id INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE article_metadata (
                project_id TEXT NOT NULL,
                article_id INTEGER NOT NULL,
                namespace TEXT NOT NULL,
                payload TEXT NOT NULL DEFAULT '{}',
                updated_at TEXT NOT NULL,
                PRIMARY KEY (project_id, article_id, namespace)
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, content_dir, active)
             VALUES ('proj1', 'Test', ?1, 'content/blog', 1)",
            [project_path],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES ('proj1', 1)",
            [],
        )
        .unwrap();
        conn
    }

    fn research_artifact() -> TaskArtifact {
        TaskArtifact {
            key: "research_final_selection".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: None,
            content: Some(
                serde_json::json!({
                    "difficulty": {
                        "results": [
                            {
                                "keyword": "seo tools",
                                "difficulty": 30,
                                "volume": "5,000-10,000",
                                "intent": "informational",
                                "recommended_title": "Best SEO Tools",
                                "selection_reason": "clear demand",
                                "winnability": "target",
                                "winnability_reason": "moderate KD"
                            }
                        ]
                    }
                })
                .to_string(),
            ),
        }
    }

    fn insert_research_task(conn: &Connection) -> String {
        TaskSpawner::spawn(
            conn,
            TaskSpec {
                project_id: "proj1".to_string(),
                task_type: "research_keywords".to_string(),
                artifacts: vec![research_artifact()],
                ..Default::default()
            },
        )
        .unwrap()
        .id
    }

    fn meta_ok() -> String {
        // 120–155 chars for meta_description_length check
        "A comprehensive guide covering the best seo tools for modern teams seeking better rankings and workflow efficiency today."
            .to_string()
    }

    fn short_article_mdx(keyword: &str) -> String {
        format!(
            "---\ntitle: Best SEO Tools\ndescription: {}\nslug: seo-tools\ndate: \"2024-06-01\"\nstatus: draft\n---\n\n# Best SEO Tools\n\n{keyword} intro only.\n",
            meta_ok()
        )
    }

    fn long_article_mdx(keyword: &str) -> String {
        // count_words strips markdown; pad body past 800.
        let pad = "word ".repeat(850);
        format!(
            "---\ntitle: Best SEO Tools\ndescription: {}\nslug: seo-tools\ndate: \"2024-06-01\"\nstatus: draft\n---\n\n# Best SEO Tools\n\n{keyword} guide for operators.\n\n{pad}\n",
            meta_ok()
        )
    }

    /// Long MDX with frontmatter `status: published` (must not become catalog published on Path B).
    fn long_article_mdx_fm_published(keyword: &str) -> String {
        let pad = "word ".repeat(850);
        format!(
            "---\ntitle: Best SEO Tools\ndescription: {}\nslug: seo-tools\ndate: \"2024-06-01\"\nstatus: published\n---\n\n# Best SEO Tools\n\n{keyword} guide for operators.\n\n{pad}\n",
            meta_ok()
        )
    }

    #[test]
    fn build_package_emits_brief_path_skill_and_word_floors() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);

        let pkg = build_write_package(
            &conn,
            "proj1",
            tmp.path(),
            &research_id,
            "seo tools",
        )
        .expect("package should build without LLM");

        assert_eq!(pkg.keyword, "seo tools");
        assert_eq!(pkg.project_id, "proj1");
        assert_eq!(pkg.research_task_id, research_id);
        assert_eq!(pkg.content_brief.keyword, "seo tools");
        assert_eq!(pkg.content_brief.difficulty, Some(30));
        assert_eq!(pkg.min_words, 800);
        assert_eq!(pkg.target_words, 1200);
        assert_eq!(pkg.constraints.min_word_count, 800);
        assert_eq!(pkg.constraints.target_word_count, 1200);
        assert_eq!(pkg.skill_name, "content-write");
        // Embedded skill should load even without project override.
        assert!(
            pkg.skill_content
                .as_ref()
                .map(|c| !c.is_empty())
                .unwrap_or(false),
            "skill_content should include content-write body"
        );
        assert!(
            pkg.target_file.ends_with(".mdx"),
            "target_file={}",
            pkg.target_file
        );
        assert!(
            pkg.target_path.contains("seo_tools") || pkg.target_path.contains("seo-tools"),
            "target_path={}",
            pkg.target_path
        );
        assert!(
            pkg.target_path.starts_with("content/") || pkg.target_file.contains("content"),
            "path under content dir: {} / {}",
            pkg.target_path,
            pkg.target_file
        );
        // No write task yet from select-keywords.
        assert!(pkg.write_task_id.is_none());
    }

    #[test]
    fn build_package_rejects_keyword_outside_selection() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);

        let err = build_write_package(
            &conn,
            "proj1",
            tmp.path(),
            &research_id,
            "not in list",
        )
        .unwrap_err();
        assert!(
            err.contains("outside the workflow selection list"),
            "err={err}"
        );
    }

    #[test]
    fn build_package_finds_existing_write_task() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);

        let tasks = crate::engine::keyword_selection::create_article_tasks_from_keywords(
            &conn,
            "proj1",
            &research_id,
            vec!["seo tools".into()],
        )
        .unwrap();
        assert_eq!(tasks.len(), 1);

        let pkg = build_write_package(
            &conn,
            "proj1",
            tmp.path(),
            &research_id,
            "seo tools",
        )
        .unwrap();
        assert_eq!(pkg.write_task_id.as_deref(), Some(tasks[0].id.as_str()));
    }

    #[test]
    fn submit_fails_structured_when_body_under_800_words() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, short_article_mdx("seo tools")).unwrap();

        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts {
                keyword: Some("seo tools".into()),
                ..Default::default()
            },
        )
        .expect("submit should return structured failure, not domain Err");

        assert!(!result.ok);
        assert!(!result.ingested);
        assert!(result.follow_up_task_ids.is_empty());
        let min_check = result
            .checks
            .iter()
            .find(|c| c.id == "min_word_count")
            .expect("min_word_count check");
        assert!(!min_check.pass, "short body must fail min_word_count");
        // File must still exist for agent resubmit.
        assert!(file.is_file());
    }

    #[test]
    fn submit_succeeds_on_valid_long_mdx_and_registers() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);
        let write_tasks = crate::engine::keyword_selection::create_article_tasks_from_keywords(
            &conn,
            "proj1",
            &research_id,
            vec!["seo tools".into()],
        )
        .unwrap();
        let write_id = write_tasks[0].id.clone();

        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts {
                write_task_id: Some(write_id.clone()),
                keyword: Some("seo tools".into()),
            },
        )
        .expect("submit should succeed");

        assert!(result.ok, "checks={:?}", result.checks);
        assert_eq!(result.slug.as_deref(), Some("seo-tools"));
        assert!(result.ingested, "article should be registered");
        assert_eq!(result.write_task_id.as_deref(), Some(write_id.as_str()));
        assert_eq!(result.write_task_status.as_deref(), Some("done"));
        assert_eq!(result.catalog_status.as_deref(), Some("draft"));
        assert_eq!(
            result.catalog_status_reason.as_deref(),
            Some(CATALOG_DRAFT_UNTIL_PUBLISH_REASON)
        );
        assert!(
            result
                .message
                .as_deref()
                .unwrap_or("")
                .contains("draft until publish-content"),
            "message={:?}",
            result.message
        );
        assert!(
            !result.follow_up_task_ids.is_empty(),
            "cluster_and_link should spawn"
        );

        let write = crate::engine::task_store::get_task(&conn, &write_id).unwrap();
        assert_eq!(write.status, TaskStatus::Done);

        let articles = crate::engine::task_store::list_articles(&conn, "proj1").unwrap();
        assert!(
            articles.iter().any(|a| a.url_slug == "seo-tools"
                || a.file.contains("seo_tools")
                || a.target_keyword.as_deref() == Some("seo tools")),
            "registered articles={:?}",
            articles
                .iter()
                .map(|a| (&a.url_slug, &a.file, &a.target_keyword))
                .collect::<Vec<_>>()
        );
        let submitted = articles
            .iter()
            .find(|a| a.file.contains("seo_tools") || a.url_slug == "seo-tools")
            .expect("submitted article row");
        assert_eq!(
            submitted.status, "draft",
            "Path B catalog must be draft after submit"
        );
    }

    /// Issue #168: FM `status: published` must not leave catalog as published.
    /// Draft demote is unconditional — even with no keyword meta.
    #[test]
    fn submit_forces_catalog_draft_even_when_fm_published_and_no_keyword_meta() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx_fm_published("seo tools")).unwrap();

        // No keyword / write_task — previously skipped the draft UPDATE branch.
        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts::default(),
        )
        .expect("submit should succeed without keyword meta");

        assert!(result.ok, "checks={:?}", result.checks);
        assert!(result.ingested);
        assert_eq!(result.catalog_status.as_deref(), Some("draft"));
        assert_eq!(
            result.catalog_status_reason.as_deref(),
            Some(CATALOG_DRAFT_UNTIL_PUBLISH_REASON)
        );
        assert!(
            result
                .message
                .as_deref()
                .unwrap_or("")
                .contains("draft until publish-content"),
            "message={:?}",
            result.message
        );

        let articles = crate::engine::task_store::list_articles(&conn, "proj1").unwrap();
        let submitted = articles
            .iter()
            .find(|a| a.file.contains("seo_tools") || a.url_slug == "seo-tools")
            .expect("submitted article row");
        assert_eq!(
            submitted.status, "draft",
            "FM status:published must not become catalog published on Path B; got {:?}",
            submitted.status
        );
    }

    /// Focused registration invariant: newly ingested files always get draft
    /// even when keyword meta is absent (issue #168).
    #[test]
    fn register_submitted_article_always_drafts_new_ingest_without_keyword() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("orphan_pub.mdx");
        fs::write(
            &file,
            "---\ntitle: Orphan\ndescription: A short orphan body for registration-only draft invariant tests without full submit floors.\nslug: orphan-pub\ndate: \"2024-06-02\"\nstatus: published\n---\n\n# Orphan\n\nbody.\n",
        )
        .unwrap();

        let outcome = register_submitted_article(
            &conn,
            "proj1",
            tmp.path(),
            &file,
            None,
            None, // no keyword meta
        )
        .expect("register");

        assert!(outcome.registered);
        assert!(outcome.newly_ingested);
        assert_eq!(outcome.catalog_status.as_deref(), Some("draft"));

        let status: String = conn
            .query_row(
                "SELECT status FROM articles WHERE project_id='proj1' AND file LIKE '%orphan_pub.mdx'",
                [],
                |row| row.get(0),
            )
            .expect("row after register");
        assert_eq!(status, "draft");
    }

    #[test]
    fn submit_standalone_spawns_path_b_cluster_without_parent() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            "seo-tools", // resolve by slug after we write the file
            SubmitOpts {
                keyword: Some("seo tools".into()),
                ..Default::default()
            },
        )
        .expect("submit without write task");

        // resolve by slug needs the file discoverable via find_file_by_slug —
        // absolute path fallback if slug lookup fails in fixture.
        // If slug path failed we wouldn't get here; ok path asserts:
        if !result.ok {
            // Retry with absolute path for environments where slug lookup differs.
            let result = submit_written_article(
                &conn,
                "proj1",
                tmp.path(),
                file.to_str().unwrap(),
                SubmitOpts {
                    keyword: Some("seo tools".into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(result.ok, "checks={:?}", result.checks);
            assert!(!result.follow_up_task_ids.is_empty());
            assert!(result.write_task_id.is_none());
            let cluster =
                crate::engine::task_store::get_task(&conn, &result.follow_up_task_ids[0]).unwrap();
            assert_eq!(cluster.task_type, "cluster_and_link");
            let focus = cluster
                .artifacts
                .iter()
                .find(|a| a.key == "focus_slug")
                .and_then(|a| a.content.as_deref());
            assert_eq!(focus, Some("seo-tools"));
            return;
        }
        assert!(result.ok, "checks={:?}", result.checks);
        assert!(!result.follow_up_task_ids.is_empty());
        assert!(result.write_task_id.is_none());
        let cluster =
            crate::engine::task_store::get_task(&conn, &result.follow_up_task_ids[0]).unwrap();
        assert_eq!(cluster.task_type, "cluster_and_link");
        let focus = cluster
            .artifacts
            .iter()
            .find(|a| a.key == "focus_slug")
            .and_then(|a| a.content.as_deref());
        assert_eq!(
            focus,
            Some("seo-tools"),
            "Path B unbound must attach focus_slug via create_cluster_and_link_task"
        );
    }

    #[test]
    fn submit_rejects_file_outside_content_dir() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        // Write valid long MDX outside content/blog.
        let outside = tmp.path().join("elsewhere");
        fs::create_dir_all(&outside).unwrap();
        let file = outside.join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts {
                keyword: Some("seo tools".into()),
                ..Default::default()
            },
        )
        .expect("structured failure, not domain Err");

        assert!(!result.ok);
        assert!(!result.ingested);
        assert!(result.follow_up_task_ids.is_empty());
        assert!(
            result
                .message
                .as_deref()
                .unwrap_or("")
                .contains("outside the project content dir"),
            "message={:?}",
            result.message
        );
    }

    #[test]
    fn submit_explicit_wrong_task_type_errors_without_marking_done() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);
        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let err = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts {
                write_task_id: Some(research_id.clone()),
                keyword: Some("seo tools".into()),
            },
        )
        .unwrap_err();
        assert!(
            err.contains("expected write_article"),
            "err={err}"
        );
        let research = crate::engine::task_store::get_task(&conn, &research_id).unwrap();
        assert_ne!(research.status, TaskStatus::Done);
    }

    #[test]
    fn submit_explicit_done_write_task_is_noop_completion() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);
        let write_tasks = crate::engine::keyword_selection::create_article_tasks_from_keywords(
            &conn,
            "proj1",
            &research_id,
            vec!["seo tools".into()],
        )
        .unwrap();
        let write_id = write_tasks[0].id.clone();
        crate::engine::task_store::update_task_status(&conn, &write_id, TaskStatus::Done).unwrap();

        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts {
                write_task_id: Some(write_id.clone()),
                keyword: Some("seo tools".into()),
            },
        )
        .expect("Done write_article should no-op complete");

        assert!(result.ok, "checks={:?}", result.checks);
        assert!(result.ingested);
        assert_eq!(result.write_task_id.as_deref(), Some(write_id.as_str()));
        assert_eq!(result.write_task_status.as_deref(), Some("done"));
        let write = crate::engine::task_store::get_task(&conn, &write_id).unwrap();
        assert_eq!(write.status, TaskStatus::Done);
    }

    #[test]
    fn submit_explicit_cancelled_write_task_errors() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);
        let write_tasks = crate::engine::keyword_selection::create_article_tasks_from_keywords(
            &conn,
            "proj1",
            &research_id,
            vec!["seo tools".into()],
        )
        .unwrap();
        let write_id = write_tasks[0].id.clone();
        crate::engine::task_store::update_task_status(&conn, &write_id, TaskStatus::Cancelled)
            .unwrap();

        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let err = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts {
                write_task_id: Some(write_id.clone()),
                keyword: Some("seo tools".into()),
            },
        )
        .unwrap_err();
        assert!(err.contains("cancelled"), "err={err}");
        let write = crate::engine::task_store::get_task(&conn, &write_id).unwrap();
        assert_eq!(write.status, TaskStatus::Cancelled);
    }

    #[test]
    fn find_active_write_task_skips_done_and_cancelled() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);
        let write_tasks = crate::engine::keyword_selection::create_article_tasks_from_keywords(
            &conn,
            "proj1",
            &research_id,
            vec!["seo tools".into()],
        )
        .unwrap();
        let write_id = write_tasks[0].id.clone();
        let normalized = normalize_keyword("seo tools");

        assert_eq!(
            find_active_write_task_id(&conn, "proj1", &normalized).as_deref(),
            Some(write_id.as_str())
        );

        crate::engine::task_store::update_task_status(&conn, &write_id, TaskStatus::Done).unwrap();
        assert!(find_active_write_task_id(&conn, "proj1", &normalized).is_none());

        crate::engine::task_store::update_task_status(&conn, &write_id, TaskStatus::Cancelled)
            .unwrap();
        assert!(find_active_write_task_id(&conn, "proj1", &normalized).is_none());
    }

    /// Issue #203: bound write-submit spawns one +30d content_outcome_review.
    #[test]
    fn submit_bound_spawns_content_outcome_review() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let research_id = insert_research_task(&conn);
        let write_tasks = crate::engine::keyword_selection::create_article_tasks_from_keywords(
            &conn,
            "proj1",
            &research_id,
            vec!["seo tools".into()],
        )
        .unwrap();
        let write_id = write_tasks[0].id.clone();

        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts {
                write_task_id: Some(write_id.clone()),
                keyword: Some("seo tools".into()),
            },
        )
        .expect("submit should succeed");

        assert!(result.ok, "checks={:?}", result.checks);
        let reviews: Vec<_> = result
            .follow_up_task_ids
            .iter()
            .filter_map(|id| crate::engine::task_store::get_task(&conn, id).ok())
            .filter(|t| t.task_type == "content_outcome_review")
            .collect();
        assert_eq!(
            reviews.len(),
            1,
            "exactly one content_outcome_review; follow_ups={:?}",
            result.follow_up_task_ids
        );
        let review = &reviews[0];
        assert_not_before_approx_30d(review.not_before.as_deref());
        assert_content_outcome_target(review, "seo-tools", Some(write_id.as_str()));
    }

    /// Issue #272: Path B hard-fails when another article already owns exact keyword.
    #[test]
    fn submit_fails_on_exact_target_keyword_collision_before_register() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());

        // Existing owner of "seo tools" with a different slug.
        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, status,
                content_gaps_addressed, project_id, page_type
             ) VALUES (1, 'SEO Tools Hub', 'hub-seo-tools', './content/blog/hub_seo_tools.mdx',
                       'seo tools', 'published', '[]', 'proj1', 'hub')",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE articles_meta SET next_article_id = 2 WHERE project_id = 'proj1'",
            [],
        )
        .unwrap();

        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools_twin.mdx");
        // Long enough to pass structural floors; keyword still collides.
        let pad = "word ".repeat(850);
        let mdx = format!(
            "---\ntitle: SEO Tools Twin\ndescription: {}\nslug: seo-tools-twin\ndate: \"2024-06-01\"\nstatus: draft\n---\n\n# SEO Tools Twin\n\nseo tools guide for operators.\n\n{pad}\n",
            meta_ok()
        );
        fs::write(&file, mdx).unwrap();

        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts {
                keyword: Some("seo tools".into()),
                ..Default::default()
            },
        )
        .expect("structured fail, not domain Err");

        assert!(!result.ok, "collision must hard-fail");
        assert!(!result.ingested, "must not register twin");
        assert!(result.follow_up_task_ids.is_empty());
        let msg = result.message.as_deref().unwrap_or("");
        assert!(msg.contains("hub-seo-tools"), "msg={msg}");
        assert!(msg.contains("Retarget") || msg.contains("retarget"), "msg={msg}");
        assert!(
            msg.contains("consolidate_cluster") || msg.contains("Consolidate"),
            "msg={msg}"
        );
        let unique = result
            .checks
            .iter()
            .find(|c| c.id == "target_keyword_unique")
            .expect("target_keyword_unique check");
        assert!(!unique.pass);

        // Twin must not appear in catalog.
        let articles = crate::engine::task_store::list_articles(&conn, "proj1").unwrap();
        assert!(
            !articles.iter().any(|a| a.url_slug == "seo-tools-twin"
                || a.file.contains("seo_tools_twin")),
            "twin must not be registered: {:?}",
            articles.iter().map(|a| &a.url_slug).collect::<Vec<_>>()
        );
        // File left for operator retarget/resubmit.
        assert!(file.is_file());
    }

    /// Issue #272: re-submit of the same slug/keyword does not false-positive.
    #[test]
    fn submit_same_article_keyword_does_not_collide_with_self() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let opts = SubmitOpts {
            keyword: Some("seo tools".into()),
            ..Default::default()
        };
        let first = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            opts.clone(),
        )
        .expect("first submit");
        assert!(first.ok, "checks={:?}", first.checks);
        assert!(first.ingested);

        let second = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            opts,
        )
        .expect("re-submit");
        assert!(
            second.ok,
            "re-submit of same slug must not collision-fail: msg={:?} checks={:?}",
            second.message,
            second.checks
        );
    }

    /// Issue #272: empty/missing keyword skips the collision gate.
    #[test]
    fn submit_without_keyword_skips_collision_gate() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());

        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, status,
                content_gaps_addressed, project_id
             ) VALUES (1, 'Owned', 'owned-kw', './content/blog/owned.mdx',
                       'seo tools', 'published', '[]', 'proj1')",
            [],
        )
        .unwrap();

        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("no_keyword.mdx");
        let pad = "word ".repeat(850);
        let mdx = format!(
            "---\ntitle: No Keyword Article\ndescription: {}\nslug: no-keyword\ndate: \"2024-06-01\"\nstatus: draft\n---\n\n# No Keyword Article\n\nGuide without an explicit target keyword for collision skip tests.\n\n{pad}\n",
            meta_ok()
        );
        fs::write(&file, mdx).unwrap();

        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            SubmitOpts::default(), // no keyword
        )
        .expect("submit");
        assert!(
            result.ok,
            "empty keyword must skip collision gate: msg={:?} checks={:?}",
            result.message,
            result.checks
        );
        assert!(result.ingested);
    }

    /// Issue #203: unbound write-submit spawns review with synthetic parent idempotency.
    #[test]
    fn submit_unbound_spawns_content_outcome_review_and_is_idempotent() {
        let tmp = TempProjectDir::new();
        let conn = in_memory_db(tmp.path().to_str().unwrap());
        let file = tmp
            .path()
            .join("content")
            .join("blog")
            .join("seo_tools.mdx");
        fs::write(&file, long_article_mdx("seo tools")).unwrap();

        let opts = SubmitOpts {
            keyword: Some("seo tools".into()),
            ..Default::default()
        };
        let result = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            opts.clone(),
        )
        .expect("first submit");
        assert!(result.ok, "checks={:?}", result.checks);

        let review_ids: Vec<_> = result
            .follow_up_task_ids
            .iter()
            .filter_map(|id| crate::engine::task_store::get_task(&conn, id).ok())
            .filter(|t| t.task_type == "content_outcome_review")
            .map(|t| t.id)
            .collect();
        assert_eq!(review_ids.len(), 1, "first submit spawns one review");
        let first_id = review_ids[0].clone();
        let review = crate::engine::task_store::get_task(&conn, &first_id).unwrap();
        assert_not_before_approx_30d(review.not_before.as_deref());
        assert_content_outcome_target(&review, "seo-tools", Some("path-b:proj1:seo-tools"));

        // Re-submit: still one active review (idempotent synthetic parent + slug).
        let result2 = submit_written_article(
            &conn,
            "proj1",
            tmp.path(),
            file.to_str().unwrap(),
            opts,
        )
        .expect("re-submit");
        assert!(result2.ok, "checks={:?}", result2.checks);

        let all_reviews: Vec<_> = crate::engine::task_store::list_tasks(&conn, "proj1")
            .unwrap()
            .into_iter()
            .filter(|t| t.task_type == "content_outcome_review")
            .collect();
        assert_eq!(
            all_reviews.len(),
            1,
            "re-submit must not create a second content_outcome_review"
        );
        assert_eq!(all_reviews[0].id, first_id);
    }

    fn assert_not_before_approx_30d(not_before: Option<&str>) {
        let nb = not_before.expect("not_before set on content_outcome_review");
        let parsed = chrono::DateTime::parse_from_rfc3339(nb)
            .expect("not_before is RFC3339")
            .with_timezone(&chrono::Utc);
        let days = (parsed - chrono::Utc::now()).num_days();
        assert!(
            (28..=32).contains(&days),
            "not_before should be ~+30d, got {days} days (raw={nb})"
        );
    }

    fn assert_content_outcome_target(task: &Task, slug: &str, parent_id: Option<&str>) {
        let art = task
            .artifacts
            .iter()
            .find(|a| a.key == "content_outcome_target")
            .expect("content_outcome_target artifact");
        let v: serde_json::Value =
            serde_json::from_str(art.content.as_deref().unwrap_or("{}")).unwrap();
        assert_eq!(v["slug"].as_str(), Some(slug));
        if let Some(pid) = parent_id {
            assert_eq!(v["parent_task_id"].as_str(), Some(pid));
        }
    }
}
