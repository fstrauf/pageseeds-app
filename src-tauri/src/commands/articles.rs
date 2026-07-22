use super::AppState;
use crate::content::article_evidence;
use crate::content::date_policy;
use crate::db::export;
use crate::engine::task_store;
use crate::models::article::Article;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn list_articles(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<Article>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(task_store::list_articles(&db, &project_id)?)
}

#[derive(serde::Serialize)]
pub struct ImportResult {
    pub tasks_imported: usize,
    pub articles_imported: usize,
}

#[tauri::command]
pub fn import_from_repo(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ImportResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id)?;
    let project_path = std::path::PathBuf::from(&project.path);

    let tasks_imported = export::read_task_list_from_repo(&db, &project_id, &project_path)?;

    let articles_imported = export::read_articles_from_repo(&db, &project_id, &project_path)?;

    Ok(ImportResult {
        tasks_imported,
        articles_imported,
    })
}

#[tauri::command]
pub fn export_to_repo(state: State<'_, AppState>, project_id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let articles = task_store::list_articles(&db, &project_id)?;
    let report = date_policy::validate_no_future_dates(&articles);
    if !report.is_valid() {
        let detail = report
            .issues
            .iter()
            .take(8)
            .map(|i| format!("id {} {} ({})", i.article_id, i.description, i.current_date))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "Date policy check failed: {} issue(s). Future-dated articles must be corrected before export. {}",
            report.issues.len(),
            detail
        ));
    }
    let project = task_store::get_project(&db, &project_id)?;
    let project_path = std::path::PathBuf::from(&project.path);

    export::write_task_list_to_repo(&db, &project_id, &project_path)?;
    export::write_articles_to_repo(&db, &project_id, &project_path)?;

    Ok(())
}

/// Reindex durable article evidence (facts + embeddings) for a project.
///
/// Unchanged content_hash skips re-embed. When Ollama is unavailable, facts are
/// still stored with `embedding_json` NULL (no soft mega-cluster fallback).
#[tauri::command]
pub async fn reindex_article_evidence(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<article_evidence::IndexReport, String> {
    let project_path = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        std::path::PathBuf::from(project.path)
    };

    let db_arc = Arc::clone(&state.db);
    tokio::task::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        rt.block_on(async {
            article_evidence::index_stale(&db, &project_id, &project_path).await
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Coverage of live articles vs the article_evidence catalog.
#[tauri::command]
pub fn get_article_evidence_coverage(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<article_evidence::CoverageReport, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    article_evidence::coverage(&db, &project_id).map_err(|e| e.to_string())
}
