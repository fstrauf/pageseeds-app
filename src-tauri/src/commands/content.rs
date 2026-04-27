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
        let project_path = std::path::PathBuf::from(&project.path);
        crate::content::dates::apply_fixes_to_db_and_export(
            &db, &project_id, &project_path, &fix_result.fixes,
        ).map_err(|e| e.to_string())?;
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
    
    let provider = global_settings::resolve_agent_provider(&db, project.agent_provider.as_deref());
    
    let all_articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    crate::content::publish::resolve_year_mismatch_with_agent(
        &provider,
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
    let repo_root = std::path::Path::new(&project.path);
    crate::content::ops::analyze_article_readability(
        &db, &project_id, repo_root, project.content_dir.as_deref(), &slug,
    )
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
pub fn clean_stale_articles(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    let automation_dir = repo_root.join(".github").join("automation");
    crate::content::ops::clean_stale_articles_json(&automation_dir, repo_root)
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
    let repo_root = std::path::Path::new(&project.path);
    crate::content::ops::analyze_keyword_density(
        &db, &project_id, repo_root, project.content_dir.as_deref(), &slug, &target_keyword,
    )
    .map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Content format validator & cleanup
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn validate_content_format(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::content::validator::FormatValidationResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    let automation_dir = repo_root.join(".github").join("automation");

    // Load workspace config to check for schema override
    let schema = crate::engine::setup_check::load_workspace_config(&automation_dir)
        .and_then(|cfg| cfg.frontmatter_schema);

    let content_dir = crate::content::ops::resolve_content_dir(&automation_dir, repo_root)
        .map_err(|e| e.to_string())?;

    crate::content::validator::validate_project(repo_root, &content_dir, schema.as_ref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn fix_content_format(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::content::validator::FormatFixResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    let automation_dir = repo_root.join(".github").join("automation");

    // Load workspace config to check for schema override
    let schema = crate::engine::setup_check::load_workspace_config(&automation_dir)
        .and_then(|cfg| cfg.frontmatter_schema);

    let content_dir = crate::content::ops::resolve_content_dir(&automation_dir, repo_root)
        .map_err(|e| e.to_string())?;

    let validation = crate::content::validator::validate_project(repo_root, &content_dir, schema.as_ref())
        .map_err(|e| e.to_string())?;

    crate::content::validator::apply_fixes(&validation.issues, repo_root)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn repair_article_paths(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::models::article::RepairPathResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);

    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;
    let mut checked = 0usize;
    let mut repaired = 0usize;
    let mut removed = 0usize;
    let mut not_found = Vec::new();

    let content_dirs = crate::content::article_resolver::discover_content_dirs(repo_root);
    let content_dirs_refs: Vec<&str> = content_dirs.iter().map(|s| s.as_str()).collect();

    for article in &articles {
        checked += 1;
        let resolved = crate::content::article_resolver::resolve_article_file(
            repo_root, &article.file, &content_dirs_refs,
        );
        if resolved.found && resolved.was_repaired {
            // Update DB path
            let _ = db.execute(
                "UPDATE articles SET file = ?1 WHERE id = ?2",
                rusqlite::params![&resolved.relative_path, article.id],
            );
            repaired += 1;
        } else if !resolved.found {
            not_found.push(article.file.clone());
            removed += 1;
        }
    }

    // Re-export articles.json so changes are persisted to repo
    let _ = crate::db::export::export_articles(&db, &project_id);

    Ok(crate::models::article::RepairPathResult {
        checked,
        repaired,
        removed,
        not_found,
    })
}

#[tauri::command]
pub fn get_ctr_health_summary(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<crate::models::ctr::CtrHealthSummary, String> {
    use crate::models::ctr::{CtrHealthSummary, CtrHealthArticle};
    use crate::engine::exec::audit_health;

    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);

    let articles = task_store::list_articles(&db, &project_id).map_err(|e| e.to_string())?;

    // Count CTR-related tasks
    let all_tasks = task_store::list_tasks_light(&db, &project_id).map_err(|e| e.to_string())?;
    let pending_fix_tasks = all_tasks.iter().filter(|t| t.task_type == "fix_ctr_article" && t.status == crate::models::task::TaskStatus::Todo).count();
    let completed_audits = all_tasks.iter().filter(|t| t.task_type == "ctr_audit" && t.status == crate::models::task::TaskStatus::Done).count();

    let mut health_articles = Vec::new();
    let mut missing_files = 0usize;
    let mut title_issues = 0usize;
    let mut meta_issues = 0usize;
    let mut snippet_issues = 0usize;
    let mut faq_issues = 0usize;

    for article in &articles {
        let file_found = audit_health::resolve_content_file(repo_root, &article.file).is_some();
        if !file_found {
            missing_files += 1;
            health_articles.push(CtrHealthArticle {
                id: article.id,
                title: article.title.clone(),
                url_slug: article.url_slug.clone(),
                file: article.file.clone(),
                healthy: false,
                audit_status: "needs_fix".to_string(),
                issues: vec!["file_not_found".to_string()],
                last_audited_at: None,
                last_audit_issues: Vec::new(),
                resolved_issues: Vec::new(),
            });
            continue;
        }

        let (title, meta, first_paragraph, _h1, has_faq, _found) =
            audit_health::read_article_excerpt(&project.path, &article.file);

        let health = audit_health::check_article_health(
            &title,
            &meta,
            &first_paragraph,
            article.target_keyword.as_deref().unwrap_or(""),
            has_faq,
            true,
        );

        let mut issues = Vec::new();
        if title.len() > audit_health::TITLE_MAX_LEN {
            issues.push("title_too_long".to_string());
            title_issues += 1;
        }
        if meta.len() < audit_health::META_MIN_LEN || meta.len() > audit_health::META_MAX_LEN {
            issues.push("meta_too_short".to_string());
            meta_issues += 1;
        }
        let word_count = first_paragraph.split_whitespace().count();
        let has_kw_or_q = article.target_keyword.as_deref().unwrap_or("").is_empty()
            || first_paragraph.to_lowercase().contains(&article.target_keyword.as_deref().unwrap_or("").to_lowercase())
            || first_paragraph.contains('?');
        if word_count < audit_health::SNIPPET_MIN_WORDS
            || word_count > audit_health::SNIPPET_MAX_WORDS
            || !has_kw_or_q
        {
            issues.push("snippet_suboptimal".to_string());
            snippet_issues += 1;
        }
        if !has_faq {
            issues.push("missing_faq_schema".to_string());
            faq_issues += 1;
        }

        let healthy = issues.is_empty();
        let audit_status = if healthy {
            "healthy".to_string()
        } else {
            "needs_fix".to_string()
        };

        health_articles.push(CtrHealthArticle {
            id: article.id,
            title: article.title.clone(),
            url_slug: article.url_slug.clone(),
            file: article.file.clone(),
            healthy,
            audit_status,
            issues: issues.clone(),
            last_audited_at: None,
            last_audit_issues: Vec::new(),
            resolved_issues: Vec::new(),
        });
    }

    let total_articles = health_articles.len();
    let healthy_count = health_articles.iter().filter(|a| a.healthy).count();
    let unhealthy_count = total_articles - healthy_count;
    let open_issues_count = health_articles.iter().map(|a| a.issues.len()).sum();

    Ok(CtrHealthSummary {
        total_articles,
        healthy_count,
        unhealthy_count,
        improved_count: 0,
        already_healthy_count: healthy_count,
        regressed_count: 0,
        missing_files,
        title_issues,
        meta_issues,
        snippet_issues,
        faq_issues,
        last_audit_at: None,
        articles: health_articles,
        pending_fix_tasks,
        completed_audits,
        open_issues_count,
    })
}
