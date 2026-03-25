//! WorkflowHandler implementation for social media tasks

use crate::engine::workflows::handlers::WorkflowHandler;
use crate::engine::workflows::WorkflowStep;
use crate::models::task::Task;

pub struct SocialHandler;

impl WorkflowHandler for SocialHandler {
    fn supports(&self, task: &Task) -> bool {
        task.task_type.starts_with("social_")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task.task_type.as_str() {
            "social_generate_campaign" => vec![
                // Step 1: Collect content sources from project
                WorkflowStep::new("social_collect_sources", "social_collect_sources"),
                
                // Step 2: Load templates for this campaign
                WorkflowStep::new("social_load_templates", "social_load_templates"),
                
                // Step 3: For each source + template combo, generate posts
                // This is an agentic step that creates the actual content
                WorkflowStep::new("social_generate_posts", "social_generate_posts"),
                
                // Step 4: Build visual assets (overlays, carousels)
                WorkflowStep::new("social_build_visuals", "social_build_visuals"),
                
                // Step 5: Save everything to database
                WorkflowStep::new("social_save_campaign", "social_save_campaign"),
            ],
            
            "social_generate_from_article" => vec![
                // Single article → multi-platform posts
                WorkflowStep::new("social_extract_article", "social_extract_article"),
                WorkflowStep::new("social_generate_posts", "social_generate_posts"),
                WorkflowStep::new("social_build_visuals", "social_build_visuals"),
                WorkflowStep::new("social_save_campaign", "social_save_campaign"),
            ],
            
            "social_regenerate_post" => vec![
                // Regenerate a single post based on feedback
                WorkflowStep::new("social_regenerate_single", "social_regenerate_single"),
                WorkflowStep::new("social_rebuild_visual", "social_rebuild_visual"),
                WorkflowStep::new("social_update_post", "social_update_post"),
            ],
            
            "social_create_template" => vec![
                // Create a new template (agentic)
                WorkflowStep::new("social_design_template", "social_design_template"),
                WorkflowStep::new("social_save_template", "social_save_template"),
            ],
            
            _ => vec![WorkflowStep::new("social_manual", "manual")],
        }
    }
}

// TODO: Implement step runners in engine/executor.rs for:
// - social_collect_sources (deterministic)
// - social_load_templates (deterministic)
// - social_generate_posts (agentic)
// - social_build_visuals (deterministic)
// - social_save_campaign (deterministic)
// - social_regenerate_single (agentic)
// - social_rebuild_visual (deterministic)
// - social_update_post (deterministic)
// - social_design_template (agentic)
// - social_save_template (deterministic)
