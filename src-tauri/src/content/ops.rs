use rusqlite::Connection;
/// Sync utilities: reads MDX frontmatter, counts words, derives slugs.
///
/// Mirrors relevant parts of `packages/seo-content-cli/src/seo_content_mcp/seo_ops.py`
/// and the `pageseeds content sync-and-validate` CLI command.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Serialize;

use crate::error::Result;

/// Derived metadata parsed from a single MDX file's frontmatter + body.
#[derive(Debug, Clone, Serialize)]
pub struct FileMetadata {
    pub file_name: String,
    pub url_slug: String,
    pub title: Option<String>,
    pub published_date: Option<String>,
    pub status: Option<String>,
    pub word_count: usize,
}

/// Generate a url_slug from a filename.
///
/// Convention: `{id:03d}_{slug}.mdx` → `{slug}` (underscores preserved).
/// Delegates to `content::slug::strip_numeric_prefix`.
pub fn slug_from_filename(filename: &str) -> String {
    let basename = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    crate::content::slug::strip_numeric_prefix(basename)
}

/// Read a markdown file's frontmatter and count body words.
pub fn read_file_metadata(path: &Path) -> Result<FileMetadata> {
    let content = std::fs::read_to_string(path)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let url_slug = slug_from_filename(&file_name);

    let (title, published_date, status, body) =
        if let Some((fm, body)) = crate::content::frontmatter::split_mdx(&content) {
            let title = extract_value(fm, "title").map(String::from);
            let date = extract_value(fm, "date")
                .or_else(|| extract_value(fm, "publishedDate"))
                .or_else(|| extract_value(fm, "published_date"))
                .map(String::from);
            let status = extract_value(fm, "status").map(String::from);
            (title, date, status, body.to_string())
        } else {
            (None, None, None, content.clone())
        };

    let word_count = count_words(&body);

    Ok(FileMetadata {
        file_name,
        url_slug,
        title,
        published_date,
        status,
        word_count,
    })
}

/// Count words in markdown body (strips basic markdown syntax before counting).
pub fn count_words(text: &str) -> usize {
    // Strip front matter leftovers, headings, link syntax
    let re_md = Regex::new(r"[#*_`\[\]<>]|https?://\S+").unwrap();
    let stripped = re_md.replace_all(text, " ");
    stripped.split_whitespace().count()
}

fn extract_value<'a>(frontmatter: &'a str, key: &str) -> Option<&'a str> {
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        let prefix = format!("{key}:");
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

// ─── sync_and_validate ────────────────────────────────────────────────────────

/// Single issue found during sync/validation.
#[derive(Debug, Serialize)]
pub struct SyncIssue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub issue_type: String,
    pub detail: String,
}

/// Result returned by `sync_and_validate`.
#[derive(Debug, Serialize)]
pub struct SyncValidateResult {
    pub checked_entries: usize,
    pub content_files: usize,
    pub missing_files: Vec<SyncIssue>,
    pub orphan_files: Vec<String>,
    pub malformed_file_refs: Vec<SyncIssue>,
    pub duplicate_file_refs: Vec<SyncIssue>,
    pub date_mismatches: Vec<SyncIssue>,
    pub dates_synced: usize,
    pub fixable_mismatches: usize,
    pub next_action: String,
}

/// Compact health summary returned to the UI for startup checks.
#[derive(Debug, Serialize)]
pub struct ContentHealthResult {
    /// Number of articles.json entries checked.
    pub checked: usize,
    /// Number of content files found on disk.
    pub content_files: usize,
    /// Articles where frontmatter date ≠ articles.json date.
    pub date_mismatches: usize,
    /// Mismatches that can actually be patched (file has parseable frontmatter).
    pub fixable_mismatches: usize,
    /// Brief description of each mismatch (article title or id).
    pub mismatch_details: Vec<String>,
    /// MDX files on disk that have no matching entry in articles.json.
    pub orphan_files: Vec<String>,
    /// When true, `fix_date_mismatches` was called and patches were applied.
    pub fixed: bool,
    /// Number of dates successfully patched during the last fix run.
    pub dates_synced: usize,
}

/// Read-only health check: count date mismatches without writing anything.
pub fn content_health_check(
    automation_dir: &Path,
    repo_root: &Path,
    conn: &Connection,
    project_id: &str,
) -> std::result::Result<ContentHealthResult, String> {
    let result = sync_and_validate(automation_dir, repo_root, false, conn, project_id)?;
    let details = result
        .date_mismatches
        .iter()
        .map(|i| i.title.clone().unwrap_or_else(|| i.detail.clone()))
        .collect();
    Ok(ContentHealthResult {
        checked: result.checked_entries,
        content_files: result.content_files,
        date_mismatches: result.date_mismatches.len(),
        fixable_mismatches: result.fixable_mismatches,
        mismatch_details: details,
        orphan_files: result.orphan_files,
        fixed: false,
        dates_synced: result.dates_synced,
    })
}

/// Apply date fixes: patch frontmatter dates that differ from SQLite.
pub fn apply_date_fixes(
    automation_dir: &Path,
    repo_root: &Path,
    conn: &Connection,
    project_id: &str,
) -> std::result::Result<ContentHealthResult, String> {
    let result = sync_and_validate(automation_dir, repo_root, true, conn, project_id)?;
    Ok(ContentHealthResult {
        checked: result.checked_entries,
        content_files: result.content_files,
        date_mismatches: result.date_mismatches.len(),
        fixable_mismatches: result.fixable_mismatches,
        mismatch_details: vec![],
        orphan_files: result.orphan_files,
        fixed: true,
        dates_synced: result.dates_synced,
    })
}

/// Validate that SQLite article index and the content directory are in sync.
///
/// Mirrors `pageseeds content sync-and-validate --workspace-root <automation_dir> --website-path .`
///
/// When `apply_sync` is true, frontmatter dates that differ from SQLite
/// are patched in-place (same as `--apply-sync` in the CLI).
pub fn sync_and_validate(
    automation_dir: &Path,
    repo_root: &Path,
    apply_sync: bool,
    conn: &Connection,
    project_id: &str,
) -> std::result::Result<SyncValidateResult, String> {
    // 1. Load articles from SQLite (canonical runtime source of truth).
    let articles = crate::content::article_index::list_articles(conn, project_id)
        .map_err(|e| format!("Failed to load articles from SQLite: {}", e))?;

    // 2. Resolve content directory (mirrors Python CLI fallback chain).
    let content_dir = resolve_content_dir(automation_dir, repo_root)?;

    // 3. Collect content files: filename → absolute path.
    let content_files: HashMap<String, PathBuf> =
        crate::content::locator::collect_markdown_files(&content_dir)
            .into_iter()
            .filter_map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| (n.to_string(), p.clone()))
            })
            .collect();

    // 4. Cross-reference articles against content files.
    let mut seen: HashSet<String> = HashSet::new();
    let mut remaining: HashSet<String> = content_files.keys().cloned().collect();
    let mut missing_files = Vec::new();
    let mut malformed_file_refs = Vec::new();
    let mut duplicate_file_refs = Vec::new();
    let mut date_mismatches = Vec::new();
    let mut dates_synced = 0usize;
    let mut fixable_mismatches = 0usize;

    for article in &articles {
        let id = Some(article.id);
        let title = Some(article.title.clone());
        let file_ref = article.file.trim().to_string();
        let basename = Path::new(&file_ref)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if basename.is_empty() {
            malformed_file_refs.push(SyncIssue {
                id,
                title,
                file: if file_ref.is_empty() {
                    None
                } else {
                    Some(file_ref)
                },
                issue_type: "missing_file_field".into(),
                detail: "article has no file reference".into(),
            });
            continue;
        }

        if seen.contains(&basename) {
            duplicate_file_refs.push(SyncIssue {
                id,
                title,
                file: Some(basename.clone()),
                issue_type: "duplicate_reference".into(),
                detail: format!("'{}' referenced more than once", basename),
            });
            continue;
        }
        seen.insert(basename.clone());
        remaining.remove(&basename);

        let Some(file_path) = content_files.get(&basename) else {
            missing_files.push(SyncIssue {
                id,
                title,
                file: Some(file_ref),
                issue_type: "missing_file".into(),
                detail: format!("'{}' not found in content directory", basename),
            });
            continue;
        };

        // Date consistency check.
        let expected_date = article
            .published_date
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        if expected_date.is_empty() {
            continue;
        }
        let text = std::fs::read_to_string(file_path).unwrap_or_default();
        let current_date = extract_frontmatter_date(&text);
        if current_date.as_deref() == Some(expected_date.as_str()) {
            continue;
        }

        let is_fixable = crate::content::frontmatter::split_mdx(&text).is_some();
        if is_fixable {
            fixable_mismatches += 1;
        }

        date_mismatches.push(SyncIssue {
            id,
            title: title.clone(),
            file: Some(basename.clone()),
            issue_type: "date_mismatch".into(),
            detail: format!(
                "frontmatter='{}' articles.json='{}'",
                current_date.as_deref().unwrap_or("(none)"),
                expected_date,
            ),
        });

        if apply_sync {
            if let Ok(patched) = patch_frontmatter_date(&text, &expected_date) {
                if std::fs::write(file_path, patched).is_ok() {
                    dates_synced += 1;
                }
            }
        }
    }

    let mut orphan_files: Vec<String> = remaining.into_iter().collect();
    orphan_files.sort();

    let next_action = if !missing_files.is_empty() {
        "Fix missing content files referenced by articles.json"
    } else if !malformed_file_refs.is_empty() {
        "Fix malformed file references in articles.json"
    } else if !date_mismatches.is_empty() && !apply_sync {
        "Date mismatches found — re-run with apply_sync=true to patch frontmatter"
    } else if !orphan_files.is_empty() {
        "Review orphan content files not referenced in articles.json"
    } else {
        "Index and content are in sync"
    };

    Ok(SyncValidateResult {
        checked_entries: articles.len(),
        content_files: content_files.len(),
        missing_files,
        orphan_files,
        malformed_file_refs,
        duplicate_file_refs,
        date_mismatches,
        dates_synced,
        fixable_mismatches,
        next_action: next_action.into(),
    })
}

/// Clean stale entries from articles.json — remove articles whose files no longer exist.
/// The source of truth is the filesystem. Returns the list of removed article titles.
#[allow(dead_code)]
pub fn clean_stale_articles_json(
    automation_dir: &Path,
    repo_root: &Path,
) -> std::result::Result<Vec<String>, String> {
    let articles_path = automation_dir.join("articles.json");
    let json_str = std::fs::read_to_string(&articles_path).map_err(|e| {
        format!(
            "articles.json not found at {}: {}",
            articles_path.display(),
            e
        )
    })?;
    let mut doc: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse articles.json: {}", e))?;

    let articles = doc
        .get_mut("articles")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| "articles.json must contain an 'articles' array".to_string())?;

    let content_dir = resolve_content_dir(automation_dir, repo_root)?;

    // Collect all content files: filename → absolute path
    let content_files: std::collections::HashSet<String> =
        crate::content::locator::collect_markdown_files(&content_dir)
            .into_iter()
            .filter_map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_string())
            })
            .collect();

    let mut removed = Vec::new();
    articles.retain(|article| {
        let file_ref = article
            .get("file")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let basename = std::path::Path::new(file_ref)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if basename.is_empty() {
            return true; // Keep malformed entries — they'll be flagged elsewhere
        }

        if content_files.contains(basename) {
            true // File exists — keep
        } else {
            let title = article
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            removed.push(format!("{} ({})", title, file_ref));
            false // File missing — remove
        }
    });

    if !removed.is_empty() {
        let out = serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n";
        std::fs::write(&articles_path, out)
            .map_err(|e| format!("Failed to write cleaned articles.json: {}", e))?;
    }

    Ok(removed)
}

/// Resolve the content directory via the centralised `setup_check` module.
///
/// Priority:
/// 1. `content_dir` in `{automation_dir}/seo_workspace.json`
/// 2. Standard candidate auto-discovery from repo_root
pub fn resolve_content_dir(
    automation_dir: &Path,
    repo_root: &Path,
) -> std::result::Result<PathBuf, String> {
    use crate::engine::setup_check;

    // Load workspace config so setup_check can use it.
    let workspace_config = setup_check::load_workspace_config(automation_dir);

    let result = setup_check::resolve_content_dir(
        repo_root,
        automation_dir,
        workspace_config.as_ref(),
        None,
    );

    result.path.ok_or_else(|| {
        format!(
            "Content directory not found ({}). Add a seo_workspace.json with a content_dir field.",
            result.how
        )
    })
}

/// Extract the `date:` value from YAML frontmatter, stripping quotes.
fn extract_frontmatter_date(content: &str) -> Option<String> {
    let (fm, _) = crate::content::frontmatter::split_mdx(content)?;
    for line in fm.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("date:") {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Patch the `date:` field in YAML frontmatter.
/// Replaces an existing date line, or inserts one after the `title:` line.
fn patch_frontmatter_date(content: &str, new_date: &str) -> std::result::Result<String, String> {
    let Some((fm, body)) = crate::content::frontmatter::split_mdx(content) else {
        return Err("no frontmatter found".into());
    };

    let new_fm = if fm.lines().any(|l| l.trim().starts_with("date:")) {
        fm.lines()
            .map(|l| {
                if l.trim().starts_with("date:") {
                    format!("date: \"{}\"", new_date)
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else if let Some(pos) = fm.lines().position(|l| l.trim().starts_with("title:")) {
        let mut lines: Vec<String> = fm.lines().map(String::from).collect();
        lines.insert(pos + 1, format!("date: \"{}\"", new_date));
        lines.join("\n")
    } else {
        format!("{}\ndate: \"{}\"", fm, new_date)
    };

    Ok(format!("---\n{}\n---\n{}", new_fm, body))
}

// ─── IngestOrphanResult ───────────────────────────────────────────────────────

/// Result returned by orphan ingestion.
#[derive(Debug, Serialize)]
pub struct IngestOrphanResult {
    /// Number of files successfully ingested into articles.json + SQLite.
    pub ingested: usize,
    /// Basenames of newly added files.
    pub files: Vec<String>,
}

/// Re-scan all MDX files for a project and update the DB when frontmatter
/// metadata (title, published_date, status, word_count) differs from disk.
///
/// Returns the number of articles whose DB row was updated.
pub fn sync_article_metadata_from_disk(
    repo_root: &Path,
    project_id: &str,
    conn: &Connection,
) -> std::result::Result<usize, String> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| format!("Failed to list articles: {}", e))?;

    let content_dirs = crate::content::article_resolver::discover_content_dirs(repo_root);
    let content_dirs_refs: Vec<&str> = content_dirs.iter().map(|s| s.as_str()).collect();

    let mut updated = 0usize;

    for article in &articles {
        let resolved = crate::content::article_resolver::resolve_article_file(
            repo_root,
            &article.file,
            &content_dirs_refs,
        );
        if !resolved.found {
            log::warn!(
                "[sync_article_metadata] File not found for article {}: {}",
                article.id,
                article.file
            );
            continue;
        }

        let meta = match read_file_metadata(&resolved._absolute_path) {
            Ok(m) => m,
            Err(e) => {
                log::warn!(
                    "[sync_article_metadata] Failed to read {}: {}",
                    resolved._absolute_path.display(),
                    e
                );
                continue;
            }
        };

        let new_title = meta
            .title
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| article.title.clone());
        let new_published_date = meta.published_date;
        let new_status = meta
            .status
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| article.status.clone());
        let new_word_count = meta.word_count as i64;

        let changed = new_title != article.title
            || new_published_date != article.published_date
            || new_status != article.status
            || new_word_count != article.word_count;

        if changed {
            conn.execute(
                "UPDATE articles SET title = ?1, published_date = ?2, status = ?3, word_count = ?4 WHERE id = ?5 AND project_id = ?6",
                rusqlite::params![new_title, new_published_date, new_status, new_word_count, article.id, project_id],
            )
            .map_err(|e| format!("Failed to update article {}: {}", article.id, e))?;
            updated += 1;
        }
    }

    Ok(updated)
}

// ─── Article Analysis Helpers ───────────────────────────────────────────────

/// Load an article's raw content by slug.
///
/// Looks up the article in the database, resolves the content directory,
/// and reads the file from disk.
pub fn load_article_by_slug(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    content_dir_override: Option<&str>,
    slug: &str,
) -> Result<(crate::models::article::Article, String, PathBuf)> {
    use crate::engine::task_store;
    use crate::models::article::Article;

    let articles: Vec<Article> = task_store::list_articles(conn, project_id)?;
    let article = articles
        .into_iter()
        .find(|a| a.url_slug == slug)
        .ok_or_else(|| {
            crate::error::Error::Other(format!("Article with slug '{}' not found", slug))
        })?;

    let resolution = crate::content::locator::resolve(project_path, content_dir_override);
    let content_dir = resolution
        .selected
        .ok_or_else(|| crate::error::Error::Other("Content directory not found".to_string()))?;

    let file_path = content_dir.join(&article.file);
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| crate::error::Error::Other(format!("Failed to read article file: {}", e)))?;

    Ok((article, content, file_path))
}

/// Analyze readability for a single article identified by slug.
pub fn analyze_article_readability(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    content_dir_override: Option<&str>,
    slug: &str,
) -> Result<crate::content::readability::ReadabilityReport> {
    let (_article, content, _path) =
        load_article_by_slug(conn, project_id, project_path, content_dir_override, slug)?;
    let cleaned = crate::content::readability::clean_mdx_for_readability(&content);
    crate::content::readability::analyze_readability(&cleaned)
}

/// Analyze keyword density for a single article identified by slug.
pub fn analyze_keyword_density(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    content_dir_override: Option<&str>,
    slug: &str,
    target_keyword: &str,
) -> Result<crate::content::keyword_density::KeywordDensityReport> {
    let (_article, content, _path) =
        load_article_by_slug(conn, project_id, project_path, content_dir_override, slug)?;
    let (_, body) = crate::engine::exec::utils::parse_frontmatter(&content);
    Ok(crate::content::keyword_density::analyze_keyword_density(
        &body,
        target_keyword,
    ))
}

/// Build a CTR health summary for all articles in a project.
///
/// Delegated from `commands::content::get_ctr_health_summary` to keep business logic
/// out of the thin command layer.
pub fn build_ctr_health_summary(
    repo_root: &Path,
    articles: &[crate::models::article::Article],
    pending_fix_tasks: usize,
    completed_audits: usize,
    conn: &rusqlite::Connection,
    project_id: &str,
) -> crate::models::ctr::CtrHealthSummary {
    use crate::engine::exec::audit_health;
    use crate::models::ctr::{CtrHealthArticle, CtrHealthSummary};

    let mut health_articles = Vec::new();
    let mut missing_files = 0usize;
    let mut title_issues = 0usize;
    let mut meta_issues = 0usize;
    let mut snippet_issues = 0usize;
    let mut faq_issues = 0usize;
    let mut improved_count = 0usize;
    let mut regressed_count = 0usize;
    let mut already_healthy_count = 0usize;
    let mut latest_audit_at: Option<String> = None;

    for article in articles {
        let file_found = audit_health::resolve_content_file(repo_root, &article.file).is_some();

        // Load stored audit state for lifecycle tracking
        let stored_state =
            crate::db::get_article_audit_state(conn, project_id, &article.file, "ctr_audit")
                .ok()
                .flatten();

        let last_audited_at = stored_state.as_ref().map(|s| s.last_audited_at.clone());
        let last_audit_issues = stored_state
            .as_ref()
            .map(|s| s.issues_found.clone())
            .unwrap_or_default();

        // Track global last_audit_at
        if let Some(ref ts) = last_audited_at {
            if latest_audit_at.as_ref().map(|l| ts > l).unwrap_or(true) {
                latest_audit_at = Some(ts.clone());
            }
        }

        if !file_found {
            missing_files += 1;
            let resolved: Vec<String> = last_audit_issues
                .iter()
                .filter(|i| !(*i == "file_not_found"))
                .cloned()
                .collect();
            if !last_audit_issues.is_empty() && !resolved.is_empty() {
                improved_count += 1;
            }
            health_articles.push(CtrHealthArticle {
                id: article.id,
                title: article.title.clone(),
                url_slug: article.url_slug.clone(),
                file: article.file.clone(),
                healthy: false,
                audit_status: "needs_fix".to_string(),
                issues: vec!["file_not_found".to_string()],
                last_audited_at,
                last_audit_issues,
                resolved_issues: resolved,
            });
            continue;
        }

        let (title, meta, first_paragraph, _h1, has_faq, _found) =
            audit_health::read_article_excerpt(repo_root.to_str().unwrap_or(""), &article.file);

        let health = audit_health::check_article_health(
            &title,
            &meta,
            &first_paragraph,
            article.target_keyword.as_deref().unwrap_or(""),
            has_faq,
            true,
        );

        let mut issues = Vec::new();
        if !health.title_ok {
            issues.push("title_too_long".to_string());
            title_issues += 1;
        }
        if !health.meta_ok {
            issues.push("meta_too_short".to_string());
            meta_issues += 1;
        }
        if !health.snippet_ok {
            issues.push("snippet_suboptimal".to_string());
            snippet_issues += 1;
        }
        if !health.faq_ok {
            issues.push("missing_faq_schema".to_string());
            faq_issues += 1;
        }

        let healthy = issues.is_empty();
        let audit_status = if healthy {
            "healthy".to_string()
        } else {
            "needs_fix".to_string()
        };

        // Compute resolved issues: issues from last audit that are no longer present
        let resolved_issues: Vec<String> = last_audit_issues
            .iter()
            .filter(|i| !issues.contains(i))
            .cloned()
            .collect();

        // Compute improved / regressed / already_healthy
        let was_healthy = stored_state
            .as_ref()
            .map(|s| s.was_healthy)
            .unwrap_or(false);
        if was_healthy && healthy {
            already_healthy_count += 1;
        } else if !was_healthy && healthy {
            improved_count += 1;
        } else if was_healthy && !healthy {
            regressed_count += 1;
        } else {
            // Both unhealthy: compare issue sets
            let old_set: std::collections::HashSet<_> = last_audit_issues.iter().collect();
            let new_set: std::collections::HashSet<_> = issues.iter().collect();
            if new_set.is_subset(&old_set) && new_set.len() < old_set.len() {
                improved_count += 1;
            } else if !new_set.is_subset(&old_set) {
                regressed_count += 1;
            }
        }

        health_articles.push(CtrHealthArticle {
            id: article.id,
            title: article.title.clone(),
            url_slug: article.url_slug.clone(),
            file: article.file.clone(),
            healthy,
            audit_status,
            issues: issues.clone(),
            last_audited_at,
            last_audit_issues,
            resolved_issues,
        });
    }

    let total_articles = health_articles.len();
    let healthy_count = health_articles.iter().filter(|a| a.healthy).count();
    let unhealthy_count = total_articles - healthy_count;
    let open_issues_count = health_articles.iter().map(|a| a.issues.len()).sum();

    CtrHealthSummary {
        total_articles,
        healthy_count,
        unhealthy_count,
        improved_count,
        already_healthy_count,
        regressed_count,
        missing_files,
        title_issues,
        meta_issues,
        snippet_issues,
        faq_issues,
        last_audit_at: latest_audit_at,
        articles: health_articles,
        pending_fix_tasks,
        completed_audits,
        open_issues_count,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Regression tests for article-index persistence consolidation (Phase 0)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn setup_test_project(conn: &Connection, project_id: &str, project_path: &std::path::Path) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, ?2, ?3, 1, 'workspace')",
            [project_id, "Test Project", project_path.to_str().unwrap()],
        )
        .unwrap();
    }

    fn write_articles_json(automation_dir: &Path, json: &str) {
        std::fs::create_dir_all(automation_dir).unwrap();
        std::fs::write(automation_dir.join("articles.json"), json).unwrap();
    }

    fn write_seo_workspace_json(automation_dir: &Path, content_dir: &str) {
        std::fs::create_dir_all(automation_dir).unwrap();
        let config = format!(r#"{{"content_dir": "{}"}}"#, content_dir);
        std::fs::write(automation_dir.join("seo_workspace.json"), config).unwrap();
    }

    fn write_mdx(path: &Path, title: &str, date: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = format!(
            "---\ntitle: \"{}\"\ndate: \"{}\"\n---\n\nBody text.\n",
            title, date
        );
        std::fs::write(path, content).unwrap();
    }

    fn insert_article(conn: &Connection, project_id: &str, id: i64, file: &str, date: &str) {
        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                content_gaps_addressed, estimated_traffic_monthly, project_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                id,
                "Test Article",
                "test-article",
                file,
                Option::<String>::None,
                Option::<String>::None,
                0i64,
                date,
                100i64,
                "draft",
                "[]",
                Option::<String>::None,
                project_id,
            ],
        )
        .unwrap();
    }

    // ─── Regression: clean_stale_articles_json leaves SQLite stale ─────────────

    #[test]
    fn clean_stale_articles_json_removes_from_json_but_not_db() {
        let dir = unique_temp_dir("ps_stale_cleanup");
        let auto_dir = dir.join(".github").join("automation");
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        write_seo_workspace_json(&auto_dir, "content");
        write_articles_json(
            &auto_dir,
            r#"{"nextArticleId":2,"articles":[{"id":1,"title":"Old","file":"./content/001_old.mdx","status":"draft"}]}"#,
        );

        let conn = in_memory_db();
        setup_test_project(&conn, "p1", &dir);
        insert_article(&conn, "p1", 1, "./content/001_old.mdx", "2026-01-01");

        // No MDX file on disk → article is stale
        let removed = clean_stale_articles_json(&auto_dir, &dir).unwrap();
        assert_eq!(removed.len(), 1);

        // JSON should no longer contain the stale article
        let json_on_disk = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&json_on_disk).unwrap();
        assert!(doc["articles"].as_array().unwrap().is_empty());

        // SQLite SHOULD also have the article removed (correct behavior),
        // but currently it does NOT because clean_stale_articles_json only touches JSON.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM articles WHERE project_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // This assertion documents the bug: the DB row is still present.
        // When the bug is fixed, this test will need to be updated to expect 0.
        assert_eq!(count, 1, "BUG: clean_stale_articles_json removed the article from JSON but left the SQLite row intact");
    }

    // ─── Regression: sync_and_validate patches MDX from stale JSON ─────────────

// ═══════════════════════════════════════════════════════════════════════════════
// SSR Fallback / Orphaned Slug Detection
// ═══════════════════════════════════════════════════════════════════════════════

/// Detects MDX content files that have no matching route in the framework.
///
/// This catches SSR fallback bugs where a page exists as MDX but no route
/// renders it (would 404 or show a generic error page).
#[derive(Debug, Clone, Serialize)]
pub struct OrphanedSlug {
    pub slug: String,
    pub file_path: String,
    pub issue: String,
}

/// Scan the content directory and framework routes to find orphaned slugs.
pub fn detect_orphaned_slugs(
    content_dir: &Path,
    repo_root: &Path,
) -> Vec<OrphanedSlug> {
    let mdx_slugs = collect_mdx_slugs(content_dir);
    let route_patterns = collect_route_patterns(repo_root);

    mdx_slugs
        .into_iter()
        .filter(|(slug, _path)| !route_patterns.iter().any(|p| p.matches(slug)))
        .map(|(slug, path)| OrphanedSlug {
            slug,
            file_path: path.to_string_lossy().to_string(),
            issue: "No route renders this slug".to_string(),
        })
        .collect()
}

/// Collect all URL slugs from MDX files in the content directory.
fn collect_mdx_slugs(content_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut slugs = Vec::new();
    if !content_dir.is_dir() {
        return slugs;
    }

    let walker = walkdir::WalkDir::new(content_dir).max_depth(5).follow_links(false);
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mdx") {
            continue;
        }
        if let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) {
            let slug = slug_from_filename(file_name);
            if !slug.is_empty() {
                slugs.push((slug, path.to_path_buf()));
            }
        }
    }
    slugs
}

/// A simple route pattern extracted from framework files.
struct RoutePattern {
    /// e.g. "blog", "blog/slug", ""
    segments: Vec<String>,
    /// Whether the last segment is a catch-all parameter
    is_catch_all: bool,
}

impl RoutePattern {
    fn matches(&self, slug: &str) -> bool {
        let slug_parts: Vec<&str> = slug.split('/').filter(|s| !s.is_empty()).collect();

        // Empty pattern (root) matches nothing for our purposes
        if self.segments.is_empty() {
            return false;
        }

        // Simple exact match
        if !self.is_catch_all && self.segments.len() == slug_parts.len() {
            return self.segments.iter().enumerate().all(|(i, seg)| {
                if seg.starts_with('[') && seg.ends_with(']') {
                    // Parameter segment matches any non-empty value
                    !slug_parts[i].is_empty()
                } else {
                    seg == slug_parts[i]
                }
            });
        }

        // Catch-all match: e.g. app/blog/[...slug]/page.tsx matches blog/anything
        if self.is_catch_all {
            let fixed_len = self.segments.len() - 1;
            if slug_parts.len() >= fixed_len {
                return self.segments.iter().take(fixed_len).enumerate().all(|(i, seg)| {
                    if seg.starts_with('[') && seg.ends_with(']') {
                        !slug_parts[i].is_empty()
                    } else {
                        seg == slug_parts[i]
                    }
                });
            }
        }

        false
    }
}

/// Collect route patterns from common framework directories.
fn collect_route_patterns(repo_root: &Path) -> Vec<RoutePattern> {
    let mut patterns = Vec::new();

    // Framework route directories
    let route_dirs: Vec<(PathBuf, bool)> = vec![
        (repo_root.join("app"), true),      // Next.js 13+ app router
        (repo_root.join("pages"), true),    // Next.js pages / Nuxt / Astro
        (repo_root.join("src").join("app"), true),
        (repo_root.join("src").join("pages"), true),
        (repo_root.join("src").join("routes"), true), // SvelteKit
    ];

    for (dir, recurse) in route_dirs {
        if !dir.is_dir() {
            continue;
        }
        let max_depth = if recurse { 5 } else { 1 };
        let walker = walkdir::WalkDir::new(&dir).max_depth(max_depth).follow_links(false);

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // Next.js app router: page.tsx, layout.tsx, route.tsx
            // Next.js pages: [slug].tsx, index.tsx
            // SvelteKit: +page.svelte
            let is_route_file = matches!(
                file_name,
                "page.tsx" | "page.jsx" | "page.js" | "page.astro" | "page.vue"
                    | "+page.svelte" | "+page.ts" | "+page.js"
                    | "index.tsx" | "index.jsx" | "index.js" | "index.astro" | "index.vue"
                    | "[slug].tsx" | "[slug].jsx" | "[slug].js"
                    | "[id].tsx" | "[id].jsx" | "[id].js"
                    | "[...slug].tsx" | "[...slug].jsx" | "[...slug].js"
            );

            if !is_route_file {
                continue;
            }

            // Extract route segments from the directory path
            if let Ok(relative) = path.strip_prefix(&dir) {
                let parent = relative.parent().unwrap_or(Path::new(""));
                let mut segments: Vec<String> = parent
                    .components()
                    .filter_map(|c| {
                        if let Some(s) = c.as_os_str().to_str() {
                            if s != "." { Some(s.to_string()) } else { None }
                        } else {
                            None
                        }
                    })
                    .collect();

                // Handle [...slug] catch-all segments
                let is_catch_all = parent
                    .components()
                    .any(|c| {
                        c.as_os_str()
                            .to_str()
                            .map(|s| s.starts_with("[..."))
                            .unwrap_or(false)
                    });

                // Remove parameter brackets from segments for matching
                segments = segments
                    .into_iter()
                    .map(|s| {
                        if s.starts_with('[') && s.ends_with(']') {
                            s // keep as parameter marker
                        } else {
                            s
                        }
                    })
                    .collect();

                if !segments.is_empty() {
                    patterns.push(RoutePattern {
                        segments,
                        is_catch_all,
                    });
                }
            }
        }
    }

    // Also look for explicit route config files
    let config_files = [
        repo_root.join("next.config.js"),
        repo_root.join("next.config.ts"),
        repo_root.join("next.config.mjs"),
        repo_root.join("astro.config.mjs"),
        repo_root.join("astro.config.ts"),
    ];
    for config in &config_files {
        if config.exists() {
            // Presence of config implies the framework is set up; we already
            // scanned the route dirs, so this is just a signal that routes exist.
        }
    }

    // Deduplicate patterns
    patterns.sort_by(|a, b| {
        let a_key = format!("{}|{}", a.segments.join("/"), a.is_catch_all);
        let b_key = format!("{}|{}", b.segments.join("/"), b.is_catch_all);
        a_key.cmp(&b_key)
    });
    patterns.dedup_by(|a, b| {
        a.segments == b.segments && a.is_catch_all == b.is_catch_all
    });

    patterns
}

    #[test]
    fn sync_and_validate_patches_mdx_from_json_not_sqlite() {
        let dir = unique_temp_dir("ps_sync_date");
        let auto_dir = dir.join(".github").join("automation");
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        write_seo_workspace_json(&auto_dir, "content");
        write_articles_json(
            &auto_dir,
            r#"{"nextArticleId":2,"articles":[{"id":1,"title":"Test","file":"./content/001_test.mdx","published_date":"2026-01-01","status":"draft"}]}"#,
        );

        let mdx_path = content_dir.join("001_test.mdx");
        write_mdx(&mdx_path, "Test", "2026-03-01");

        let conn = in_memory_db();
        setup_test_project(&conn, "p1", &dir);
        // SQLite has a NEWER date than articles.json (e.g. publish just updated it)
        insert_article(&conn, "p1", 1, "./content/001_test.mdx", "2026-04-28");

        // Apply sync should patch MDX with the authoritative date.
        // SQLite is the intended runtime source of truth, so MDX should become 2026-04-28.
        let result = sync_and_validate(&auto_dir, &dir, true, &conn, "p1").unwrap();
        assert_eq!(result.dates_synced, 1);

        let mdx_content = std::fs::read_to_string(&mdx_path).unwrap();
        // This assertion documents the bug: sync_and_validate reads articles.json (2026-01-01)
        // instead of SQLite (2026-04-28), so MDX gets the stale JSON date.
        // When the bug is fixed, MDX should contain 2026-04-28 and this test passes.
        assert!(
            mdx_content.contains("2026-04-28"),
            "sync_and_validate should patch MDX with the SQLite date (2026-04-28), not the stale JSON date. MDX content: {}",
            mdx_content
        );
    }
}
