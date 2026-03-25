//! Tauri commands for social media marketing

use tauri::State;

use crate::commands::AppState;
use crate::models::social::*;
use crate::social::db;

/// List all campaigns for a project
#[tauri::command]
pub fn list_social_campaigns(
    state: State<'_, AppState>,
    project_id: String,
) -> std::result::Result<Vec<SocialCampaign>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::list_campaigns(&conn, &project_id).map_err(|e| e.to_string())
}

/// Get a single campaign by ID
#[tauri::command]
pub fn get_social_campaign(
    state: State<'_, AppState>,
    campaign_id: String,
) -> std::result::Result<Option<SocialCampaign>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::get_campaign(&conn, &campaign_id).map_err(|e| e.to_string())
}

/// Create a new campaign
#[tauri::command]
pub fn create_social_campaign(
    state: State<'_, AppState>,
    req: CreateCampaignRequest,
) -> std::result::Result<SocialCampaign, String> {
    use chrono::Utc;
    
    let campaign = SocialCampaign {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: req.project_id,
        name: req.name,
        description: req.description,
        source_config: req.source_config,
        target_platforms: req.target_platforms,
        template_ids: req.template_ids,
        status: CampaignStatus::Draft,
        post_count: 0,
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    };
    
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::create_campaign(&conn, &campaign).map_err(|e| e.to_string())?;
    
    Ok(campaign)
}

/// Delete a campaign
#[tauri::command]
pub fn delete_social_campaign(
    state: State<'_, AppState>,
    campaign_id: String,
) -> std::result::Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_campaign(&conn, &campaign_id).map_err(|e| e.to_string())
}

/// Get all posts for a campaign
#[tauri::command]
pub fn get_campaign_posts(
    state: State<'_, AppState>,
    campaign_id: String,
    status: Option<PostStatus>,
) -> std::result::Result<Vec<SocialPost>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::get_posts_by_campaign(&conn, &campaign_id, status).map_err(|e| e.to_string())
}

/// Get a single post by ID
#[tauri::command]
pub fn get_social_post(
    state: State<'_, AppState>,
    post_id: String,
) -> std::result::Result<Option<SocialPost>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::get_post(&conn, &post_id).map_err(|e| e.to_string())
}

/// Update post status
#[tauri::command]
pub fn update_social_post_status(
    state: State<'_, AppState>,
    post_id: String,
    status: PostStatus,
) -> std::result::Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::update_post_status(&conn, &post_id, status).map_err(|e| e.to_string())
}

/// Update post content
#[tauri::command]
pub fn update_social_post(
    state: State<'_, AppState>,
    post: SocialPost,
) -> std::result::Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::update_post(&conn, &post).map_err(|e| e.to_string())
}

/// Schedule a post
#[tauri::command]
pub fn schedule_social_post(
    state: State<'_, AppState>,
    post_id: String,
    scheduled_at: String,
) -> std::result::Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::schedule_post(&conn, &post_id, &scheduled_at).map_err(|e| e.to_string())
}

/// Mark a post as posted
#[tauri::command]
pub fn mark_social_post_posted(
    state: State<'_, AppState>,
    post_id: String,
    platform_url: String,
) -> std::result::Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::mark_posted(&conn, &post_id, &platform_url).map_err(|e| e.to_string())
}

/// Delete a post
#[tauri::command]
pub fn delete_social_post(
    state: State<'_, AppState>,
    post_id: String,
) -> std::result::Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_post(&conn, &post_id).map_err(|e| e.to_string())
}

/// List available templates
#[tauri::command]
pub fn list_social_templates(
    state: State<'_, AppState>,
    project_id: String,
) -> std::result::Result<Vec<ContentTemplate>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::list_templates(&conn, Some(&project_id)).map_err(|e| e.to_string())
}

/// Get a single template by ID
#[tauri::command]
pub fn get_social_template(
    state: State<'_, AppState>,
    template_id: String,
) -> std::result::Result<Option<ContentTemplate>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::get_template(&conn, &template_id).map_err(|e| e.to_string())
}

/// Create a new template
#[tauri::command]
pub fn create_social_template(
    state: State<'_, AppState>,
    template: ContentTemplate,
) -> std::result::Result<ContentTemplate, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::create_template(&conn, &template).map_err(|e| e.to_string())?;
    Ok(template)
}

/// Delete a template
#[tauri::command]
pub fn delete_social_template(
    state: State<'_, AppState>,
    template_id: String,
) -> std::result::Result<(), String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_template(&conn, &template_id).map_err(|e| e.to_string())
}

/// Get campaign statistics
#[tauri::command]
pub fn get_social_campaign_stats(
    state: State<'_, AppState>,
    campaign_id: String,
) -> std::result::Result<CampaignStats, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::get_campaign_stats(&conn, &campaign_id).map_err(|e| e.to_string())
}

/// Get posts by project (across all campaigns)
#[tauri::command]
pub fn get_social_posts_by_project(
    state: State<'_, AppState>,
    project_id: String,
    status: Option<PostStatus>,
) -> std::result::Result<Vec<SocialPost>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::get_posts_by_project(&conn, &project_id, status).map_err(|e| e.to_string())
}
