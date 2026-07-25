//! Simplified social media post generation
//!
//! Flow: Articles → Agent → Social Posts

use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::engine::workflows::WorkflowStep;
use crate::models::social::*;
use crate::models::task::Task;
use crate::social::db;
use crate::social::generator::generate_posts_from_articles;
use crate::content::ops::list_articles;
use crate::engine::project_paths::ProjectPaths;

/// Step 1: Generate posts from articles (Agentic)
pub async fn exec_social_generate_posts(
    _step: &WorkflowStep,
    task: &Task,
    _project_path: &str,
    agent_provider: &str,
) -> StepResult {
    // Parse campaign config from task description
    let (campaign_id, platforms) = parse_campaign_config(task);
    
    log::info!("[social_generate_posts] generating posts for campaign {}", campaign_id);
    
    // Get articles from the project
    let articles = match list_articles(&task.project_id) {
        Ok(a) => a,
        Err(e) => {
            return StepResult::fail(format!("Failed to load articles: {}", e));
        }
    };
    
    if articles.is_empty() {
        return StepResult::fail("No articles found. Add articles to your project first.".to_string());
    }
    
    log::info!("[social_generate_posts] found {} articles", articles.len());
    
    // Get project paths for image generation
    let paths = ProjectPaths::from_path(_project_path);
    let output_dir = paths.social_output_dir();
    if let Err(e) = std::fs::create_dir_all(&output_dir) {
        return StepResult::fail(format!("Failed to create output directory: {}", e));
    }
    
    // Generate posts agentically
    let posts = match generate_posts_from_articles(
        &campaign_id,
        &task.project_id,
        &articles,
        &platforms,
        agent_provider,
        Path::new(_project_path),
        &output_dir,
    ).await {
        Ok(p) => p,
        Err(e) => {
            return StepResult::fail(format!("Failed to generate posts: {}", e));
        }
    };
    
    if posts.is_empty() {
        return StepResult::fail("No posts were generated. Check agent output.".to_string());
    }
    
    // Save posts to database
    for post in &posts {
        if let Err(e) = db::create_post(&crate::get_db_conn(), post) {
            log::warn!("Failed to save post {}: {}", post.id, e);
        }
    }
    
    // Update campaign status
    if let Err(e) = db::update_campaign_status(
        &crate::get_db_conn(),
        &campaign_id,
        CampaignStatus::Active
    ) {
        log::warn!("Failed to update campaign status: {}", e);
    }
    
    let posts_json = match serde_json::to_string(&posts) {
        Ok(j) => j,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize posts: {}", e));
        }
    };
    
    StepResult {
        success: true,
        message: format!("Generated {} social media posts from {} articles", posts.len(), articles.len()),
        output: Some(posts_json),
        artifact_key: None,
    }
}

/// Parse campaign configuration from task description
fn parse_campaign_config(task: &Task) -> (String, Vec<Platform>) {
    let mut campaign_id = task.id.clone();
    let mut platforms = vec![Platform::InstagramFeed, Platform::TikTok];
    
    if let Some(desc) = &task.description {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(desc) {
            if let Some(cid) = json.get("campaign_id").and_then(|v| v.as_str()) {
                campaign_id = cid.to_string();
            }
            if let Some(plat) = json.get("target_platforms").and_then(|v| v.as_array()) {
                platforms = plat.iter()
                    .filter_map(|p| p.as_str())
                    .filter_map(|p| match p {
                        "tiktok" => Some(Platform::TikTok),
                        "instagram_feed" => Some(Platform::InstagramFeed),
                        "instagram_reel" => Some(Platform::InstagramReel),
                        "instagram_story" => Some(Platform::InstagramStory),
                        _ => None,
                    })
                    .collect();
            }
        }
    }
    
    (campaign_id, platforms)
}

fn get_db_conn() -> rusqlite::Connection {
    // This is a placeholder - in real implementation, get from app state
    rusqlite::Connection::open_in_memory().unwrap()
}
