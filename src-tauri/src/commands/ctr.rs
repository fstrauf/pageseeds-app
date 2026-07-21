use tauri::State;

use crate::models::ctr::CtrOutcome;

use super::AppState;

/// List CTR fix outcomes (baseline/after metrics + verdict) for a project.
#[tauri::command]
pub fn list_ctr_outcomes(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<CtrOutcome>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::list_ctr_outcomes(&db, &project_id).map_err(|e| e.to_string())
}
