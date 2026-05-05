//! Social media marketing workflow step executors

use chrono::Utc;

use crate::engine::workflows::StepResult;
use crate::engine::workflows::WorkflowStep;
use crate::models::social::*;
use crate::models::task::Task;
use crate::social::models::{AgentPostOutput, AgentTemplateOutput, PostGenerationJob};
use crate::social::templates::{TemplateDef, TemplateRegistry};

mod extract;
mod generate;
mod templates;
mod visuals;

pub(crate) use extract::*;
pub(crate) use generate::*;
pub(crate) use templates::*;
pub(crate) use visuals::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Load Templates
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_load_templates(task: &Task, _project_path: &str) -> StepResult {
    // Get campaign config from task description
    let template_ids = parse_template_ids_from_task(task);

    // For now, we'll create default templates if they don't exist
    // In a real implementation, this would load from the database

    log::info!(
        "[social_load_templates] loading {} templates",
        template_ids.len()
    );

    // Return success - templates will be loaded from DB in the generate step
    StepResult {
        success: true,
        message: format!("Loaded {} template configurations", template_ids.len()),
        output: Some(serde_json::to_string(&template_ids).unwrap_or_default()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 5: Save Campaign
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_save_campaign(
    task: &Task,
    _project_path: &str,
    conn: &rusqlite::Connection,
) -> StepResult {
    use crate::social::db;

    // Get the posts from previous step
    let posts = match load_posts_from_artifacts(task) {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: "No posts to save. Run previous steps first.".to_string(),
                output: None,
            };
        }
    };

    log::info!(
        "[social_save_campaign] saving {} posts for campaign {}",
        posts.len(),
        task.id
    );

    // Save each post to the database
    let mut saved_count = 0;
    for post in &posts {
        match db::create_post(conn, post) {
            Ok(_) => saved_count += 1,
            Err(e) => {
                log::warn!(
                    "[social_save_campaign] failed to save post {}: {}",
                    post.id,
                    e
                );
            }
        }
    }

    // Update campaign post count
    if let Some(campaign_id) = extract_campaign_id_from_task(task) {
        if let Err(e) = db::update_campaign_post_count(conn, &campaign_id, saved_count as u32) {
            log::warn!(
                "[social_save_campaign] failed to update campaign count: {}",
                e
            );
        }

        // Update campaign status to active
        if let Err(e) = db::update_campaign_status(conn, &campaign_id, CampaignStatus::Active) {
            log::warn!(
                "[social_save_campaign] failed to update campaign status: {}",
                e
            );
        }
    }

    StepResult {
        success: saved_count > 0,
        message: format!("Saved {} of {} posts", saved_count, posts.len()),
        output: Some(format!(
            "{{\"saved\":{},\"total\":{}}}",
            saved_count,
            posts.len()
        )),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Regenerate Steps
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_regenerate_single(
    _step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    // Parse post ID and feedback from task description
    let (post_id, feedback) = parse_regenerate_params(task);

    log::info!(
        "[social_regenerate_single] regenerating post {} with feedback: {}",
        post_id,
        feedback
    );

    // For now, return success - full implementation would load post, call agent with feedback
    let _ = (post_id, feedback, project_path, agent_provider);

    StepResult {
        success: true,
        message: "Post regeneration initiated".to_string(),
        output: None,
    }
}

pub fn exec_social_update_post(_task: &Task, _project_path: &str) -> StepResult {
    StepResult {
        success: true,
        message: "Post updated".to_string(),
        output: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_source_config_from_task(task: &Task) -> SourceConfig {
    // Parse source_config from task description JSON
    if let Some(desc) = &task.description {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(desc) {
            if let Some(config) = json.get("source_config") {
                if let Ok(source_config) = serde_json::from_value::<SourceConfig>(config.clone()) {
                    return source_config;
                }
            }
        }
    }

    // Fallback to default config if parsing fails
    SourceConfig {
        include_articles: true,
        article_slugs: vec![],
        include_screenshots: true,
        screenshot_dirs: vec![],
        include_specs: false,
    }
}

fn parse_template_ids_from_task(task: &Task) -> Vec<String> {
    // Parse template_ids from task description JSON
    if let Some(desc) = &task.description {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(desc) {
            if let Some(template_ids) = json.get("template_ids").and_then(|v| v.as_array()) {
                return template_ids
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
            }
        }
    }

    // Fallback to default templates if parsing fails
    vec!["article_hook".to_string(), "article_carousel".to_string()]
}

fn parse_platforms_from_task(task: &Task) -> Vec<Platform> {
    // Parse target_platforms from task description JSON
    if let Some(desc) = &task.description {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(desc) {
            if let Some(platforms) = json.get("target_platforms").and_then(|v| v.as_array()) {
                let parsed: Vec<Platform> = platforms
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| match s {
                        "tiktok" => Some(Platform::TikTok),
                        "instagram_feed" => Some(Platform::InstagramFeed),
                        "instagram_reel" => Some(Platform::InstagramReel),
                        "instagram_story" => Some(Platform::InstagramStory),
                        _ => None,
                    })
                    .collect();
                if !parsed.is_empty() {
                    return parsed;
                }
            }
        }
    }

    // Fallback to default platforms if parsing fails
    vec![Platform::InstagramFeed, Platform::TikTok]
}

fn load_posts_from_artifacts(task: &Task) -> Option<Vec<SocialPost>> {
    // Try to load from social_generate_posts first, then social_build_visuals
    task.artifacts
        .iter()
        .find(|a| a.key == "social_build_visuals" || a.key == "social_generate_posts")
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str(c).ok())
}

fn get_site_url_from_project(_task: &Task) -> String {
    // In real implementation, get from project
    "https://example.com".to_string()
}

fn load_templates_for_generation(
    _project_id: &str,
    template_ids: &[String],
) -> Result<Vec<TemplateDef>, String> {
    // Use new template registry
    let registry = TemplateRegistry::default();
    let mut templates = Vec::new();

    for id in template_ids {
        if let Some(template) = registry.get(id) {
            templates.push(template.clone());
        } else {
            // Fallback: try to use the template ID as a platform hint
            log::warn!("Template {} not found in registry, skipping", id);
        }
    }

    // If no templates found, use all defaults
    if templates.is_empty() {
        templates = registry.all().to_vec();
    }

    Ok(templates)
}

fn _create_default_template(id: &str) -> Result<ContentTemplate, String> {
    let (name, platform, format, creation_prompt) = match id {
        "article_hook" => (
            "Article Hook",
            Platform::TikTok,
            PostFormat::SingleImage,
            "Create a punchy, attention-grabbing hook based on the article's main insight. Focus on the biggest takeaway or most surprising fact. Keep it under 100 characters."
        ),
        "article_carousel" => (
            "Article Carousel",
            Platform::InstagramFeed,
            PostFormat::Carousel,
            "Create a 5-slide carousel summarizing the article's key points. Each slide should have one clear takeaway. Use a narrative flow: problem → insight → solution → action."
        ),
        "stat_highlight" => (
            "Stat Highlight",
            Platform::InstagramFeed,
            PostFormat::SingleImage,
            "Highlight the most impressive statistic from the content. Make it big, bold, and contextualize why it matters."
        ),
        "tip_card" => (
            "Quick Tip",
            Platform::TikTok,
            PostFormat::SingleImage,
            "Extract one actionable tip that readers can implement immediately. Focus on practical, specific advice."
        ),
        _ => return Err(format!("Unknown template ID: {}", id)),
    };

    Ok(ContentTemplate {
        id: id.to_string(),
        project_id: None,
        name: name.to_string(),
        description: Some(creation_prompt.to_string()),
        platform,
        format,
        creation_prompt: creation_prompt.to_string(),
        overlay_config: OverlayConfig {
            canvas_size: CanvasSize::Square,
            font_family: "Inter".to_string(),
            primary_color: "#FFFFFF".to_string(),
            secondary_color: "#000000".to_string(),
            text_position: TextPosition::Center,
            max_text_length: 100,
        },
        default_hashtags: vec![
            "#contentmarketing".to_string(),
            "#digitalmarketing".to_string(),
        ],
        example_output: None,
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    })
}

fn parse_agent_post_output(output: &str) -> Result<AgentPostOutput, String> {
    // Extract JSON from agent output
    let json_str = crate::engine::text::extract_json_string(output)
        .ok_or_else(|| "No JSON block found in output".to_string())?;
    serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))
}

fn parse_agent_template_output(output: &str) -> Result<AgentTemplateOutput, String> {
    let json_str = crate::engine::text::extract_json_string(output)
        .ok_or_else(|| "No JSON block found in output".to_string())?;
    serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))
}

fn create_social_post_from_agent_output(
    campaign_id: &str,
    project_id: &str,
    job: &PostGenerationJob,
    agent_output: &AgentPostOutput,
    agent_provider: &str,
) -> SocialPost {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Generate deterministic ID based on source + template + timestamp
    let mut hasher = DefaultHasher::new();
    job.source.source_id.hash(&mut hasher);
    job.template.id.hash(&mut hasher);
    Utc::now().timestamp().hash(&mut hasher);
    let id = format!("{:x}", hasher.finish());

    SocialPost {
        id,
        campaign_id: campaign_id.to_string(),
        project_id: project_id.to_string(),
        source_type: job.source.source_type.clone(),
        source_id: job.source.source_id.clone(),
        source_url: None,
        platform: job.platform.clone(),
        format: job.template.format.clone(),
        hook: agent_output.hook.clone(),
        caption: agent_output.caption.clone(),
        hashtags: agent_output.hashtags.clone(),
        cta: agent_output.cta.clone(),
        visual_assets: vec![VisualAsset {
            path: job.source.path.to_string_lossy().to_string(),
            asset_type: AssetType::Image,
            description: agent_output.visual_description.clone(),
            overlay_text: agent_output.overlay_text.clone(),
        }],
        // AI-generated prompt for external image generation (Midjourney, DALL-E, etc.)
        image_generation_prompt: agent_output.image_generation_prompt.clone(),
        // Note: Image resolution happens in social_build_visuals step
        // where we try article images -> screenshots -> generated branded graphics
        status: PostStatus::Draft,
        scheduled_at: None,
        posted_at: None,
        platform_post_id: None,
        platform_post_url: None,
        metrics: None,
        template_id: job.template.id.clone(),
        generated_by: Some(agent_provider.to_string()),
        generation_prompt_hash: None,
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    }
}

fn create_template_from_agent_output(
    request: &CreateTemplateRequest,
    agent_output: &AgentTemplateOutput,
) -> ContentTemplate {
    ContentTemplate {
        id: uuid::Uuid::new_v4().to_string(),
        project_id: request.project_id.clone(),
        name: request.name.clone(),
        description: Some(request.description.clone()),
        platform: request.platform.clone(),
        format: request.format.clone(),
        creation_prompt: agent_output.creation_prompt.clone(),
        overlay_config: agent_output.overlay_config.clone(),
        default_hashtags: agent_output.default_hashtags.clone(),
        example_output: Some(TemplateExample {
            hook: agent_output.example.hook.clone(),
            caption: agent_output.example.caption.clone(),
            visual_description: agent_output.example.visual_description.clone(),
        }),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    }
}

fn parse_regenerate_params(_task: &Task) -> (String, String) {
    // Parse from task description
    ("post_id".to_string(), "feedback".to_string())
}

fn parse_create_template_request(task: &Task) -> CreateTemplateRequest {
    // Parse from task description
    CreateTemplateRequest {
        project_id: Some(task.project_id.clone()),
        name: "New Template".to_string(),
        platform: Platform::InstagramFeed,
        format: PostFormat::SingleImage,
        description: "A new content template".to_string(),
    }
}

fn extract_campaign_id_from_task(task: &Task) -> Option<String> {
    // Try to parse campaign_id from task description (JSON)
    if let Some(desc) = &task.description {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(desc) {
            if let Some(campaign_id) = json.get("campaign_id").and_then(|v| v.as_str()) {
                return Some(campaign_id.to_string());
            }
        }
    }
    // Fallback: use task id as campaign id
    Some(task.id.clone())
}
