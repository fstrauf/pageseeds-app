use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::*;
// ─── Internal helpers ─────────────────────────────────────────────────────────

pub(crate) fn parse_target_artifact(task: &Task) -> Option<serde_json::Value> {
    task.artifacts
        .iter()
        .find(|a| a.key == "indexing_link_target")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|v| v.get("target").cloned())
}

pub(crate) fn find_file_by_article_id(project_id: &str, article_id: i64) -> Option<String> {
    let db_path = crate::db::default_db_path();
    let db = rusqlite::Connection::open(&db_path).ok()?;
    let articles = crate::content::article_index::list_articles(&db, project_id).ok()?;
    articles
        .into_iter()
        .find(|a| a.id == article_id)
        .map(|a| a.file)
}

