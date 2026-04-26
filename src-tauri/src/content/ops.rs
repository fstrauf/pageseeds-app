/// Sync utilities: reads MDX frontmatter, counts words, derives slugs.
///
/// Mirrors relevant parts of `packages/seo-content-cli/src/seo_content_mcp/seo_ops.py`
/// and the `pageseeds content sync-and-validate` CLI command.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use rusqlite::Connection;

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
pub fn slug_from_filename(filename: &str) -> String {
    let basename = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    // Strip leading numeric prefix: "042_my_article"  → "my_article"
    let re = Regex::new(r"^\d+_").unwrap();
    re.replace(basename, "").into_owned()
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
        if let Some((fm, body)) = crate::content::cleaner::parse_frontmatter(&content) {
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
fn count_words(text: &str) -> usize {
    // Strip front matter leftovers, headings, link syntax
    let re_md = Regex::new(r"[#*_`\[\]<>]|https?://\S+").unwrap();
    let stripped = re_md.replace_all(text, " ");
    stripped
        .split_whitespace()
        .count()
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
    /// Brief description of each mismatch (article title or id).
    pub mismatch_details: Vec<String>,
    /// MDX files on disk that have no matching entry in articles.json.
    pub orphan_files: Vec<String>,
    /// When true, `fix_date_mismatches` was called and patches were applied.
    pub fixed: bool,
}

/// Read-only health check: count date mismatches without writing anything.
pub fn content_health_check(
    automation_dir: &Path,
    repo_root: &Path,
) -> std::result::Result<ContentHealthResult, String> {
    let result = sync_and_validate(automation_dir, repo_root, false)?;
    let details = result
        .date_mismatches
        .iter()
        .map(|i| i.title.clone().unwrap_or_else(|| i.detail.clone()))
        .collect();
    Ok(ContentHealthResult {
        checked: result.checked_entries,
        content_files: result.content_files,
        date_mismatches: result.date_mismatches.len(),
        mismatch_details: details,
        orphan_files: result.orphan_files,
        fixed: false,
    })
}

/// Apply date fixes: patch frontmatter dates that differ from articles.json.
pub fn apply_date_fixes(
    automation_dir: &Path,
    repo_root: &Path,
) -> std::result::Result<ContentHealthResult, String> {
    let result = sync_and_validate(automation_dir, repo_root, true)?;
    Ok(ContentHealthResult {
        checked: result.checked_entries,
        content_files: result.content_files,
        date_mismatches: result.date_mismatches.len(),
        mismatch_details: vec![],
        orphan_files: result.orphan_files,
        fixed: true,
    })
}

/// Validate that articles.json and the content directory are in sync.
///
/// Mirrors `pageseeds content sync-and-validate --workspace-root <automation_dir> --website-path .`
///
/// When `apply_sync` is true, frontmatter dates that differ from articles.json
/// are patched in-place (same as `--apply-sync` in the CLI).
pub fn sync_and_validate(
    automation_dir: &Path,
    repo_root: &Path,
    apply_sync: bool,
) -> std::result::Result<SyncValidateResult, String> {
    // 1. Read articles.json from the automation workspace.
    let articles_path = automation_dir.join("articles.json");
    let json_str = std::fs::read_to_string(&articles_path)
        .map_err(|e| format!("articles.json not found at {}: {}", articles_path.display(), e))?;
    let doc: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse articles.json: {}", e))?;
    let articles = doc
        .get("articles")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "articles.json must contain an 'articles' array".to_string())?;

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

    for article in articles {
        let id = article.get("id").and_then(|v| v.as_i64());
        let title = article.get("title").and_then(|v| v.as_str()).map(String::from);
        let file_ref = article
            .get("file")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let basename = Path::new(&file_ref)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if basename.is_empty() {
            malformed_file_refs.push(SyncIssue {
                id,
                title,
                file: if file_ref.is_empty() { None } else { Some(file_ref) },
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
            .get("published_date")
            .and_then(|v| v.as_str())
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
        next_action: next_action.into(),
    })
}

/// Clean stale entries from articles.json — remove articles whose files no longer exist.
/// The source of truth is the filesystem. Returns the list of removed article titles.
pub fn clean_stale_articles_json(
    automation_dir: &Path,
    repo_root: &Path,
) -> std::result::Result<Vec<String>, String> {
    let articles_path = automation_dir.join("articles.json");
    let json_str = std::fs::read_to_string(&articles_path)
        .map_err(|e| format!("articles.json not found at {}: {}", articles_path.display(), e))?;
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

    let result =
        setup_check::resolve_content_dir(repo_root, automation_dir, workspace_config.as_ref(), None);

    result.path.ok_or_else(|| {
        format!(
            "Content directory not found ({}). Add a seo_workspace.json with a content_dir field.",
            result.how
        )
    })
}

/// Extract the `date:` value from YAML frontmatter, stripping quotes.
fn extract_frontmatter_date(content: &str) -> Option<String> {
    let (fm, _) = crate::content::cleaner::parse_frontmatter(content)?;
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
fn patch_frontmatter_date(
    content: &str,
    new_date: &str,
) -> std::result::Result<String, String> {
    let Some((fm, body)) = crate::content::cleaner::parse_frontmatter(content) else {
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

// ─── ingest_orphan_files ──────────────────────────────────────────────────────

/// Result returned by `ingest_orphan_files`.
#[derive(Debug, Serialize)]
pub struct IngestOrphanResult {
    /// Number of files successfully ingested into articles.json + SQLite.
    pub ingested: usize,
    /// Basenames of newly added files.
    pub files: Vec<String>,
}

/// Auto-ingest MDX files that exist on disk but are not tracked in articles.json.
///
/// Mirrors `apply_import_from_repo` from the Python CLI
/// (`packages/seo-content-cli/src/seo_content_mcp/seo_ops.py`).
///
/// For each orphan file:
///   1. Parses frontmatter to extract title, slug, and date.
///   2. Derives a URL slug from the filename if not present in frontmatter.
///   3. Assigns the next available article ID.
///   4. Inserts into SQLite and updates articles.json.
pub fn ingest_orphan_files(
    automation_dir: &Path,
    repo_root: &Path,
    project_id: &str,
    conn: &Connection,
) -> std::result::Result<IngestOrphanResult, String> {
    // 1. Identify orphan files via sync_and_validate.
    let sync_result = sync_and_validate(automation_dir, repo_root, false)?;
    if sync_result.orphan_files.is_empty() {
        return Ok(IngestOrphanResult { ingested: 0, files: vec![] });
    }

    // 2. Resolve content directory.
    let content_dir = resolve_content_dir(automation_dir, repo_root)?;

    // 3. Build a map of all content files: basename → full path.
    let content_files: HashMap<String, PathBuf> =
        crate::content::locator::collect_markdown_files(&content_dir)
            .into_iter()
            .filter_map(|p| {
                let name = p.file_name()?.to_str()?.to_string();
                Some((name, p))
            })
            .collect();

    // 4. Compute a safe starting ID: take the max of articles_meta.next_article_id
    //    and MAX(existing id) + 1. articles_meta can be stale if articles were added
    //    externally (e.g. via the Python CLI) without bumping nextArticleId.
    let max_existing_id: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(id), 0) FROM articles WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let meta_next_id: i64 = conn
        .query_row(
            "SELECT next_article_id FROM articles_meta WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let mut next_id = std::cmp::max(max_existing_id + 1, meta_next_id.max(1));

    // 5. Insert a new article row for each orphan.
    let mut ingested_files = Vec::new();
    for basename in &sync_result.orphan_files {
        let file_path = match content_files.get(basename) {
            Some(p) => p,
            None => continue,
        };

        let meta = match read_file_metadata(file_path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let url_slug = derive_url_slug(basename);
        let title = meta.title.unwrap_or_else(|| url_slug.replace('-', " "));
        let file_ref = format!("./content/{}", basename);

        // No ON CONFLICT clause — if the ID somehow already exists, surface the error
        // rather than silently skipping the row (which would cause a phantom orphan).
        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                content_gaps_addressed, estimated_traffic_monthly, project_id
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            rusqlite::params![
                next_id,
                title,
                url_slug,
                file_ref,
                Option::<String>::None,
                Option::<String>::None,
                0i64,
                meta.published_date,
                meta.word_count as i64,
                "published",
                "[]",
                Option::<String>::None,
                project_id,
            ],
        )
        .map_err(|e| format!("Failed to insert '{}' (id {}): {}", basename, next_id, e))?;

        ingested_files.push(basename.clone());
        next_id += 1;
    }

    if ingested_files.is_empty() {
        return Ok(IngestOrphanResult { ingested: 0, files: vec![] });
    }

    // 6. Update articles_meta with the new next_article_id.
    conn.execute(
        "INSERT INTO articles_meta (project_id, next_article_id)
         VALUES (?1, ?2)
         ON CONFLICT(project_id) DO UPDATE SET next_article_id = excluded.next_article_id",
        rusqlite::params![project_id, next_id],
    )
    .map_err(|e| e.to_string())?;

    // 7. Write articles.json back to the repo (bypassing the date-policy gate since
    //    we may be ingesting old published content that hasn't been date-indexed yet).
    let json = crate::db::export::export_articles(conn, project_id)
        .map_err(|e| e.to_string())?;
    let articles_json_path = repo_root
        .join(".github")
        .join("automation")
        .join("articles.json");
    std::fs::create_dir_all(articles_json_path.parent().unwrap())
        .map_err(|e| e.to_string())?;
    std::fs::write(&articles_json_path, json).map_err(|e| e.to_string())?;

    let count = ingested_files.len();
    Ok(IngestOrphanResult { ingested: count, files: ingested_files })
}

/// Derive a URL slug for an MDX file, mirroring the Python CLI convention.
///
/// "242_pour_over_coffee_cafes_auckland.mdx" → "pour-over-coffee-cafes-auckland"
fn derive_url_slug(filename: &str) -> String {
    let base = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    // Strip leading numeric prefix (matches Python CLI: `re.sub(r"^\d+[_-]+", "", base)`)
    let re = Regex::new(r"^\d+[_\-]+").unwrap();
    let stripped = re.replace(base, "");
    stripped.to_lowercase().replace('_', "-")
}
