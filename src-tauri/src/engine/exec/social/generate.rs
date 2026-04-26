use std::path::Path;

use chrono::Utc;

use crate::engine::workflows::StepResult;
use crate::engine::workflows::WorkflowStep;
use crate::models::social::*;
use crate::models::task::Task;
use crate::social::content::sources::discover_sources;
use crate::social::models::{AgentPostOutput, PostGenerationJob};
use crate::social::prompts;
use crate::social::templates::{TemplateDef, render_prompt, validate_output};

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
    let config = super::parse_source_config_from_task(task);
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

    let template_ids = super::parse_template_ids_from_task(task);
    let platforms = super::parse_platforms_from_task(task);

    // Get templates from database or create defaults
    let templates = match super::load_templates_for_generation(&task.project_id, &template_ids) {
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
    let site_url = super::get_site_url_from_project(task);
    let _project_context = prompts::default_project_context(&site_url);

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

    // Limit to max 10 posts per campaign to avoid overwhelming the user
    const MAX_POSTS: usize = 10;
    if jobs.len() > MAX_POSTS {
        log::info!("[social_generate_posts] limiting from {} to {} posts", jobs.len(), MAX_POSTS);
        jobs.truncate(MAX_POSTS);
    }
    
    log::info!("[social_generate_posts] generating {} posts", jobs.len());

    // Generate each post via agent using template system
    let mut generated_posts: Vec<SocialPost> = Vec::new();
    let campaign_id = super::extract_campaign_id_from_task(task).unwrap_or_else(|| task.id.clone());
    
    // Use new template registry for better template selection
    let _registry = crate::social::templates::TemplateRegistry::default();

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
                match super::parse_agent_post_output(&output) {
                    Ok(agent_output) => {
                        // Validate against template schema
                        let json_output = serde_json::to_value(&agent_output).unwrap_or_default();
                        let validation_errors = validate_output(&job.template, &json_output);
                        
                        if !validation_errors.is_empty() {
                            log::warn!("[social_generate_posts] validation errors: {:?}", validation_errors);
                        }
                        
                        let post = super::create_social_post_from_agent_output(
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
