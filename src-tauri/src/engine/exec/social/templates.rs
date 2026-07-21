use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::engine::workflows::WorkflowStep;
use crate::models::task::Task;
use crate::social::prompts;

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
    let request = super::parse_create_template_request(task);

    let prompt = prompts::create_template_prompt(
        &request.name,
        &request.platform,
        &request.format,
        &request.description,
    );

    log::info!(
        "[social_design_template] designing template '{}' for {:?}",
        request.name,
        request.platform
    );

    match crate::engine::agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(output) => match super::parse_agent_template_output(&output) {
            Ok(agent_output) => {
                let template = super::create_template_from_agent_output(&request, &agent_output);

                StepResult {
                    success: true,
                    message: format!("Template '{}' designed successfully", template.name),
                    output: Some(serde_json::to_string(&template).unwrap_or_default()),
                }
            }
            Err(e) => StepResult::fail_with_output(format!("Failed to parse template output: {}", e), output),
        },
        Err(e) => StepResult::fail(format!("Agent failed: {}", e)),
    }
}

pub fn exec_social_save_template(_task: &Task, _project_path: &str) -> StepResult {
    StepResult {
        success: true,
        message: "Template saved".to_string(),
        output: None,
    }
}
