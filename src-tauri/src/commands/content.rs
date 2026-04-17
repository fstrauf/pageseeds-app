use tauri::State;
use crate::engine::task_store;
use crate::content::date_policy::{self, DatePolicyConfig, DatePolicyReport};
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
pub fn analyze_article_date_policy(
    state: State<'_, AppState>,
    project_id: String,
    status_filter: Option<Vec<String>>,
    allowed_future_days: Option<i64>,
) -> Result<DatePolicyReport, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;

    let statuses = status_filter.map(|values| {
        values
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect::<std::collections::HashSet<_>>()
    });

    let cfg = DatePolicyConfig {
        allowed_future_days: allowed_future_days.unwrap_or(0),
        statuses,
    };

    Ok(date_policy::validate_dates(&articles, &cfg))
}

#[tauri::command]
pub fn suggest_next_article_publish_date(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    Ok(date_policy::suggest_next_safe_date(&articles))
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
pub fn ingest_orphan_articles(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::content::ops::IngestOrphanResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    let automation_dir = repo_root.join(".github").join("automation");
    crate::content::ops::ingest_orphan_files(&automation_dir, repo_root, &project_id, &db)
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

#[tauri::command]
pub fn preflight_publish_articles(
    state: State<'_, AppState>,
    project_id: String,
    article_ids: Vec<i64>,
) -> Result<crate::content::publish::PublishPreflightResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::PathBuf::from(&project.path);
    let resolution = crate::content::locator::resolve(&repo_root, project.content_dir.as_deref());
    let content_dir = resolution
        .selected
        .ok_or_else(|| "Content directory not found".to_string())?;
    let all_articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    let candidates: Vec<_> = all_articles
        .iter()
        .filter(|a| article_ids.contains(&a.id))
        .cloned()
        .collect();
    Ok(crate::content::publish::preflight(&candidates, &all_articles, &content_dir))
}

#[tauri::command]
pub fn apply_publish_articles(
    state: State<'_, AppState>,
    project_id: String,
    article_ids: Vec<i64>,
    date_fixes: std::collections::HashMap<String, String>,
    year_resolutions: Vec<crate::content::publish::YearMismatchResolution>,
) -> Result<crate::content::publish::PublishResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::PathBuf::from(&project.path);
    let resolution = crate::content::locator::resolve(&repo_root, project.content_dir.as_deref());
    let content_dir = resolution
        .selected
        .ok_or_else(|| "Content directory not found".to_string())?;
    Ok(crate::content::publish::apply_publish(
        &db,
        &project_id,
        &article_ids,
        &date_fixes,
        &year_resolutions,
        &content_dir,
        &repo_root,
    ))
}

#[tauri::command]
pub fn resolve_year_mismatch_agent(
    state: State<'_, AppState>,
    project_id: String,
    article_id: i64,
    title: String,
    title_year: i32,
    publish_year: i32,
) -> Result<crate::content::publish::YearMismatchResolution, String> {
    use crate::db::global_settings;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::PathBuf::from(&project.path);
    
    // Agent provider is global (user preference), check for legacy project setting
    let provider = if let Some(legacy) = &project.agent_provider {
        legacy.as_str()
    } else {
        &global_settings::get_agent_provider(&db)
    };
    
    let all_articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    crate::content::publish::resolve_year_mismatch_with_agent(
        provider,
        article_id,
        &title,
        title_year,
        publish_year,
        &repo_root,
        &all_articles,
    )
}

#[tauri::command]
pub fn get_keyword_coverage(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let (exists, last_analyzed) = crate::engine::exec::coverage::get_coverage_status(&project.path);
    
    let coverage = if exists {
        crate::engine::exec::coverage::read_keyword_coverage(&project.path)
    } else {
        None
    };
    
    Ok(serde_json::json!({
        "exists": exists,
        "last_analyzed": last_analyzed,
        "coverage": coverage,
    }))
}

#[tauri::command]
pub fn analyze_article_readability(
    state: State<'_, AppState>,
    project_id: String,
    slug: String,
) -> Result<crate::content::readability::ReadabilityReport, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::PathBuf::from(&project.path);
    let resolution = crate::content::locator::resolve(&repo_root, project.content_dir.as_deref());
    let content_dir = resolution
        .selected
        .ok_or_else(|| "Content directory not found".to_string())?;
    
    // Find the article file by slug
    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    let article = articles
        .iter()
        .find(|a| a.url_slug == slug)
        .ok_or_else(|| format!("Article with slug '{}' not found", slug))?;
    
    // Read the article file
    let file_path = content_dir.join(&article.file);
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read article file: {}", e))?;
    
    // Clean the content for readability analysis
    let cleaned = crate::content::readability::clean_mdx_for_readability(&content);
    
    // Analyze readability
    crate::content::readability::analyze_readability(&cleaned)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn compare_competitor_content(
    _state: State<'_, AppState>,
    keyword: String,
    competitor_urls: Vec<String>,
    user_url: Option<String>,
) -> Result<crate::content::competitor::WordCountComparison, String> {
    crate::content::competitor::compare_word_counts(
        &keyword,
        &competitor_urls,
        user_url.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn analyze_keyword_density(
    state: State<'_, AppState>,
    project_id: String,
    slug: String,
    target_keyword: String,
) -> Result<crate::content::keyword_density::KeywordDensityReport, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::PathBuf::from(&project.path);
    let resolution = crate::content::locator::resolve(&repo_root, project.content_dir.as_deref());
    let content_dir = resolution
        .selected
        .ok_or_else(|| "Content directory not found".to_string())?;
    
    // Find the article file by slug
    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    let article = articles
        .iter()
        .find(|a| a.url_slug == slug)
        .ok_or_else(|| format!("Article with slug '{}' not found", slug))?;
    
    // Read the article file
    let file_path = content_dir.join(&article.file);
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read article file: {}", e))?;
    
    // Strip frontmatter to get body content
    let (_, body) = crate::engine::exec::utils::parse_frontmatter(&content);
    
    // Analyze keyword density
    Ok(crate::content::keyword_density::analyze_keyword_density(&body, &target_keyword))
}
