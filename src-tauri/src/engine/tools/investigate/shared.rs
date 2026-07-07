use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::project_paths::ProjectPaths;
use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Standalone tool functions — shared by Tool impls and CLI
// ═══════════════════════════════════════════════════════════════════════════════

/// Scan all article titles and return pattern counts.
pub fn scan_article_titles(ctx: &InvestigationContext) -> Result<serde_json::Value, InvestigationToolError> {
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let articles = crate::engine::task_store::list_articles(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;
    let mut missing = 0usize; let mut dup = 0usize; let mut lit = 0usize; let mut long = 0usize;
    let mut examples: Vec<serde_json::Value> = Vec::new();
    for a in &articles {
        let t = a.title.trim();
        if t.is_empty() { missing += 1; continue; }
        let tl = t.to_lowercase();
        if tl.contains("| brand |") || tl.contains("{brand}") || tl.contains("{{title}}") {
            lit += 1;
            if examples.len() < 5 { examples.push(serde_json::json!({"title": t, "slug": a.url_slug, "issue": "literal template variable"})); }
        }
        let tokens: Vec<&str> = tl.split(|c: char| !c.is_alphanumeric()).filter(|s| s.len() > 2).collect();
        let mut counts = std::collections::HashMap::new();
        for tok in &tokens { *counts.entry(*tok).or_insert(0) += 1; }
        if counts.values().any(|&c| c >= 2) {
            dup += 1;
            if examples.len() < 5 {
                let w = counts.iter().find(|(_, &c)| c >= 2).map(|(w, _)| *w).unwrap_or("");
                examples.push(serde_json::json!({"title": t, "slug": a.url_slug, "issue": format!("token '{}' appears {} times", w, counts[w])}));
            }
        }
        if t.len() > 60 { long += 1; }
    }
    Ok(serde_json::json!({
        "total_titles": articles.len(), "missing_titles": missing,
        "duplicate_token_titles": dup, "literal_var_titles": lit,
        "long_titles": long, "examples": examples,
    }))
}

/// Hash all article bodies and find exact duplicate groups.
pub fn hash_article_bodies(ctx: &InvestigationContext) -> Result<Vec<serde_json::Value>, InvestigationToolError> {
    use sha2::{Digest, Sha256};
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let articles = crate::engine::task_store::list_articles(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;
    let paths = ctx.paths();
    let mut groups: std::collections::HashMap<String, Vec<serde_json::Value>> = std::collections::HashMap::new();
    for a in &articles {
        let source = crate::engine::exec::utils::read_source_file(&paths.repo_root, &a.file);
        let (_fm, body) = crate::engine::exec::utils::parse_frontmatter(source.as_deref().unwrap_or(""));
        let mut h = Sha256::new();
        h.update(body.as_bytes());
        let hash = format!("{:x}", h.finalize());
        groups.entry(hash).or_default().push(serde_json::json!({
            "id": a.id, "title": a.title, "slug": a.url_slug, "file": a.file,
        }));
    }
    Ok(groups.into_iter().filter(|(_, v)| v.len() > 1)
        .map(|(hash, arts)| serde_json::json!({"hash": hash, "count": arts.len(), "articles": arts}))
        .collect())
}

/// Read content audit report from DB (primary) or legacy JSON file (fallback).
pub fn read_content_audit_report(project_path: &str) -> Result<serde_json::Value, InvestigationToolError> {
    // Try database first
    if let Ok(conn) = rusqlite::Connection::open(crate::db::default_db_path()) {
        let project_id: Option<String> = conn
            .query_row(
                "SELECT id FROM projects WHERE path = ?1",
                [project_path],
                |row| row.get(0),
            )
            .ok();
        if let Some(pid) = project_id {
            if let Ok(Some(json)) = crate::db::content_audit::get_audit_report_as_json(&conn, &pid) {
                return Ok(json);
            }
        }
    }

    // Fallback: legacy JSON file during transition
    let paths = crate::engine::project_paths::ProjectPaths::from_path(project_path);
    let p = paths.automation_dir.join("content_audit.json");
    if !p.exists() {
        return Err(InvestigationToolError::NotAvailable("No content audit found. Run run_content_audit first.".into()));
    }
    let s = std::fs::read_to_string(&p)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
    serde_json::from_str(&s)
        .map_err(|e| InvestigationToolError::Execution(format!("Invalid JSON: {e}")))
}

/// Read cannibalization_strategy.json from disk.
pub fn read_cannibalization_clusters(project_path: &str) -> Result<serde_json::Value, InvestigationToolError> {
    let paths = crate::engine::project_paths::ProjectPaths::from_path(project_path);
    let p = paths.automation_dir.join("cannibalization_strategy.json");
    if !p.exists() { return Ok(serde_json::json!({"clusters": [], "note": "No strategy found. Run cannibalization_audit first."})); }
    let s = std::fs::read_to_string(&p)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
    serde_json::from_str(&s)
        .map_err(|e| InvestigationToolError::Execution(format!("Invalid JSON: {e}")))
}

/// Get GSC URL indexing status summary.
pub fn get_indexing_status(ctx: &InvestigationContext) -> Result<serde_json::Value, InvestigationToolError> {
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let statuses = crate::gsc::db::list_by_project(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to load indexing status: {e}")))?;
    let total = statuses.len();
    let indexed = statuses.iter().filter(|s| s.last_reason_code.as_deref() == Some("indexed_pass")).count();
    let mut reasons: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for s in &statuses {
        if let Some(r) = &s.last_reason_code { if r != "indexed_pass" { *reasons.entry(r.clone()).or_default() += 1; } }
    }
    Ok(serde_json::json!({
        "total_urls": total, "indexed": indexed, "not_indexed": total.saturating_sub(indexed),
        "issues_by_reason": reasons.iter().map(|(k, v)| serde_json::json!({"reason": k, "count": v})).collect::<Vec<_>>(),
    }))
}

/// Read framework files from the project repo.
pub fn read_framework_files(project_path: &str, file: Option<&str>) -> Result<serde_json::Value, InvestigationToolError> {
    let root = std::path::Path::new(project_path);
    let candidates = [
        ("app/layout.tsx", "Next.js app layout"),
        ("pages/_app.tsx", "Next.js pages app"),
        ("next.config.js", "Next.js config"),
        ("next-sitemap.config.js", "Sitemap config"),
        ("app/sitemap.ts", "App router sitemap"),
        ("robots.txt", "Robots exclusion"),
    ];
    if let Some(f) = file {
        let p = root.join(f);
        if !p.exists() { return Err(InvestigationToolError::NotAvailable(format!("File not found: {f}"))); }
        let content = std::fs::read_to_string(&p)
            .map_err(|e| InvestigationToolError::Execution(format!("Failed to read: {e}")))?;
        let truncated = if content.len() > 8000 { format!("{}...\n[truncated from {} chars]", &content[..8000], content.len()) } else { content };
        Ok(serde_json::json!({"file": f, "content": truncated}))
    } else {
        let found: Vec<serde_json::Value> = candidates.iter().map(|(f, desc)| {
            serde_json::json!({"path": f, "description": desc, "exists": root.join(f).exists()})
        }).collect();
        Ok(serde_json::json!({"files": found, "repo_root": root.to_string_lossy()}))
    }
}

/// Scan internal link graph.
pub fn scan_link_graph(ctx: &InvestigationContext) -> Result<serde_json::Value, InvestigationToolError> {
    let paths = ctx.paths();
    let content_dir = crate::content::ops::resolve_content_dir(&paths.automation_dir, &paths.repo_root)
        .map_err(|e| InvestigationToolError::NotAvailable(format!("Content dir not found: {e}")))?;
    let db = ctx.open_db().map_err(|e| InvestigationToolError::Execution(e))?;
    let articles = crate::engine::task_store::list_articles(&db, &ctx.project_id)
        .map_err(|e| InvestigationToolError::Execution(format!("Failed to list articles: {e}")))?;
    drop(db);
    let scan = crate::content::linking::scan_links(&content_dir, &articles)
        .map_err(|e| InvestigationToolError::Execution(format!("Link scan failed: {e}")))?;
    let orphans: Vec<serde_json::Value> = scan.orphan_ids.iter().map(|&id| {
        let a = articles.iter().find(|a| a.id == id);
        serde_json::json!({"id": id, "title": a.map(|a| a.title.as_str()).unwrap_or(""), "slug": a.map(|a| a.url_slug.as_str()).unwrap_or("")})
    }).collect();
    Ok(serde_json::json!({
        "total_articles": scan.total_articles, "total_internal_links": scan.total_internal_links,
        "orphan_count": scan.orphan_ids.len(), "orphans": orphans,
    }))
}
