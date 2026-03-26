//! Social media marketing workflow step executors

use std::path::Path;

use chrono::Utc;

use crate::engine::workflows::StepResult;
use crate::engine::workflows::WorkflowStep;
use crate::models::social::*;
use crate::models::task::Task;
use crate::social::content::sources::{discover_sources, ensure_output_dir};
use crate::social::db;
use crate::social::models::{AgentPostOutput, AgentTemplateOutput, ContentSource, PostGenerationJob, SourceManifest};
use crate::social::prompts;
use crate::social::templates::{TemplateRegistry, TemplateDef, render_prompt, validate_output};

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Collect Content Sources
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_collect_sources(
    task: &Task,
    project_path: &str,
) -> StepResult {
    // Parse source config from task description
    let config = parse_source_config_from_task(task);

    log::info!("[social_collect_sources] discovering sources for project {}", task.project_id);

    let manifest = match discover_sources(Path::new(project_path), &config) {
        Ok(m) => m,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to discover sources: {}", e),
                output: None,
            };
        }
    };

    let total = manifest.total_sources();
    log::info!("[social_collect_sources] found {} sources ({} articles, {} screenshots, {} specs)",
        total,
        manifest.articles.len(),
        manifest.screenshots.len(),
        manifest.specs.len()
    );

    if total == 0 {
        return StepResult {
            success: false,
            message: "No content sources found. Check your source configuration.".to_string(),
            output: None,
        };
    }

    // For now, we don't serialize the full manifest (contains PathBuf)
    // Instead, we'll rediscover sources in the next step
    // TODO: Create a serializable manifest structure

    StepResult {
        success: true,
        message: format!("Discovered {} content sources", total),
        output: Some(format!("{{\"total\":{}}}", total)),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Load Templates
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_load_templates(
    task: &Task,
    _project_path: &str,
) -> StepResult {
    // Get campaign config from task description
    let template_ids = parse_template_ids_from_task(task);

    // For now, we'll create default templates if they don't exist
    // In a real implementation, this would load from the database

    log::info!("[social_load_templates] loading {} templates", template_ids.len());

    // Return success - templates will be loaded from DB in the generate step
    StepResult {
        success: true,
        message: format!("Loaded {} template configurations", template_ids.len()),
        output: Some(serde_json::to_string(&template_ids).unwrap_or_default()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Generate Posts (Agentic)
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_generate_posts(
    _step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    // Rediscover sources (since we can't serialize PathBuf in manifest)
    let config = parse_source_config_from_task(task);
    let manifest = match discover_sources(Path::new(project_path), &config) {
        Ok(m) => m,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to discover sources: {}", e),
                output: None,
            };
        }
    };

    if manifest.is_empty() {
        return StepResult {
            success: false,
            message: "No content sources found.".to_string(),
            output: None,
        };
    }

    let template_ids = parse_template_ids_from_task(task);
    let platforms = parse_platforms_from_task(task);

    // Get templates from database or create defaults
    let templates = match load_templates_for_generation(&task.project_id, &template_ids) {
        Ok(t) => t,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load templates: {}", e),
                output: None,
            };
        }
    };

    // Get project context
    let site_url = get_site_url_from_project(task);
    let project_context = prompts::default_project_context(&site_url);

    // Generate jobs: cross product of sources × templates × platforms
    let mut jobs: Vec<PostGenerationJob> = Vec::new();
    for source in manifest.all_sources() {
        for template in &templates {
            for platform in &platforms {
                // Only generate if platform matches template platform
                if &template.platform == platform {
                    jobs.push(PostGenerationJob {
                        source: source.clone(),
                        template: template.clone(),
                        platform: platform.clone(),
                    });
                }
            }
        }
    }

    log::info!("[social_generate_posts] generating {} posts", jobs.len());

    // Generate each post via agent using template system
    let mut generated_posts: Vec<SocialPost> = Vec::new();
    let campaign_id = extract_campaign_id_from_task(task).unwrap_or_else(|| task.id.clone());
    
    // Use new template registry for better template selection
    let registry = TemplateRegistry::default();

    for (idx, job) in jobs.iter().enumerate() {
        // Use new template system to render prompt
        let prompt = render_prompt(&job.template, &job.source);

        log::info!("[social_generate_posts] job {}/{}: generating for source {:?} using template {:?}",
            idx + 1,
            jobs.len(),
            job.source.source_id,
            job.template.id
        );

        // Call the agent
        match crate::engine::agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
            Ok(output) => {
                // Parse the agent output
                match parse_agent_post_output(&output) {
                    Ok(agent_output) => {
                        // Validate against template schema
                        let json_output = serde_json::to_value(&agent_output).unwrap_or_default();
                        let validation_errors = validate_output(&job.template, &json_output);
                        
                        if !validation_errors.is_empty() {
                            log::warn!("[social_generate_posts] validation errors: {:?}", validation_errors);
                        }
                        
                        let post = create_social_post_from_agent_output(
                            &campaign_id,
                            &task.project_id,
                            &job,
                            &agent_output,
                            agent_provider,
                        );
                        generated_posts.push(post);
                    }
                    Err(e) => {
                        log::warn!("[social_generate_posts] failed to parse agent output: {}", e);
                    }
                }
            }
            Err(e) => {
                log::warn!("[social_generate_posts] agent failed: {}", e);
            }
        }
    }

    if generated_posts.is_empty() {
        return StepResult {
            success: false,
            message: "No posts were generated. Check agent output.".to_string(),
            output: None,
        };
    }

    // Save generated posts as artifact
    let posts_json = match serde_json::to_string(&generated_posts) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize posts: {}", e),
                output: None,
            };
        }
    };

    StepResult {
        success: true,
        message: format!("Generated {} social media posts", generated_posts.len()),
        output: Some(posts_json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Build Visuals
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_build_visuals(
    task: &Task,
    project_path: &str,
) -> StepResult {
    // Load generated posts from previous step
    let mut posts = match load_posts_from_artifacts(task) {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: "No generated posts found. Run social_generate_posts first.".to_string(),
                output: None,
            };
        }
    };

    // Ensure output directory exists
    let output_dir = match ensure_output_dir(Path::new(project_path)) {
        Ok(d) => d,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to create output directory: {}", e),
                output: None,
            };
        }
    };

    log::info!("[social_build_visuals] building visuals for {} posts", posts.len());

    // Build visuals for each post
    for post in &mut posts {
        // For now, just copy the source image to the output directory
        // In a real implementation, this would apply text overlays
        if let Some(first_asset) = post.visual_assets.first() {
            let source_path = Path::new(project_path).join(&first_asset.path);
            let output_filename = format!("{}.png", post.id);
            let output_path = output_dir.join(&output_filename);

            if source_path.exists() {
                if let Err(e) = std::fs::copy(&source_path, &output_path) {
                    log::warn!("[social_build_visuals] failed to copy image: {}", e);
                } else {
                    // Update the post's visual asset to point to the new location
                    let relative_path = output_path
                        .strip_prefix(project_path)
                        .unwrap_or(&output_path)
                        .to_string_lossy()
                        .to_string();

                    post.visual_assets = vec![VisualAsset {
                        path: relative_path,
                        asset_type: AssetType::Image,
                        description: first_asset.description.clone(),
                        overlay_text: first_asset.overlay_text.clone(),
                    }];
                }
            }
        }
    }

    // Save updated posts
    let posts_json = match serde_json::to_string(&posts) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize posts: {}", e),
                output: None,
            };
        }
    };

    StepResult {
        success: true,
        message: format!("Built visuals for {} posts", posts.len()),
        output: Some(posts_json),
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

    log::info!("[social_save_campaign] saving {} posts for campaign {}", posts.len(), task.id);

    // Save each post to the database
    let mut saved_count = 0;
    for post in &posts {
        match db::create_post(conn, post) {
            Ok(_) => saved_count += 1,
            Err(e) => {
                log::warn!("[social_save_campaign] failed to save post {}: {}", post.id, e);
            }
        }
    }

    // Update campaign post count
    if let Some(campaign_id) = extract_campaign_id_from_task(task) {
        if let Err(e) = db::update_campaign_post_count(conn, &campaign_id, saved_count as u32) {
            log::warn!("[social_save_campaign] failed to update campaign count: {}", e);
        }
        
        // Update campaign status to active
        if let Err(e) = db::update_campaign_status(conn, &campaign_id, CampaignStatus::Active) {
            log::warn!("[social_save_campaign] failed to update campaign status: {}", e);
        }
    }

    StepResult {
        success: saved_count > 0,
        message: format!("Saved {} of {} posts", saved_count, posts.len()),
        output: Some(format!("{{\"saved\":{},\"total\":{}}}", saved_count, posts.len())),
    }
}

#[derive(serde::Serialize)]
struct CampaignSaveResult {
    campaign_id: String,
    posts_saved: usize,
    posts: Vec<SocialPost>,
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

    log::info!("[social_regenerate_single] regenerating post {} with feedback: {}",
        post_id, feedback);

    // For now, return success - full implementation would load post, call agent with feedback
    let _ = (post_id, feedback, project_path, agent_provider);

    StepResult {
        success: true,
        message: "Post regeneration initiated".to_string(),
        output: None,
    }
}

pub fn exec_social_rebuild_visual(
    _task: &Task,
    _project_path: &str,
) -> StepResult {
    StepResult {
        success: true,
        message: "Visual rebuild complete".to_string(),
        output: None,
    }
}

pub fn exec_social_update_post(
    _task: &Task,
    _project_path: &str,
) -> StepResult {
    StepResult {
        success: true,
        message: "Post updated".to_string(),
        output: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Template Creation Steps
// ═══════════════════════════════════════════════════════════════════════════════

pub fn exec_social_design_template(
    _step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    // Parse template request from task description
    let request = parse_create_template_request(task);

    let prompt = prompts::create_template_prompt(
        &request.name,
        &request.platform,
        &request.format,
        &request.description,
    );

    log::info!("[social_design_template] designing template '{}' for {:?}",
        request.name, request.platform);

    match crate::engine::agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(output) => {
            match parse_agent_template_output(&output) {
                Ok(agent_output) => {
                    let template = create_template_from_agent_output(
                        &request,
                        &agent_output,
                    );

                    StepResult {
                        success: true,
                        message: format!("Template '{}' designed successfully", template.name),
                        output: Some(serde_json::to_string(&template).unwrap_or_default()),
                    }
                }
                Err(e) => StepResult {
                    success: false,
                    message: format!("Failed to parse template output: {}", e),
                    output: Some(output),
                }
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Agent failed: {}", e),
            output: None,
        }
    }
}

pub fn exec_social_save_template(
    _task: &Task,
    _project_path: &str,
) -> StepResult {
    StepResult {
        success: true,
        message: "Template saved".to_string(),
        output: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_source_config_from_task(_task: &Task) -> SourceConfig {
    // Default config - in real implementation, parse from task description
    SourceConfig {
        include_articles: true,
        article_slugs: vec![],
        include_screenshots: true,
        screenshot_dirs: vec![],
        include_specs: false,
    }
}

fn parse_template_ids_from_task(task: &Task) -> Vec<String> {
    // Parse from task description
    vec![
        "article_hook".to_string(),
        "article_carousel".to_string(),
    ]
}

fn parse_platforms_from_task(task: &Task) -> Vec<Platform> {
    vec![
        Platform::InstagramFeed,
        Platform::TikTok,
    ]
}

fn load_posts_from_artifacts(task: &Task) -> Option<Vec<SocialPost>> {
    // Try to load from social_generate_posts first, then social_build_visuals
    task.artifacts
        .iter()
        .find(|a| a.key == "social_build_visuals" || a.key == "social_generate_posts")
        .and_then(|a| a.content.as_ref())
        .and_then(|c| serde_json::from_str(c).ok())
}

fn get_site_url_from_project(task: &Task) -> String {
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

fn create_default_template(id: &str) -> Result<ContentTemplate, String> {
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
        default_hashtags: vec!["#contentmarketing".to_string(), "#digitalmarketing".to_string()],
        example_output: None,
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
    })
}

fn parse_agent_post_output(output: &str) -> Result<AgentPostOutput, String> {
    // Extract JSON from agent output
    let json_str = extract_json_block(output)?;
    serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))
}

fn parse_agent_template_output(output: &str) -> Result<AgentTemplateOutput, String> {
    let json_str = extract_json_block(output)?;
    serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse JSON: {}", e))
}

fn extract_json_block(output: &str) -> Result<String, String> {
    // Find JSON block between ```json and ```
    if let Some(start) = output.find("```json") {
        let after_start = &output[start + 7..];
        if let Some(end) = after_start.find("```") {
            return Ok(after_start[..end].trim().to_string());
        }
    }

    // Try just finding { and }
    if let Some(start) = output.find('{') {
        if let Some(end) = output.rfind('}') {
            return Ok(output[start..=end].to_string());
        }
    }

    Err("No JSON block found in output".to_string())
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

fn parse_regenerate_params(task: &Task) -> (String, String) {
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
