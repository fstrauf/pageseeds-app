use tauri::State;
use crate::engine::task_store;
use super::AppState;

#[tauri::command]
pub fn resolve_content_dir(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::content::locator::ContentDirResolution, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::PathBuf::from(&project.path);
    let resolution = crate::content::locator::resolve(&repo_root, project.content_dir.as_deref());
    Ok(resolution)
}

#[tauri::command]
pub fn scan_content_health(
    state: State<'_, AppState>,
    project_id: String,
    dry_run: bool,
) -> Result<crate::content::cleaner::CleaningResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::PathBuf::from(&project.path);
    let resolution = crate::content::locator::resolve(&repo_root, project.content_dir.as_deref());
    let content_dir = resolution
        .selected
        .ok_or_else(|| "Content directory not found".to_string())?;
    crate::content::cleaner::scan_and_clean(&content_dir, dry_run).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn fix_content_dates(
    state: State<'_, AppState>,
    project_id: String,
    dry_run: bool,
) -> Result<crate::content::dates::DateFixResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    let mut fix_result = crate::content::dates::calculate_fixes(&articles);
    fix_result.dry_run = dry_run;

    if !dry_run {
        let now = chrono::Utc::now().to_rfc3339();
        for fix in &fix_result.fixes {
            db.execute(
                "UPDATE articles SET published_date = ?1 WHERE id = ?2 AND project_id = ?3",
                rusqlite::params![fix.new_date, fix.article_id, project_id],
            ).map_err(|e| e.to_string())?;
        }
        let project_path = std::path::PathBuf::from(&project.path);
        crate::db::export::write_articles_to_repo(&db, &project_id, &project_path)
            .map_err(|e| e.to_string())?;
        let _ = now;
    }

    Ok(fix_result)
}

#[tauri::command]
pub fn scan_content_links(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::content::linking::LinkScanResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::PathBuf::from(&project.path);
    let resolution = crate::content::locator::resolve(&repo_root, project.content_dir.as_deref());
    let content_dir = resolution
        .selected
        .ok_or_else(|| "Content directory not found".to_string())?;
    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    crate::content::linking::scan_links(&content_dir, &articles).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_content_health(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::content::ops::ContentHealthResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    let automation_dir = repo_root.join(".github").join("automation");
    crate::content::ops::content_health_check(&automation_dir, repo_root)
}

#[tauri::command]
pub fn fix_date_mismatches(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::content::ops::ContentHealthResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    let automation_dir = repo_root.join(".github").join("automation");
    crate::content::ops::apply_date_fixes(&automation_dir, repo_root)
}
