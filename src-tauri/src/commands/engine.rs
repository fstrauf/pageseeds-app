use std::sync::Arc;
use tauri::State;
use crate::engine::{batch, executor, ledger, scheduler, task_store};
use super::AppState;

#[tauri::command]
pub async fn execute_task(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<executor::ExecutionResult, String> {
    let db_arc = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        executor::execute_task(&db, &task_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_batch_summary(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<batch::BatchSummary, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    batch::get_batch_summary(&db, &project_id)
}

#[tauri::command]
pub async fn run_batch(
    state: State<'_, AppState>,
    project_id: String,
    max_tasks: Option<usize>,
    pause_on_error: Option<bool>,
) -> Result<batch::BatchResult, String> {
    let config = batch::BatchConfig {
        max_tasks: max_tasks.unwrap_or(20),
        pause_on_error: pause_on_error.unwrap_or(true),
        delay_secs: 0.5,
    };
    let db_arc = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        batch::run_batch(&db, &project_id, &config).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_scheduler_rules(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<scheduler::SchedulerRule>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    scheduler::list_rules(&db, &project_id)
}

#[tauri::command]
pub fn upsert_scheduler_rule(
    state: State<'_, AppState>,
    rule: scheduler::SchedulerRule,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    scheduler::upsert_rule(&db, &rule)
}

#[tauri::command]
pub fn delete_scheduler_rule(
    state: State<'_, AppState>,
    rule_id: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    scheduler::delete_rule(&db, &rule_id)
}

#[tauri::command]
pub fn set_scheduler_rule_enabled(
    state: State<'_, AppState>,
    rule_id: String,
    enabled: bool,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    scheduler::set_rule_enabled(&db, &rule_id, enabled)
}

#[tauri::command]
pub fn run_scheduler_cycle(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<scheduler::SchedulerCycleResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    scheduler::run_cycle(&db, &project_id)
}

#[tauri::command]
pub fn list_ledger_runs(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let l = ledger::Ledger::new(std::path::Path::new(&project.path));
    l.list_runs()
}

#[tauri::command]
pub fn get_ledger_run_summary(
    state: State<'_, AppState>,
    project_id: String,
    run_id: String,
) -> Result<ledger::RunSummary, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let l = ledger::Ledger::new(std::path::Path::new(&project.path));
    l.get_summary(&run_id)
}

#[tauri::command]
pub fn get_ledger_run_events(
    state: State<'_, AppState>,
    project_id: String,
    run_id: String,
) -> Result<Vec<ledger::LedgerEvent>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let l = ledger::Ledger::new(std::path::Path::new(&project.path));
    l.get_events(&run_id)
}
