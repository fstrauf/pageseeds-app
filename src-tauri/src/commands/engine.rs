use std::sync::Arc;
use tauri::State;
use crate::config::env_resolver::EnvResolver;
use crate::engine::{batch, executor, ledger, scheduler, task_store};
use super::{AppState, GscState};

#[tauri::command]
pub async fn execute_task(
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    task_id: String,
) -> Result<executor::ExecutionResult, String> {
    let db_arc = Arc::clone(&state.db);
    let mut token = gsc_state
        .token
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .filter(|t| !t.is_expired())
        .map(|t| t.access_token.clone());

    // If there is no cached token, attempt service-account auth and cache it.
    if token.is_none() {
        let project_path = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let task = task_store::get_task(&db, &task_id).map_err(|e| e.to_string())?;
            let project = task_store::get_project(&db, &task.project_id).map_err(|e| e.to_string())?;
            project.path
        };

        let resolver = EnvResolver::new(&project_path);
        if let Some(sa_path) = resolver
            .resolve("GSC_SERVICE_ACCOUNT_PATH")
            .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
            .map(|(v, _)| v)
        {
            if let Ok(token_state) = crate::gsc::auth::get_service_account_token(&sa_path).await {
                token = Some(token_state.access_token.clone());
                if let Ok(mut guard) = gsc_state.token.lock() {
                    *guard = Some(token_state);
                }
            }
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        executor::execute_task_with_token(&db, &task_id, token.as_deref())
            .map_err(|e| e.to_string())
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
    gsc_state: State<'_, GscState>,
    project_id: String,
    max_tasks: Option<usize>,
    pause_on_error: Option<bool>,
) -> Result<batch::BatchResult, String> {
    let config = batch::BatchConfig {
        max_tasks: max_tasks.unwrap_or(20),
        pause_on_error: pause_on_error.unwrap_or(true),
        delay_secs: 0.5,
    };

    let mut token = gsc_state
        .token
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .filter(|t| !t.is_expired())
        .map(|t| t.access_token.clone());

    if token.is_none() {
        let project_path = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
            project.path
        };

        let resolver = EnvResolver::new(&project_path);
        if let Some(sa_path) = resolver
            .resolve("GSC_SERVICE_ACCOUNT_PATH")
            .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
            .map(|(v, _)| v)
        {
            if let Ok(token_state) = crate::gsc::auth::get_service_account_token(&sa_path).await {
                token = Some(token_state.access_token.clone());
                if let Ok(mut guard) = gsc_state.token.lock() {
                    *guard = Some(token_state);
                }
            }
        }
    }

    let db_arc = Arc::clone(&state.db);
    tauri::async_runtime::spawn_blocking(move || {
        let db = db_arc.lock().map_err(|e| e.to_string())?;
        batch::run_batch_with_token(&db, &project_id, &config, token.as_deref())
            .map_err(|e| e.to_string())
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
