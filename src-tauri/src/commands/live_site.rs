use tauri::State;

use crate::engine::task_store;
use crate::models::live_site::{
    LiveSiteAuditReport, LiveSiteGscSyncResult, LiveSiteImportResult, LiveSiteLinkScanResult,
    LiveSitePage,
};
use crate::models::project::ProjectMode;

use super::{AppState, GscState};

#[tauri::command]
pub async fn import_live_site(
    state: State<'_, AppState>,
    project_id: String,
    limit: Option<usize>,
) -> Result<LiveSiteImportResult, String> {
    let project = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?
    };

    if project.project_mode != ProjectMode::LiveSite {
        return Err("Live site import is only available for live-site projects".to_string());
    }

    let inventory = crate::live_site::import_project_site(&project, limit)
        .await
        .map_err(|e| e.to_string())?;

    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::live_site::store_imported_site_inventory(&db, &project_id, inventory)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_live_site_pages(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<LiveSitePage>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::live_site::list_live_site_pages(&db, &project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_live_site_audit(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<LiveSiteAuditReport, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;

    if project.project_mode != ProjectMode::LiveSite {
        return Err("Live-site audit is only available for live-site projects".to_string());
    }

    crate::live_site::get_live_site_audit(&db, &project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn scan_live_site_links(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<LiveSiteLinkScanResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;

    if project.project_mode != ProjectMode::LiveSite {
        return Err("Live-site link scan is only available for live-site projects".to_string());
    }

    crate::live_site::scan_live_site_links(&db, &project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sync_live_site_gsc(
    state: State<'_, AppState>,
    gsc_state: State<'_, GscState>,
    project_id: String,
    start_date: String,
    end_date: String,
    limit: Option<u32>,
) -> Result<LiveSiteGscSyncResult, String> {
    let project = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?
    };

    if project.project_mode != ProjectMode::LiveSite {
        return Err("Live-site GSC sync is only available for live-site projects".to_string());
    }

    let token = {
        let guard = gsc_state.token.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some(token) if !token.is_expired() => token.access_token.clone(),
            Some(_) => return Err("GSC token has expired. Please re-authenticate.".to_string()),
            None => {
                return Err(
                    "Not authenticated. Call gsc_authenticate or gsc_oauth_start first."
                        .to_string(),
                )
            }
        }
    };

    let site_url = project
        .site_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Live-site project has no site URL configured".to_string())?
        .to_string();

    let page_rows = crate::gsc::analytics::fetch_page_rows(
        &token,
        &site_url,
        &start_date,
        &end_date,
        limit.unwrap_or(250),
    )
    .await
    .map_err(|e| e.to_string())?;

    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::live_site::apply_live_site_gsc_metrics(&db, &project, &start_date, &end_date, page_rows)
        .map_err(|e| e.to_string())
}
