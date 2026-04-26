use tauri::State;
use crate::config::env_resolver::EnvResolver;
use crate::engine::task_store;
use crate::models::gsc::{
    Coverage404Record, GscAuthStatus, InspectionRecord, MoverMetrics, PageMetrics, QueryMetrics,
    RedirectRecord,
};
use super::{AppState, GscState};

#[tauri::command]
pub fn gsc_get_auth_status(
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    project_id: String,
) -> Result<GscAuthStatus, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;

    let resolver = EnvResolver::new(&project.path);

    let sa_path = resolver
        .resolve("GSC_SERVICE_ACCOUNT_PATH")
        .map(|(v, _)| v)
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v));
    let oauth_path = resolver.resolve("GSC_REPORT_OAUTH_CLIENT_SECRETS").map(|(v, _)| v);

    let token_ok = gsc_state
        .token
        .lock()
        .map(|t| t.as_ref().map(|t| !t.is_expired()).unwrap_or(false))
        .unwrap_or(false);

    let method = if token_ok {
        if sa_path.is_some() {
            Some("service_account".to_string())
        } else {
            Some("oauth2".to_string())
        }
    } else {
        None
    };

    Ok(GscAuthStatus {
        service_account_configured: sa_path.is_some(),
        oauth_configured: oauth_path.is_some(),
        authenticated: token_ok,
        method,
        sa_path: sa_path.clone(),
        oauth_path: oauth_path.clone(),
    })
}

#[tauri::command]
pub async fn gsc_authenticate(
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    project_id: String,
) -> Result<(), String> {
    let sa_path = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        let resolver = EnvResolver::new(&project.path);
        resolver
            .resolve("GSC_SERVICE_ACCOUNT_PATH")
            .map(|(v, _)| v)
            .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS").map(|(v, _)| v))
            .ok_or("GSC_SERVICE_ACCOUNT_PATH / GOOGLE_APPLICATION_CREDENTIALS not set".to_string())?
    };

    let token = crate::gsc::auth::get_service_account_token(&sa_path)
        .await
        .map_err(|e| e.to_string())?;

    let mut guard = gsc_state.token.lock().map_err(|e| e.to_string())?;
    *guard = Some(token);
    Ok(())
}

#[tauri::command]
pub async fn gsc_oauth_start(
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    project_id: String,
) -> Result<(), String> {
    let oauth_path = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        let resolver = EnvResolver::new(&project.path);
        resolver
            .resolve("GSC_REPORT_OAUTH_CLIENT_SECRETS")
            .map(|(v, _)| v)
            .ok_or("GSC_REPORT_OAUTH_CLIENT_SECRETS not set".to_string())?
    };

    let token = crate::gsc::auth::start_oauth_flow(&oauth_path)
        .await
        .map_err(|e| e.to_string())?;

    let mut guard = gsc_state.token.lock().map_err(|e| e.to_string())?;
    *guard = Some(token);
    Ok(())
}

#[tauri::command]
pub async fn gsc_fetch_analytics(
    gsc_state: State<'_, GscState>,
    site_url: String,
    start_date: String,
    end_date: String,
    limit: Option<u32>,
) -> Result<Vec<PageMetrics>, String> {
    let token = gsc_token(&gsc_state)?;
    crate::gsc::analytics::fetch_page_rows(&token, &site_url, &start_date, &end_date, limit.unwrap_or(25))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn gsc_fetch_queries_for_page(
    gsc_state: State<'_, GscState>,
    site_url: String,
    page_url: String,
    start_date: String,
    end_date: String,
    limit: Option<u32>,
) -> Result<Vec<QueryMetrics>, String> {
    let token = gsc_token(&gsc_state)?;
    crate::gsc::analytics::fetch_queries_for_page(
        &token,
        &site_url,
        &page_url,
        &start_date,
        &end_date,
        limit.unwrap_or(25),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn gsc_compute_movers(
    gsc_state: State<'_, GscState>,
    site_url: String,
    curr_start: String,
    curr_end: String,
    prev_start: String,
    prev_end: String,
    limit: Option<u32>,
) -> Result<Vec<MoverMetrics>, String> {
    let token = gsc_token(&gsc_state)?;
    crate::gsc::analytics::compute_movers(
        &token,
        &site_url,
        &curr_start,
        &curr_end,
        &prev_start,
        &prev_end,
        limit.unwrap_or(50),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn gsc_inspect_urls(
    gsc_state: State<'_, GscState>,
    site_url: String,
    urls: Vec<String>,
) -> Result<Vec<InspectionRecord>, String> {
    let token = gsc_token(&gsc_state)?;
    crate::gsc::indexing::inspect_batch(&token, &site_url, urls)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gsc_generate_indexing_report(
    state: State<'_, AppState>,
    project_id: String,
    site_url: String,
    records: Vec<InspectionRecord>,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;

    let artifacts_dir = std::path::PathBuf::from(&project.path)
        .join(".github/automation/artifacts");

    crate::gsc::reports::generate_and_save_indexing_report(&records, &site_url, &artifacts_dir)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gsc_parse_coverage_csv(csv_content: String) -> Result<Vec<Coverage404Record>, String> {
    crate::gsc::coverage::parse_coverage_csv(&csv_content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn gsc_parse_redirect_csv(csv_content: String) -> Result<Vec<RedirectRecord>, String> {
    crate::gsc::redirects::parse_redirect_csv(&csv_content).map_err(|e| e.to_string())
}

/// Resolve a GSC token using the shared state cache, falling back to
/// service-account authentication via the project's env resolver.
/// On success, caches the new token in `gsc_state` and returns it.
pub async fn resolve_gsc_token(
    gsc_state: &State<'_, GscState>,
    project_path: &str,
) -> Result<Option<String>, String> {
    // 1. Check cache (scoped so guard never crosses an await)
    let cached = {
        let guard = gsc_state.token.lock().map_err(|e| e.to_string())?;
        guard.as_ref().filter(|t| !t.is_expired()).map(|t| t.access_token.clone())
    };
    if let Some(token) = cached {
        return Ok(Some(token));
    }

    // 2. Try service account
    let resolver = EnvResolver::new(project_path);
    if let Some(sa_path) = resolver
        .resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)
    {
        if let Ok(token_state) = crate::gsc::auth::get_service_account_token(&sa_path).await {
            let token = token_state.access_token.clone();
            if let Ok(mut guard) = gsc_state.token.lock() {
                *guard = Some(token_state);
            }
            return Ok(Some(token));
        }
    }

    Ok(None)
}

pub(super) fn gsc_token(gsc_state: &State<'_, GscState>) -> Result<String, String> {
    let guard = gsc_state.token.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        Some(t) if !t.is_expired() => Ok(t.access_token.clone()),
        Some(_) => Err("GSC token has expired. Please re-authenticate.".to_string()),
        None => Err("Not authenticated. Call gsc_authenticate or gsc_oauth_start first.".to_string()),
    }
}
