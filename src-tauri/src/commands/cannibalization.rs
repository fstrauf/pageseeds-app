/// Cannibalization strategy review and approval commands.
///
/// Thin IPC wrappers — all business logic lives in `crate::cannibalization`.
use tauri::State;

use crate::commands::AppState;
use crate::models::cannibalization::{
    ApprovalStatus, CannibalizationSelection, StrategyReview, StrategyWithReviews,
};
use crate::models::task::Task;

/// Read the cannibalization strategy for a project.
#[tauri::command]
pub fn get_cannibalization_strategy(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Option<StrategyWithReviews>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::cannibalization::get_strategy_with_reviews(&db, &project_id).map_err(|e| e.into())
}

/// Approve or reject a single recommendation.
#[tauri::command]
pub fn set_recommendation_approval(
    state: State<'_, AppState>,
    strategy_id: String,
    project_id: String,
    recommendation_type: String,
    recommendation_id: String,
    status: String,
    notes: Option<String>,
) -> Result<StrategyReview, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    let status_enum = match status.as_str() {
        "approved" => ApprovalStatus::Approved,
        "rejected" => ApprovalStatus::Rejected,
        "needs_review" => ApprovalStatus::NeedsReview,
        _ => ApprovalStatus::Pending,
    };

    crate::db::set_strategy_review(
        &db,
        &strategy_id,
        &project_id,
        &recommendation_type,
        &recommendation_id,
        status_enum,
        None,
        notes.as_deref(),
    )
    .map_err(|e| e.into())
}

/// Get all reviews for a strategy.
#[tauri::command]
pub fn get_strategy_reviews(
    state: State<'_, AppState>,
    strategy_id: String,
) -> Result<Vec<StrategyReview>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::db::list_strategy_reviews(&db, &strategy_id).map_err(|e| e.into())
}

/// Create follow-up tasks from approved recommendations.
#[tauri::command]
pub fn create_tasks_from_approved_recommendations(
    state: State<'_, AppState>,
    strategy_id: String,
    project_id: String,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::cannibalization::spawn_tasks_from_approved(&db, &strategy_id, &project_id)
        .map_err(|e| e.into())
}

/// Create follow-up tasks from user selections in the task drawer.
#[tauri::command]
pub fn create_cannibalization_tasks_from_selection(
    state: State<'_, AppState>,
    parent_task_id: String,
    selections: Vec<CannibalizationSelection>,
) -> Result<Vec<Task>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::cannibalization::spawn_tasks_from_selection(&db, &parent_task_id, &selections)
        .map_err(|e| e.into())
}

/// Backfill hub page_type for existing articles based on heuristics.
#[tauri::command]
pub fn backfill_hub_pages(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<usize, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::cannibalization::backfill_hub_page_types(&db, &project_id).map_err(|e| e.into())
}
