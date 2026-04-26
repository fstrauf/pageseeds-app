use std::path::Path;

use crate::reddit::config as reddit_cfg;
use crate::engine::skills;

/// Build the prompt and generate a draft reply for a Reddit opportunity.
///
/// This function reads project context, Reddit config, guardrails, and skill files,
/// builds the prompt, calls the agent, and returns the generated reply text.
/// It does NOT write to the database — the caller is responsible for persisting.
pub async fn generate_draft_reply(
    project_path: &str,
    agent_provider: &str,
    opp: &crate::models::reddit::RedditOpportunity,
) -> Result<String, String> {
    let repo_root = Path::new(project_path);
    let automation_dir = repo_root.join(".github").join("automation");

    let missing = reddit_cfg::missing_config_files(&automation_dir);
    if !missing.is_empty() {
        return Err(format!(
            "Missing required config files: {}. Create them in .github/automation/ first.",
            missing.join(", ")
        ));
    }

    // Primary: project.md (consolidated). Fallback: legacy files.
    let project_context = std::fs::read_to_string(automation_dir.join("project.md"))
        .or_else(|_| {
            let summary = std::fs::read_to_string(automation_dir.join("project_summary.md")).unwrap_or_default();
            let brand = std::fs::read_to_string(automation_dir.join("brandvoice.md")).unwrap_or_default();
            if summary.is_empty() && brand.is_empty() {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no project context"))
            } else {
                Ok(format!("{}\n\n{}", summary, brand))
            }
        })
        .map_err(|e| format!("Failed to read project.md: {}", e))?;

    let reddit_config_raw = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
        .unwrap_or_default();
    let guardrails = std::fs::read_to_string(
        automation_dir.join("reddit").join("_reply_guardrails.md")
    ).unwrap_or_default();

    let skill_content = skills::load_skill(repo_root, "reddit-reply-drafting")
        .map(|s| s.content)
        .unwrap_or_default();

    // Read product_name and mention_stance from the opportunity row.
    // Fall back to deterministic parsing only for pre-migration rows.
    let (product_name, mention_stance) = match (&opp.product_name, &opp.mention_stance) {
        (Some(name), Some(stance)) if !name.is_empty() => {
            log::info!("[draft] using DB-stored params: name='{}', stance='{}'", name, stance);
            (name.clone(), stance.clone())
        }
        _ => {
            log::info!("[draft] DB values missing, falling back to deterministic parse");
            let cfg = reddit_cfg::parse_reddit_config(&reddit_config_raw);
            let name = cfg.product_name.unwrap_or_else(|| "the product".to_string());
            let stance = cfg.mention_stance.as_str().to_string();
            (name, stance)
        }
    };

    let prompt = crate::reddit::prompts::build_draft_reply_prompt(
        &project_context,
        &guardrails,
        &skill_content,
        &product_name,
        &mention_stance,
        opp,
    );

    let reply_text = crate::engine::agent::run_agent(agent_provider, &prompt, repo_root)
        .map_err(|e| format!("Agent failed: {}", e))?;

    Ok(reply_text.trim().to_string())
}
