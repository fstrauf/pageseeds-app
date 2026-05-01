use std::path::{Path, PathBuf};
use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;

// ═══════════════════════════════════════════════════════════════════════════════
// Data Structures
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubBrief {
    pub topic: String,
    pub suggested_url: String,
    pub suggested_title: String,
    pub intent: String,
    pub target_keyword: String,
    pub spokes: Vec<HubSpokeBrief>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HubSpokeBrief {
    pub article_id: i64,
    pub title: String,
    pub url_slug: String,
    pub file: String,
    pub impressions: f64,
    pub excerpt: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) fn gather_spoke_briefs(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    spoke_ids: &[i64],
) -> Vec<HubSpokeBrief> {
    let paths = ProjectPaths::from_path(project_path);
    let resolution = crate::content::locator::resolve(&paths.repo_root, None);
    let content_dir = resolution.selected;

    let mut spokes = Vec::new();
    for id in spoke_ids {
        let article: Option<(String, String, String)> = conn
            .query_row(
                "SELECT title, url_slug, file FROM articles WHERE id = ?1 AND project_id = ?2",
                rusqlite::params![id, project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        let (title, url_slug, file) = match article {
            Some((t, s, f)) => (t, s, f),
            None => {
                log::warn!("[hub_build_brief] spoke article {} not found", id);
                continue;
            }
        };

        let excerpt = {
            let repo_path = paths.repo_root.join(&file);
            let path_to_read = if repo_path.exists() {
                repo_path
            } else if let Some(ref dir) = content_dir {
                let basename = std::path::Path::new(&file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&file);
                dir.join(basename)
            } else {
                PathBuf::new()
            };
            if path_to_read.exists() {
                read_excerpt(&path_to_read, 150)
            } else {
                String::new()
            }
        };

        let impressions: f64 = conn
            .query_row(
                "SELECT payload FROM article_metadata
                 WHERE project_id = ?1 AND article_id = ?2 AND namespace = 'gsc'",
                rusqlite::params![project_id, id],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|payload| serde_json::from_str::<serde_json::Value>(&payload).ok())
            .and_then(|v| v.get("impressions").and_then(|i| i.as_f64()))
            .unwrap_or(0.0);

        spokes.push(HubSpokeBrief {
            article_id: *id,
            title,
            url_slug,
            file,
            impressions,
            excerpt,
        });
    }

    spokes
}

fn read_excerpt(file_path: &Path, max_chars: usize) -> String {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let body = crate::content::frontmatter::split_mdx(&content)
        .map(|(_, b)| b)
        .unwrap_or(&content);

    let text = body
        .lines()
        .filter(|l| !l.trim().starts_with("```") && !l.trim().starts_with("---"))
        .collect::<Vec<_>>()
        .join(" ");

    let cleaned = text.replace('*', "").replace('_', "").replace('#', "");

    if cleaned.chars().count() > max_chars {
        let mut excerpt = String::new();
        let mut count = 0;
        for ch in cleaned.chars() {
            if count >= max_chars {
                excerpt.push('…');
                break;
            }
            excerpt.push(ch);
            count += 1;
        }
        excerpt
    } else {
        cleaned
    }
}
