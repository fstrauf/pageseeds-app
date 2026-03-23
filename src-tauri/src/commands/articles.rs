use tauri::State;
use crate::db::export;
use crate::content::date_policy;
use crate::engine::task_store;
use crate::models::article::Article;
use super::AppState;

#[tauri::command]
pub fn list_articles(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<Article>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())
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
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let project_path = std::path::PathBuf::from(&project.path);

    let tasks_imported =
        export::read_task_list_from_repo(&db, &project_id, &project_path)
            .map_err(|e| e.to_string())?;

    let articles_imported =
        export::read_articles_from_repo(&db, &project_id, &project_path)
            .map_err(|e| e.to_string())?;

    Ok(ImportResult {
        tasks_imported,
        articles_imported,
    })
}

#[tauri::command]
pub fn export_to_repo(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    let report = date_policy::validate_publish_ready_dates(&articles);
    if !report.is_valid() {
        let detail = report
            .issues
            .iter()
            .take(8)
            .map(|i| format!("id {} {} ({})", i.article_id, i.description, i.current_date))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "Date policy check failed: {} issue(s). Resolve future/duplicate dates before export. {}",
            report.issues.len(),
            detail
        ));
    }
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let project_path = std::path::PathBuf::from(&project.path);

    export::write_task_list_to_repo(&db, &project_id, &project_path)
        .map_err(|e| e.to_string())?;
    export::write_articles_to_repo(&db, &project_id, &project_path)
        .map_err(|e| e.to_string())?;

    Ok(())
}
