/// Clean example of how commands should look with the runtime abstraction
/// 
/// THIS IS A DEMONSTRATION FILE - not compiled into the app
/// Use this pattern when refactoring existing commands

use tauri::State;
use crate::config::env_resolver::EnvResolver;
use crate::engine::{batch, executor, ledger, scheduler, task_store, runtime};
use super::{AppState, GscState};

/// Example: execute_task with the runtime abstraction
#[tauri::command]
pub async fn execute_task_clean(
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    app_handle: tauri::AppHandle,
    task_id: String,
) -> Result<executor::ExecutionResult, String> {
    // 1. Do async prep work (token refresh, etc.)
    let token = get_gsc_token(&state, &gsc_state, &task_id).await?;
    
    // 2. Run the async executor with a fresh connection
    // This replaces the 8-line boilerplate with 4 clean lines
    runtime::with_connection_timeout(&state.db_path, 10, |conn| async move {
        executor::execute_task_with_token(
            conn, 
            &task_id, 
            token.as_deref(), 
            Some(app_handle), 
            false
        ).await
    }).await
}

/// Example: dry_run_task is now 3 lines instead of 10
#[tauri::command]
pub async fn dry_run_task_clean(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<executor::ExecutionResult, String> {
    runtime::with_connection(&state.db_path, |conn| async move {
        executor::dry_run_task(conn, &task_id).await
    }).await
}

/// Example: run_batch
#[tauri::command]
pub async fn run_batch_clean(
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
    
    let token = get_cached_token(&gsc_state).await;
    
    runtime::with_connection_timeout(&state.db_path, 10, |conn| async move {
        batch::run_batch_with_token(conn, &project_id, &config, token.as_deref()).await
    }).await
}

/// Example: scheduler cycle
#[tauri::command]
pub async fn run_scheduler_cycle_clean(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<scheduler::SchedulerCycleResult, String> {
    runtime::with_connection(&state.db_path, |conn| async move {
        scheduler::run_cycle(conn, &project_id).await
    }).await
}

// Helper to keep commands clean
async fn get_gsc_token(
    state: &State<'_, AppState>,
    gsc_state: &State<'_, GscState>,
    task_id: &str,
) -> Result<Option<String>, String> {
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
            let task = task_store::get_task(&db, task_id).map_err(|e| e.to_string())?;
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
    
    Ok(token)
}

async fn get_cached_token(gsc_state: &State<'_, GscState>) -> Option<String> {
    gsc_state
        .token
        .lock()
        .ok()
        .and_then(|t| t.as_ref().filter(|t| !t.is_expired()).map(|t| t.access_token.clone()))
}
