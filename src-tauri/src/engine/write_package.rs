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
use crate::models::task::{AgentPolicy, Priority, Task, TaskRunPolicy, TaskStatus};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

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

    // Prefer keyword from opts, then write-task description, then frontmatter.
    let write_task_id = opts
        .write_task_id
        .clone()
        .or_else(|| {
            opts.keyword
                .as_ref()
                .map(|k| normalize_keyword(k))
                .and_then(|n| find_active_write_task_id(conn, project_id, &n))
        });

    let write_task: Option<Task> = write_task_id
        .as_ref()
        .and_then(|id| crate::engine::task_store::get_task(conn, id).ok())
        .filter(|t| t.project_id == project_id);

    let target_keyword = opts
        .keyword
        .clone()
        .or_else(|| {
            write_task
                .as_ref()
                .and_then(crate::engine::post_actions::content_task_target_keyword)
        })
        .filter(|k| !k.trim().is_empty());

    let valid_link_targets = crate::engine::task_store::load_valid_link_targets(
        conn,
        project_id,
        &project_path.to_string_lossy(),
    )
    .ok();

    let input = ValidateArticleInput {
        target_keyword: target_keyword.clone(),
        valid_link_targets,
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
            message: Some(
                "Validation failed — expand the article (min 800 words, structure, meta) and resubmit."
                    .to_string(),
            ),
        });
    }

    // Ensure file is under the project content dir; if agent wrote elsewhere
    // under the project tree, still ingest from its location when discoverable.
    // Prefer writing to package target_file; we do not force-move here so agents
    // keep the path they were given.
    let content_dir = resolve_content_dir_for_package(conn, project_id, project_path).ok();
    if let Some(ref dir) = content_dir {
        if !file_path.starts_with(dir) {
            log::warn!(
                "[write_package] submitted file {} is outside content dir {}; ingest may miss it if not under content tree",
                file_path.display(),
                dir.display()
            );
        }
    }

    // Register article: ingest orphans + keyword tag + export.
    let ingested = if let Some(ref task) = write_task {
        match crate::engine::post_actions::ingest_content_write_files(conn, task, project_path) {
            Ok(summary) => {
                if summary.ingested > 0 {
                    true
                } else {
                    // File may already be registered (resubmit after pass) — still export.
                    let _ = crate::content::article_index::export_projection(
                        conn,
                        project_id,
                        project_path,
                    );
                    // Treat already-tracked file under content dir as success path.
                    article_tracked(conn, project_id, &file_path)
                }
            }
            Err(e) => {
                return Err(format!("Article registration failed: {e}"));
            }
        }
    } else {
        match crate::content::article_index::ingest_orphans(conn, project_id, project_path) {
            Ok(summary) => {
                if let Some(ref kw) = target_keyword {
                    for filename in &summary.files {
                        let _ = conn.execute(
                            "UPDATE articles
                             SET target_keyword=?1, status='draft'
                             WHERE project_id=?2 AND file LIKE ?3",
                            rusqlite::params![kw, project_id, format!("%{filename}")],
                        );
                    }
                }
                let _ =
                    crate::content::article_index::export_projection(conn, project_id, project_path);
                if summary.ingested > 0 {
                    true
                } else {
                    article_tracked(conn, project_id, &file_path)
                }
            }
            Err(e) => {
                return Err(format!("Article registration failed: {e}"));
            }
        }
    };

    // Mark write task done so the queue does not re-run the nested writer.
    let mut write_task_status: Option<String> = None;
    let mut final_write_task_id = write_task.as_ref().map(|t| t.id.clone());
    if let Some(ref task) = write_task {
        match crate::engine::task_store::update_task_status(conn, &task.id, TaskStatus::Done) {
            Ok(updated) => {
                write_task_status = Some(updated.status.as_str().to_string());
                final_write_task_id = Some(updated.id);
            }
            Err(e) => {
                log::warn!(
                    "[write_package] failed to mark write task {} done: {}",
                    task.id,
                    e
                );
                write_task_status = Some(task.status.as_str().to_string());
            }
        }
    }

    // Spawn cluster_and_link follow-up.
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
    } else {
        if let Some(id) = spawn_standalone_cluster_and_link(conn, project_id, &slug, &target_keyword)
        {
            follow_up_task_ids.push(id);
        }
    }

    Ok(SubmitResult {
        ok: true,
        slug: Some(slug),
        path: Some(file_path.to_string_lossy().to_string()),
        validation: Some(validation),
        checks,
        ingested,
        write_task_id: final_write_task_id,
        write_task_status,
        follow_up_task_ids,
        message: Some("Article validated and registered.".to_string()),
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

/// Look up an active write_article task via the select-keywords idempotency key.
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
    // Prefer active tasks; still return done ones so submit can no-op status.
    match task.status {
        TaskStatus::Todo
        | TaskStatus::Queued
        | TaskStatus::InProgress
        | TaskStatus::Review
        | TaskStatus::Failed => Some(task.id),
        TaskStatus::Done | TaskStatus::Cancelled => Some(task.id),
    }
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

fn spawn_standalone_cluster_and_link(
    conn: &Connection,
    project_id: &str,
    slug: &str,
    keyword: &Option<String>,
) -> Option<String> {
    let topic = keyword
        .as_deref()
        .filter(|k| !k.is_empty())
        .unwrap_or(slug);
    let title = format!("Cluster and link: {topic}");
    let description = format!(
        "Scan internal link graph and add missing links following Path B article: {slug}"
    );
    let idempotency_key = format!("followup:path-b:{project_id}:{slug}:cluster_and_link");
    let spec = TaskSpec {
        project_id: project_id.to_string(),
        task_type: "cluster_and_link".to_string(),
        title: Some(title),
        description: Some(description),
        phase: Some("implementation".to_string()),
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        artifacts: vec![],
        depends_on: vec![],
        ..Default::default()
    };
    match TaskSpawner::spawn(conn, spec) {
        Ok(task) => {
            log::info!(
                "[write_package] spawned standalone cluster_and_link {} for slug {}",
                task.id,
                slug
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!(
                "[write_package] failed to spawn standalone cluster_and_link: {}",
                e
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{TaskArtifact, TaskStatus};
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
            return;
        }
        assert!(result.ok, "checks={:?}", result.checks);
        assert!(!result.follow_up_task_ids.is_empty());
        assert!(result.write_task_id.is_none());
    }
}
