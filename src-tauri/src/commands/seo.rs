use tauri::State;
use crate::config::env_resolver::EnvResolver;
use crate::engine::task_store;
use crate::seo::backlinks::BacklinksResult;
use crate::seo::keywords::{KeywordDifficultyResult, KeywordIdeasResult};
use crate::seo::traffic::TrafficResult;
use super::{AppState, SeoState};

fn capsolver_key(db: &rusqlite::Connection, project_id: &str) -> Result<String, String> {
    let project = task_store::get_project(db, project_id).map_err(|e| e.to_string())?;
    let resolver = EnvResolver::new(&project.path);
    resolver
        .resolve("CAPSOLVER_API_KEY")
        .map(|(v, _)| v)
        .ok_or_else(|| "CAPSOLVER_API_KEY not configured. Add it to ~/.config/automation/secrets.env".to_string())
}

#[tauri::command]
pub async fn seo_get_keyword_ideas(
    state: State<'_, AppState>,
    project_id: String,
    keyword: String,
    country: Option<String>,
    search_engine: Option<String>,
) -> Result<KeywordIdeasResult, String> {
    let api_key = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        capsolver_key(&db, &project_id)?
    };
    crate::seo::keywords::get_keyword_ideas(
        &api_key,
        &keyword,
        country.as_deref().unwrap_or("us"),
        search_engine.as_deref().unwrap_or("Google"),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seo_get_keyword_difficulty(
    state: State<'_, AppState>,
    project_id: String,
    keyword: String,
    country: Option<String>,
) -> Result<KeywordDifficultyResult, String> {
    let api_key = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        capsolver_key(&db, &project_id)?
    };
    crate::seo::keywords::get_keyword_difficulty(
        &api_key,
        &keyword,
        country.as_deref().unwrap_or("us"),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seo_get_backlinks(
    state: State<'_, AppState>,
    seo_state: State<'_, SeoState>,
    project_id: String,
    domain: String,
) -> Result<BacklinksResult, String> {
    let api_key = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        capsolver_key(&db, &project_id)?
    };
    crate::seo::backlinks::get_backlinks(&api_key, &domain, &seo_state.sig_cache)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seo_check_traffic(
    state: State<'_, AppState>,
    project_id: String,
    domain: String,
    mode: Option<String>,
    country: Option<String>,
) -> Result<TrafficResult, String> {
    let api_key = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        capsolver_key(&db, &project_id)?
    };
    crate::seo::traffic::check_traffic(
        &api_key,
        &domain,
        mode.as_deref().unwrap_or("subdomains"),
        country.as_deref().unwrap_or("None"),
    )
    .await
    .map_err(|e| e.to_string())
}
