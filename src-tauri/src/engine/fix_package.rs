//! CLI Path B fix package + submit (issue #137).
//!
//! Deterministic package for session agents to edit full MDX (not nested 3k
//! excerpt generate). Submit applies optional structured patches or accepts an
//! already-edited file, then validates structure — no nested LLM generate.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::content::validate_article::{
    validate_article_content, ArticleCheck, ValidateArticleInput, META_MAX_LEN, META_MIN_LEN,
};
use crate::engine::exec::audit_health::{resolve_content_file, TITLE_MAX_LEN};
use crate::engine::exec::content::materialize_content_fix_changes;
use crate::engine::exec::ctr_audit::{
    materialize_ctr_fix_changes, normalize_patch_fields, prune_invalid_change_fields,
    validate_patch_fields,
};
use crate::engine::site_state::{
    get_article_package, QueryMetric, BODY_SIZE_CAP, BODY_TRUNCATION_NOTE, DEFAULT_PERIOD_DAYS,
};
use crate::engine::skills;
use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::content_review::ContentFixPatch;
use crate::models::ctr::CtrFixPatch;

// ─── Types ───────────────────────────────────────────────────────────────────

/// Fix pipeline kind (content SERP health vs CTR).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixKind {
    Content,
    Ctr,
}

impl FixKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "content" => Ok(Self::Content),
            "ctr" => Ok(Self::Ctr),
            other => Err(Error::Validation(format!(
                "invalid fix kind '{other}' (expected content|ctr)"
            ))),
        }
    }

    pub fn skill_name(self) -> &'static str {
        match self {
            Self::Content => "content-fix-apply",
            Self::Ctr => "ctr-fix-apply",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Ctr => "ctr",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSerpFields {
    pub title: String,
    pub title_len: usize,
    pub meta_description: Option<String>,
    pub meta_len: usize,
    pub h1: Option<String>,
    pub has_faq: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSkillRef {
    pub name: String,
    /// Full SKILL.md body when loadable (session agents get craft rules without a second hop).
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixConstraints {
    pub title_max_len: usize,
    pub meta_min_len: usize,
    pub meta_max_len: usize,
    /// Allowed structured patch fields for this kind.
    pub allowed_change_fields: Vec<String>,
    /// Nested `execute-task fix_*` generate is not the Path B happy path.
    pub ban_nested_generate: bool,
    /// Prefer editing the full file at `file_absolute` over body excerpts.
    pub prefer_full_file: bool,
    /// Word-count floors are not hard-failed on submit for partial SERP fixes.
    pub hard_fail_min_word_count: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixPackage {
    pub kind: FixKind,
    pub article_id: i64,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goals: Option<String>,
    /// Repo-relative MDX path.
    pub file: String,
    /// Absolute path the session agent should edit.
    pub file_absolute: String,
    pub serp: FixSerpFields,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_keyword: Option<String>,
    pub queries: Vec<QueryMetric>,
    /// Body when under the desk size cap (much larger than nested generate's 3k).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    pub body_included: bool,
    pub body_truncated: bool,
    pub body_size_cap: usize,
    pub available_link_slugs: Vec<String>,
    pub skill: FixSkillRef,
    pub constraints: FixConstraints,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSubmitResult {
    pub ok: bool,
    pub kind: FixKind,
    pub slug: String,
    pub file: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applied: Vec<String>,
    pub validation: FixValidationReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixValidationReport {
    pub ok: bool,
    pub checks: Vec<ArticleCheck>,
}

/// Options for [`submit_fix`].
#[derive(Debug, Clone, Default)]
pub struct FixSubmitOpts {
    /// Optional absolute or repo-relative MDX path override.
    pub file_override: Option<String>,
    /// Optional raw JSON for ContentFixPatch or CtrFixPatch (by kind).
    pub patch_json: Option<String>,
}

// ─── Build package ───────────────────────────────────────────────────────────

/// Build a Path B fix package for one slug. No LLM.
pub fn build_fix_package(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    slug: &str,
    kind: FixKind,
    goals: Option<&str>,
    period_days: Option<i64>,
) -> Result<FixPackage> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err(Error::Validation("slug is required".into()));
    }
    if project_path.trim().is_empty() {
        return Err(Error::Validation("project_path is required".into()));
    }

    let period = period_days.unwrap_or(DEFAULT_PERIOD_DAYS);
    let article = get_article_package(conn, project_id, project_path, slug, Some(period))?;

    let file = article.content.file.clone();
    if file.is_empty() {
        return Err(Error::Validation(format!(
            "Article '{slug}' has no file path in the database"
        )));
    }

    let repo = Path::new(project_path);
    let file_absolute = resolve_content_file(repo, &file)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| repo.join(&file).to_string_lossy().to_string());

    let body_raw = article.content.body_markdown;
    let body_truncated = body_raw.contains(BODY_TRUNCATION_NOTE.trim())
        || body_raw.chars().count() >= BODY_SIZE_CAP;
    let body_included = !body_raw.is_empty();
    let body_markdown = if body_included {
        Some(body_raw)
    } else {
        None
    };

    let skill_name = kind.skill_name();
    let skill = match skills::load_skill(repo, skill_name) {
        Some(s) => FixSkillRef {
            name: s.name,
            content: s.content,
        },
        None => FixSkillRef {
            name: skill_name.to_string(),
            content: String::new(),
        },
    };

    let available_link_slugs = task_store::load_valid_link_targets(conn, project_id, project_path)
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();

    let goals = goals
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(|g| g.to_string());

    let constraints = constraints_for(kind);
    let instructions = path_b_instructions(kind, &file_absolute, goals.as_deref());

    Ok(FixPackage {
        kind,
        article_id: article.article_id,
        slug: article.slug,
        goals,
        file,
        file_absolute,
        serp: FixSerpFields {
            title: article.catalog.serp.title,
            title_len: article.catalog.serp.title_len,
            meta_description: article.catalog.serp.meta_description,
            meta_len: article.catalog.serp.meta_len,
            h1: article.catalog.h1,
            has_faq: article.catalog.serp.has_faq,
        },
        target_keyword: article.catalog.target_keyword,
        queries: article.queries,
        body_markdown,
        body_included,
        body_truncated,
        body_size_cap: BODY_SIZE_CAP,
        available_link_slugs,
        skill,
        constraints,
        instructions,
    })
}

fn constraints_for(kind: FixKind) -> FixConstraints {
    let allowed_change_fields = match kind {
        FixKind::Content => vec![
            "title".into(),
            "h1".into(),
            "description".into(),
            "intro".into(),
            "internal_links".into(),
            "faq_questions".into(),
            "eeat_signal".into(),
            "cta".into(),
        ],
        FixKind::Ctr => vec![
            "title".into(),
            "description".into(),
            "first_paragraph".into(),
            "faq_questions".into(),
            "snippet_patch".into(),
        ],
    };
    FixConstraints {
        title_max_len: TITLE_MAX_LEN,
        meta_min_len: META_MIN_LEN,
        meta_max_len: META_MAX_LEN,
        allowed_change_fields,
        ban_nested_generate: true,
        prefer_full_file: true,
        hard_fail_min_word_count: false,
    }
}

fn path_b_instructions(kind: FixKind, file_absolute: &str, goals: Option<&str>) -> String {
    let goals_line = goals
        .map(|g| format!("Goals: {g}\n"))
        .unwrap_or_default();
    format!(
        "Path B ({kind}): edit the full MDX at `{file}` (prefer full file over excerpts). \
         Apply craft rules from the skill in this package. \
         Then run fix-submit with the same slug/kind — no nested execute-task fix_* generate.\n\
         {goals_line}",
        kind = kind.as_str(),
        file = file_absolute,
        goals_line = goals_line,
    )
}

// ─── Submit ──────────────────────────────────────────────────────────────────

/// Apply optional patch and/or validate the on-disk (or override) MDX. No LLM.
pub fn submit_fix(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    slug: &str,
    kind: FixKind,
    opts: FixSubmitOpts,
) -> Result<FixSubmitResult> {
    let package = build_fix_package(
        conn,
        project_id,
        project_path,
        slug,
        kind,
        None,
        None,
    )?;

    let repo = Path::new(project_path);
    let mut applied = Vec::new();

    let target_rel = if let Some(ref override_path) = opts.file_override {
        override_path.clone()
    } else {
        package.file.clone()
    };

    let file_path = resolve_file(repo, &target_rel).ok_or_else(|| {
        Error::Validation(format!(
            "File not found: {target_rel}. Run sanitize_content or pass --file."
        ))
    })?;

    // Optional structured patch apply (deterministic, no generate).
    if let Some(ref patch_json) = opts.patch_json {
        let patch_applied = match kind {
            FixKind::Content => apply_content_patch(
                &file_path,
                package.article_id,
                &package.file,
                patch_json,
            )?,
            FixKind::Ctr => apply_ctr_patch(
                &file_path,
                package.article_id,
                &package.file,
                package.target_keyword.as_deref(),
                patch_json,
            )?,
        };
        applied.extend(patch_applied);
    }

    let content = std::fs::read_to_string(&file_path).map_err(|e| {
        Error::Other(format!("Failed to read {}: {e}", file_path.display()))
    })?;

    let validation = validate_fix_content(&package.slug, &content, package.target_keyword.as_deref());

    if validation.ok {
        touch_last_edited(conn, project_id, package.article_id);

        // Path B CTR ships record a sparse change event (issue #152). Best-effort:
        // ship success must not fail if measurement row write fails.
        if kind == FixKind::Ctr {
            let fix_task_id = crate::engine::post_actions::path_b_ctr_fix_task_id(
                project_id,
                package.article_id,
                &package.slug,
            );
            if let Err(e) = crate::engine::post_actions::record_ctr_change_event(
                conn,
                project_id,
                package.article_id,
                &fix_task_id,
                None,
                Some(package.slug.as_str()),
            ) {
                log::warn!(
                    "[fix_package] Failed to record CTR change event for article {} ({}): {}",
                    package.article_id,
                    package.slug,
                    e
                );
            }
        }
    }

    let message = if validation.ok {
        if applied.is_empty() {
            Some(format!(
                "Validated {} (kind={}) without nested generate",
                package.file,
                kind.as_str()
            ))
        } else {
            Some(format!(
                "Applied {} and validated {} (kind={})",
                applied.join(", "),
                package.file,
                kind.as_str()
            ))
        }
    } else {
        Some(format!(
            "Validation failed for {} — fix MDX and resubmit",
            package.file
        ))
    };

    Ok(FixSubmitResult {
        ok: validation.ok,
        kind,
        slug: package.slug,
        file: package.file,
        applied,
        validation,
        message,
    })
}

fn resolve_file(repo: &Path, file_ref: &str) -> Option<PathBuf> {
    resolve_content_file(repo, file_ref).or_else(|| {
        let p = Path::new(file_ref);
        if p.is_absolute() && p.exists() {
            Some(p.to_path_buf())
        } else {
            let joined = repo.join(p);
            joined.exists().then_some(joined)
        }
    })
}

/// Fix-path validation: always gate MDX structure + H1 + title; do **not** hard-fail
/// min_word_count (partial SERP fixes on short legacy posts).
fn validate_fix_content(
    slug: &str,
    content: &str,
    target_keyword: Option<&str>,
) -> FixValidationReport {
    let input = ValidateArticleInput {
        target_keyword: target_keyword.map(|s| s.to_string()),
        valid_link_targets: None,
        // Soft: do not fail partial SERP fixes on pre-existing short bodies.
        min_word_count: Some(0),
    };
    let full = validate_article_content(slug, content, &input);

    // Keep structural + identity checks; drop length floors and keyword presence
    // that would block intentional partial SERP fixes (mirrors fix_verify residual policy).
    let keep = |id: &str| matches!(id, "mdx_structure" | "has_h1" | "frontmatter_title");
    let checks: Vec<ArticleCheck> = full
        .checks
        .into_iter()
        .filter(|c| keep(&c.id))
        .collect();
    let ok = checks.iter().all(|c| c.pass);

    FixValidationReport { ok, checks }
}

fn touch_last_edited(conn: &Connection, project_id: &str, article_id: i64) {
    if article_id == 0 {
        return;
    }
    let now = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "UPDATE articles SET last_edited_at = ?1 WHERE id = ?2 AND project_id = ?3",
        rusqlite::params![&now, article_id, project_id],
    );
}

// ─── Deterministic patch apply via shared materializers (no Task / no LLM) ───

fn apply_content_patch(
    file_path: &Path,
    article_id: i64,
    package_file: &str,
    patch_json: &str,
) -> Result<Vec<String>> {
    let mut patch: ContentFixPatch = serde_json::from_str(patch_json)
        .map_err(|e| Error::Validation(format!("invalid ContentFixPatch JSON: {e}")))?;

    if let Some(err) = patch.error.as_ref() {
        return Err(Error::Validation(format!("patch reported error: {err}")));
    }

    // Pin identity from package — never trust hallucinated paths.
    if !package_file.is_empty() {
        patch.file = package_file.to_string();
    }
    if article_id != 0 {
        patch.article_id = article_id;
    }

    let original = std::fs::read_to_string(file_path)
        .map_err(|e| Error::Other(format!("read {}: {e}", file_path.display())))?;

    let (new_content, applied) = materialize_content_fix_changes(&original, &patch.changes)
        .map_err(Error::Validation)?;

    if applied.is_empty() {
        return Ok(applied);
    }

    std::fs::write(file_path, &new_content).map_err(|e| {
        Error::Other(format!("Failed to write {}: {e}", file_path.display()))
    })?;
    Ok(applied)
}

fn apply_ctr_patch(
    file_path: &Path,
    article_id: i64,
    package_file: &str,
    target_keyword: Option<&str>,
    patch_json: &str,
) -> Result<Vec<String>> {
    let mut patch: CtrFixPatch = serde_json::from_str(patch_json)
        .map_err(|e| Error::Validation(format!("invalid CtrFixPatch JSON: {e}")))?;

    if let Some(err) = patch.error.as_ref() {
        return Err(Error::Validation(format!("patch reported error: {err}")));
    }

    if !package_file.is_empty() {
        patch.file = package_file.to_string();
    }
    if article_id != 0 {
        patch.article_id = article_id;
    }

    let original = std::fs::read_to_string(file_path)
        .map_err(|e| Error::Other(format!("read {}: {e}", file_path.display())))?;

    // Same safety boundary as the queue/task CTR apply path.
    let mut repair_notes = normalize_patch_fields(&mut patch, target_keyword);
    let mut validation_errors = validate_patch_fields(&patch, target_keyword, &original);
    if !validation_errors.is_empty() {
        let pruned = prune_invalid_change_fields(&mut patch, &validation_errors);
        if !pruned.is_empty() {
            repair_notes.extend(pruned);
            validation_errors = validate_patch_fields(&patch, target_keyword, &original);
        }
    }
    if !validation_errors.is_empty() {
        return Err(Error::Validation(format!(
            "invalid CtrFixPatch values: {}. No changes written.",
            validation_errors.join("; ")
        )));
    }

    let (new_content, applied) = materialize_ctr_fix_changes(&original, &patch.changes)
        .map_err(Error::Validation)?;

    if applied.is_empty() {
        return Ok(applied);
    }

    std::fs::write(file_path, &new_content).map_err(|e| {
        Error::Other(format!("Failed to write {}: {e}", file_path.display()))
    })?;

    if !repair_notes.is_empty() {
        log::info!(
            "[fix_package] CTR patch normalized for {}: {}",
            package_file,
            repair_notes.join("; ")
        );
    }
    Ok(applied)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn temp_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-fix-package-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(dir.join(".github/automation")).unwrap();
        fs::create_dir_all(dir.join("content")).unwrap();
        dir
    }

    fn insert_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, 'Test', ?2, 1, 'workspace')",
            rusqlite::params![id, path],
        )
        .unwrap();
    }

    fn insert_article(
        conn: &Connection,
        project_id: &str,
        id: i64,
        slug: &str,
        title: &str,
        file: &str,
    ) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count, content_hash
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'published', 'cold brew', '[]', 0, 200, 0, 'hash')",
            rusqlite::params![id, project_id, title, slug, file],
        )
        .unwrap();
    }

    fn write_mdx(project: &Path, rel: &str, body: &str) {
        let path = project.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    fn valid_mdx(title: &str, h1: &str, body_extra: &str) -> String {
        format!(
            r#"---
title: "{title}"
description: "A solid meta description that is long enough for SEO structural checks and stays under one fifty five."
date: "2026-01-15"
---

# {h1}

Intro paragraph about cold brew makers for home use.

{body_extra}
"#
        )
    }

    #[test]
    fn fix_kind_parse() {
        assert_eq!(FixKind::parse("content").unwrap(), FixKind::Content);
        assert_eq!(FixKind::parse("CTR").unwrap(), FixKind::Ctr);
        assert!(FixKind::parse("other").is_err());
    }

    #[test]
    fn build_content_package_includes_path_skill_and_body() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);
        insert_article(
            &conn,
            "proj1",
            1,
            "cold-brew-guide",
            "Cold Brew Guide",
            "content/cold.mdx",
        );

        // Body larger than nested generate's 3k so package proves the upgrade.
        let long_body = "word ".repeat(900);
        write_mdx(
            &project,
            "content/cold.mdx",
            &valid_mdx("Cold Brew Guide", "Cold Brew Guide", &long_body),
        );

        let pkg = build_fix_package(
            &conn,
            "proj1",
            &project_path,
            "cold-brew-guide",
            FixKind::Content,
            Some("Improve title CTR without spam"),
            None,
        )
        .unwrap();

        assert_eq!(pkg.kind, FixKind::Content);
        assert_eq!(pkg.slug, "cold-brew-guide");
        assert_eq!(pkg.file, "content/cold.mdx");
        assert!(pkg.file_absolute.contains("cold.mdx"));
        assert_eq!(pkg.skill.name, "content-fix-apply");
        assert!(!pkg.skill.content.is_empty(), "embedded skill should load");
        assert!(pkg.body_included);
        let body = pkg.body_markdown.as_ref().unwrap();
        // Nested generate used 3k; package should carry far more when present.
        assert!(
            body.chars().count() > 3_000,
            "body should exceed nested 3k excerpt (got {})",
            body.chars().count()
        );
        assert_eq!(pkg.body_size_cap, BODY_SIZE_CAP);
        assert!(pkg.constraints.ban_nested_generate);
        assert!(pkg.constraints.prefer_full_file);
        assert_eq!(
            pkg.goals.as_deref(),
            Some("Improve title CTR without spam")
        );
        assert!(!pkg.serp.title.is_empty());
        assert!(pkg.instructions.contains("Path B"));
    }

    #[test]
    fn build_ctr_package_uses_ctr_skill() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);
        insert_article(
            &conn,
            "proj1",
            2,
            "espresso-101",
            "Espresso 101",
            "content/esp.mdx",
        );
        write_mdx(
            &project,
            "content/esp.mdx",
            &valid_mdx("Espresso 101", "Espresso 101", "More words here."),
        );

        let pkg = build_fix_package(
            &conn,
            "proj1",
            &project_path,
            "espresso-101",
            FixKind::Ctr,
            None,
            None,
        )
        .unwrap();

        assert_eq!(pkg.kind, FixKind::Ctr);
        assert_eq!(pkg.skill.name, "ctr-fix-apply");
        assert!(!pkg.skill.content.is_empty());
        assert!(pkg
            .constraints
            .allowed_change_fields
            .iter()
            .any(|f| f == "snippet_patch"));
    }

    #[test]
    fn build_package_missing_slug_errors() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);

        let err = build_fix_package(
            &conn,
            "proj1",
            &project_path,
            "",
            FixKind::Content,
            None,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().to_lowercase().contains("slug"));
    }

    #[test]
    fn submit_invalid_mdx_fails_validation() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);
        insert_article(
            &conn,
            "proj1",
            1,
            "broken",
            "Broken",
            "content/broken.mdx",
        );
        // Missing closing frontmatter fence → invalid structure
        write_mdx(
            &project,
            "content/broken.mdx",
            "---\ntitle: Broken\n# no close\n",
        );

        let result = submit_fix(
            &conn,
            "proj1",
            &project_path,
            "broken",
            FixKind::Content,
            FixSubmitOpts::default(),
        )
        .unwrap();

        assert!(!result.ok);
        assert!(
            result
                .validation
                .checks
                .iter()
                .any(|c| c.id == "mdx_structure" && !c.pass)
                || result.validation.checks.iter().any(|c| !c.pass)
        );
    }

    #[test]
    fn submit_valid_mdx_ok() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);
        insert_article(
            &conn,
            "proj1",
            1,
            "good-post",
            "Good Post",
            "content/good.mdx",
        );
        write_mdx(
            &project,
            "content/good.mdx",
            &valid_mdx("Good Post", "Good Post", "Extra body for cold brew fans."),
        );

        let result = submit_fix(
            &conn,
            "proj1",
            &project_path,
            "good-post",
            FixKind::Content,
            FixSubmitOpts::default(),
        )
        .unwrap();

        assert!(result.ok, "{:?}", result.validation);
        assert!(result.applied.is_empty());
    }

    #[test]
    fn submit_content_patch_updates_title() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);
        insert_article(
            &conn,
            "proj1",
            1,
            "patch-me",
            "Old Title",
            "content/patch.mdx",
        );
        write_mdx(
            &project,
            "content/patch.mdx",
            &valid_mdx("Old Title", "Old Title", "Body text about cold brew makers."),
        );

        let patch = r#"{
            "article_id": 1,
            "file": "content/patch.mdx",
            "changes": {
                "title": "New SERP Title for Cold Brew"
            }
        }"#;

        let result = submit_fix(
            &conn,
            "proj1",
            &project_path,
            "patch-me",
            FixKind::Content,
            FixSubmitOpts {
                patch_json: Some(patch.to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(result.ok, "{:?}", result);
        assert!(result.applied.iter().any(|a| a == "title"));
        let written = fs::read_to_string(project.join("content/patch.mdx")).unwrap();
        assert!(written.contains("New SERP Title for Cold Brew"));
        assert!(!written.contains("title: \"Old Title\"") && !written.contains("title: Old Title"));
    }

    #[test]
    fn submit_does_not_hard_fail_short_body_word_count() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);
        insert_article(
            &conn,
            "proj1",
            1,
            "shorty",
            "Shorty",
            "content/short.mdx",
        );
        // Well under 800 words — write path would fail; fix path must not.
        write_mdx(
            &project,
            "content/short.mdx",
            &valid_mdx("Shorty", "Shorty", "Tiny body."),
        );

        let result = submit_fix(
            &conn,
            "proj1",
            &project_path,
            "shorty",
            FixKind::Content,
            FixSubmitOpts::default(),
        )
        .unwrap();

        assert!(
            result.ok,
            "short body should still submit ok on fix path: {:?}",
            result.validation
        );
    }

    /// Issue #152: successful Path B `fix-submit` with `kind=ctr` records a change event.
    #[test]
    fn submit_ctr_records_change_event() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);
        insert_article(
            &conn,
            "proj1",
            7,
            "ctr-path-b",
            "CTR Path B",
            "content/ctr-path-b.mdx",
        );
        write_mdx(
            &project,
            "content/ctr-path-b.mdx",
            &valid_mdx(
                "CTR Path B Title",
                "CTR Path B Title",
                "Body about SERP snippets.",
            ),
        );

        let result = submit_fix(
            &conn,
            "proj1",
            &project_path,
            "ctr-path-b",
            FixKind::Ctr,
            FixSubmitOpts::default(),
        )
        .unwrap();

        assert!(result.ok, "{:?}", result.validation);
        let outcomes = crate::db::list_ctr_outcomes(&conn, "proj1").unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].article_id, 7);
        assert!(
            outcomes[0].fix_task_id.starts_with("path_b_fix:"),
            "synthetic Path B fix id: {}",
            outcomes[0].fix_task_id
        );
        assert_eq!(outcomes[0].outcome_status, "pending");
        assert!(outcomes[0].deployed_at.is_none());
    }

    #[test]
    fn submit_content_does_not_record_ctr_outcome() {
        let conn = in_memory_db();
        let project = temp_project();
        let project_path = project.to_string_lossy().to_string();
        insert_project(&conn, "proj1", &project_path);
        insert_article(
            &conn,
            "proj1",
            1,
            "content-only",
            "Content Only",
            "content/c.mdx",
        );
        write_mdx(
            &project,
            "content/c.mdx",
            &valid_mdx("Content Only", "Content Only", "Body."),
        );

        let result = submit_fix(
            &conn,
            "proj1",
            &project_path,
            "content-only",
            FixKind::Content,
            FixSubmitOpts::default(),
        )
        .unwrap();
        assert!(result.ok);
        assert!(crate::db::list_ctr_outcomes(&conn, "proj1")
            .unwrap()
            .is_empty());
    }
}
