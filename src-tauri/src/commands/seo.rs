use tauri::State;
use crate::commands::{AppState, SeoState};
use crate::config::env_resolver::EnvResolver;
use crate::engine::task_store;
use crate::seo::provider::SeoDataProvider;
use crate::seo::keywords::{KeywordDifficultyResult, KeywordIdeasResult, KeywordIdea};
use crate::seo::intent::IntentClassification;
use crate::seo::scoring::{OpportunityScore, score_opportunities};

/// Resolve the SEO provider for a project.
async fn resolve_provider_for_project(
    state: &State<'_, AppState>,
    project_id: &str,
) -> Result<Box<dyn SeoDataProvider>, String> {
    let (project_path, seo_provider) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, project_id).map_err(|e| e.to_string())?;
        let provider = project.seo_provider.clone().unwrap_or_else(|| "ahrefs".to_string());
        (project.path, provider)
    };

    let resolver = EnvResolver::new(&project_path);
    crate::seo::resolve_provider(&seo_provider, &resolver).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seo_get_keyword_ideas(
    state: State<'_, AppState>,
    project_id: String,
    keyword: String,
    country: Option<String>,
    search_engine: Option<String>,
) -> Result<KeywordIdeasResult, String> {
    let provider = resolve_provider_for_project(&state, &project_id).await?;
    provider
        .keyword_ideas(&keyword, country.as_deref().unwrap_or("us"), search_engine.as_deref().unwrap_or("Google"))
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
    let provider = resolve_provider_for_project(&state, &project_id).await?;
    provider
        .keyword_difficulty(&keyword, country.as_deref().unwrap_or("us"))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seo_batch_keyword_difficulty(
    state: State<'_, AppState>,
    project_id: String,
    keywords: Vec<String>,
    country: Option<String>,
) -> Result<Vec<KeywordDifficultyResult>, String> {
    let provider = resolve_provider_for_project(&state, &project_id).await?;
    provider
        .batch_keyword_difficulty(&keywords, country.as_deref().unwrap_or("us"))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seo_get_backlinks(
    state: State<'_, AppState>,
    seo_state: State<'_, SeoState>,
    project_id: String,
    domain: String,
) -> Result<crate::seo::backlinks::BacklinksResult, String> {
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
) -> Result<crate::seo::traffic::TrafficResult, String> {
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

#[tauri::command]
pub async fn get_seo_provider(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    Ok(project.seo_provider.unwrap_or_else(|| "ahrefs".to_string()))
}

#[tauri::command]
pub async fn set_seo_provider(
    state: State<'_, AppState>,
    project_id: String,
    provider: String,
) -> Result<(), String> {
    let mut db = state.db.lock().map_err(|e| e.to_string())?;
    let mut project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    
    // Validate provider name
    let valid_provider = match provider.to_lowercase().as_str() {
        "dataforseo" => "dataforseo",
        _ => "ahrefs",
    };
    
    project.seo_provider = Some(valid_provider.to_string());
    task_store::update_project(&mut db, &project).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn classify_search_intent(
    state: State<'_, AppState>,
    project_id: String,
    keywords: Vec<String>,
) -> Result<Vec<IntentClassification>, String> {
    let provider = resolve_provider_for_project(&state, &project_id).await?;
    provider.search_intent(&keywords).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn score_keyword_opportunities(
    state: State<'_, AppState>,
    _project_id: String,
    keywords: Vec<KeywordIdea>,
    intents: Vec<IntentClassification>,
    existing_slugs: Vec<String>,
) -> Result<Vec<OpportunityScore>, String> {
    Ok(score_opportunities(&keywords, &intents, &existing_slugs))
}

fn capsolver_key(db: &rusqlite::Connection, project_id: &str) -> Result<String, String> {
    let project = task_store::get_project(db, project_id).map_err(|e| e.to_string())?;
    let resolver = EnvResolver::new(&project.path);
    resolver
        .resolve("CAPSOLVER_API_KEY")
        .map(|(v, _)| v)
        .ok_or_else(|| "CAPSOLVER_API_KEY not configured. Add it to ~/.config/automation/secrets.env".to_string())
}
