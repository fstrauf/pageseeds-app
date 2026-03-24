/// Prompt builder — combines a Skill's SKILL.md content with project context
/// and task metadata to construct a complete agent prompt string.

use serde::{Deserialize, Serialize};
use crate::engine::skills::Skill;
use crate::models::task::Task;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptContext {
    pub task_id: String,
    pub skill_name: String,
    pub project_id: String,
    pub prompt: String,
    pub word_count: usize,
    pub sections: Vec<PromptSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSection {
    pub label: String,
    pub content: String,
}

// ─── Builder ─────────────────────────────────────────────────────────────────

/// Build a prompt context for a task + skill combination.
///
/// The constructed prompt has three sections (in order):
/// 1. **Skill** — the full SKILL.md content (provides workflow definition)
/// 2. **Project Context** — site_url, project id, repo path
/// 3. **Task** — task_id, type, title, description, phase, artifacts
pub fn build_prompt(
    task: &Task,
    skill: &Skill,
    project_path: &str,
    site_url: Option<&str>,
) -> PromptContext {
    let mut sections: Vec<PromptSection> = Vec::new();

    // 1. Skill section
    sections.push(PromptSection {
        label: "Skill".to_string(),
        content: skill.content.clone(),
    });

    // 2. Project context
    let project_ctx = build_project_section(&task.project_id, project_path, site_url);
    sections.push(PromptSection {
        label: "Project Context".to_string(),
        content: project_ctx,
    });

    // 3. Task section
    let task_ctx = build_task_section(task);
    sections.push(PromptSection {
        label: "Task".to_string(),
        content: task_ctx,
    });

    // Assemble
    let prompt = sections
        .iter()
        .map(|s| format!("## {}\n\n{}", s.label, s.content))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");

    let word_count = prompt.split_whitespace().count();

    PromptContext {
        task_id: task.id.clone(),
        skill_name: skill.name.clone(),
        project_id: task.project_id.clone(),
        prompt,
        word_count,
        sections,
    }
}

// ─── Private helpers ─────────────────────────────────────────────────────────

fn build_project_section(project_id: &str, path: &str, site_url: Option<&str>) -> String {
    let mut lines = vec![
        format!("- project_id: {}", project_id),
        format!("- repo_path: {}", path),
    ];
    if let Some(url) = site_url {
        if !url.is_empty() {
            lines.push(format!("- site_url: {}", url));
        }
    }
    lines.join("\n")
}

fn build_task_section(task: &Task) -> String {
    let mut lines = vec![
        format!("- id: {}", task.id),
        format!("- type: {}", task.task_type),
        format!("- phase: {}", task.phase),
        format!("- status: {}", task.status),
        format!("- priority: {}", task.priority),
    ];

    if let Some(ref title) = task.title {
        if !title.is_empty() {
            lines.push(format!("- title: {}", title));
        }
    }

    if let Some(ref desc) = task.description {
        if !desc.is_empty() {
            lines.push(format!("\n### Description\n\n{}", desc));
        }
    }

    // Artifacts
    if !task.artifacts.is_empty() {
        lines.push("\n### Artifacts".to_string());
        for a in &task.artifacts {
            let mut parts = vec![format!("- key: {}", a.key)];
            if let Some(ref p) = a.path {
                parts.push(format!("  path: {}", p));
            }
            if let Some(ref t) = a.artifact_type {
                parts.push(format!("  type: {}", t));
            }
            if let Some(ref c) = a.content {
                // Inline content — truncate to 500 chars to avoid huge prompts
                let preview = if c.len() > 500 {
                    format!("{}… [truncated]", crate::engine::text::char_prefix(c, 500))
                } else {
                    c.clone()
                };
                parts.push(format!("  content: {}", preview));
            }
            lines.push(parts.join("\n"));
        }
    }

    lines.join("\n")
}
