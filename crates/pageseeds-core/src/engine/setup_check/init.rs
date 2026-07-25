/// Template for project.md
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::*;
fn project_md_template(project_name: &str) -> String {
    format!(
        r#"# {}

## Project Summary
<!-- Describe your project, target audience, and main value proposition -->

## Brand Voice
<!-- Describe your brand personality, tone, and style guidelines -->
- Professional but approachable
- Technical accuracy with clarity
- Helpful and educational

## Key Topics
<!-- List main content topics and themes -->
- 

## Target Audience
<!-- Describe your ideal reader/customer -->

## Competitive Differentiation
<!-- What makes your content/product unique -->
"#,
        project_name
    )
}

/// Template for reddit_config.md
fn reddit_config_template() -> &'static str {
    r#"# Reddit Configuration

## Product Name
<!-- Your product or service name -->
- 

## Mention Stance
<!-- REQUIRED, RECOMMENDED, OPTIONAL, or OMIT -->
- OPTIONAL

## Trigger Topics
<!-- Topics to search for on Reddit -->
- topic 1
- topic 2
- topic 3

## Seed Subreddits
<!-- Subreddits to monitor (without r/ prefix) -->
- subreddit1
- subreddit2

## Excluded Subreddits
<!-- Subreddits to ignore -->
- 
"#
}

/// Template for _reply_guardrails.md
fn reply_guardrails_template() -> &'static str {
    r#"# Reddit Reply Guardrails

## Safety Rules
- Never spam or post low-effort replies
- Always provide genuine value
- Disclose affiliation when mentioning the product
- Follow subreddit rules and reddiquette

## Tone Guidelines
- Be helpful and authentic
- Avoid overly promotional language
- Match the community's tone and style
- Use proper grammar and formatting

## Prohibited Content
- Direct sales pitches
- Off-topic mentions
- Copy-paste responses
- Multiple posts to the same thread
"#
}

/// Initialize a complete project workspace with all required files.
/// This creates:
/// 1. .github/automation/ directory structure
/// 2. seo_workspace.json with auto-discovered content_dir
/// 3. articles.json (empty with nextArticleId: 1)
/// 4. project.md template
/// 5. reddit_config.md template
/// 6. reddit/_reply_guardrails.md template
/// 7. artifacts/, task_results/ directories
/// 8. Updates .gitignore
///
/// Returns a summary of what was created.
pub fn initialize_project_workspace(
    repo_root: &Path,
    site_url_hint: Option<&str>,
    project_name: Option<&str>,
) -> std::result::Result<Vec<String>, String> {
    let automation_dir = repo_root.join(".github").join("automation");
    let reddit_dir = automation_dir.join("reddit");
    let mut created = Vec::new();

    // Create directory structure
    std::fs::create_dir_all(&automation_dir)
        .map_err(|e| format!("Cannot create automation directory: {}", e))?;
    std::fs::create_dir_all(&reddit_dir)
        .map_err(|e| format!("Cannot create reddit directory: {}", e))?;
    std::fs::create_dir_all(automation_dir.join("artifacts"))
        .map_err(|e| format!("Cannot create artifacts directory: {}", e))?;
    std::fs::create_dir_all(automation_dir.join("task_results"))
        .map_err(|e| format!("Cannot create task_results directory: {}", e))?;

    // Auto-discover content directory
    let content_dir = auto_discover_content_dir(repo_root);
    let content_dir_str = content_dir
        .as_ref()
        .and_then(|p| p.strip_prefix(repo_root).ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "content".to_string());

    // Create seo_workspace.json if it doesn't exist
    let workspace_config_path = automation_dir.join("seo_workspace.json");
    if !workspace_config_path.exists() {
        let site_url = site_url_hint.unwrap_or("");
        write_workspace_config(&automation_dir, &content_dir_str, site_url)?;
        created.push(format!(
            "seo_workspace.json (content_dir: {})",
            content_dir_str
        ));
    }

    // Create articles.json if it doesn't exist
    let articles_json_path = automation_dir.join("articles.json");
    if !articles_json_path.exists() {
        let empty_articles = r#"{
  "nextArticleId": 1,
  "articles": []
}"#;
        std::fs::write(&articles_json_path, empty_articles)
            .map_err(|e| format!("Cannot write articles.json: {}", e))?;
        created.push("articles.json".to_string());
    }

    // Create project.md if it doesn't exist (and no legacy files)
    let project_md_path = automation_dir.join("project.md");
    let legacy_summary = automation_dir.join("project_summary.md");
    let legacy_brand = automation_dir.join("brandvoice.md");
    if !project_md_path.exists() && !legacy_summary.exists() && !legacy_brand.exists() {
        let name = project_name.unwrap_or("My Project");
        std::fs::write(&project_md_path, project_md_template(name))
            .map_err(|e| format!("Cannot write project.md: {}", e))?;
        created.push("project.md".to_string());
    }

    // Create reddit_config.md if it doesn't exist
    let reddit_config_path = automation_dir.join("reddit_config.md");
    if !reddit_config_path.exists() {
        std::fs::write(&reddit_config_path, reddit_config_template())
            .map_err(|e| format!("Cannot write reddit_config.md: {}", e))?;
        created.push("reddit_config.md".to_string());
    }

    // Create reddit/_reply_guardrails.md if it doesn't exist
    let guardrails_path = reddit_dir.join("_reply_guardrails.md");
    if !guardrails_path.exists() {
        std::fs::write(&guardrails_path, reply_guardrails_template())
            .map_err(|e| format!("Cannot write _reply_guardrails.md: {}", e))?;
        created.push("reddit/_reply_guardrails.md".to_string());
    }

    // Update .gitignore
    if let Err(e) = update_gitignore(repo_root, &automation_dir) {
        log::warn!(
            "[initialize_project_workspace] Failed to update .gitignore: {}",
            e
        );
        // Don't fail initialization if gitignore update fails
    }

    Ok(created)
}

/// Update the project's .gitignore to exclude automation files that shouldn't be committed.
/// Adds entries for:
/// - artifacts/ (generated files)
/// - task_results/ (generated files)
fn update_gitignore(repo_root: &Path, _automation_dir: &Path) -> std::result::Result<(), String> {
    let gitignore_path = repo_root.join(".gitignore");

    // Entries to add
    let entries = vec![
        "# PageSeeds automation - generated artifacts",
        ".github/automation/artifacts/",
        ".github/automation/task_results/",
    ];

    // Read existing content
    let existing = if gitignore_path.exists() {
        std::fs::read_to_string(&gitignore_path)
            .map_err(|e| format!("Cannot read .gitignore: {}", e))?
    } else {
        String::new()
    };

    // Check which entries are missing
    let missing: Vec<&str> = entries
        .iter()
        .filter(|entry| !existing.contains(**entry))
        .copied()
        .collect();

    if missing.is_empty() {
        return Ok(());
    }

    // Append missing entries
    let mut new_content = existing;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push('\n');
    for entry in missing {
        new_content.push_str(entry);
        new_content.push('\n');
    }

    std::fs::write(&gitignore_path, new_content)
        .map_err(|e| format!("Cannot write .gitignore: {}", e))?;

    Ok(())
}
