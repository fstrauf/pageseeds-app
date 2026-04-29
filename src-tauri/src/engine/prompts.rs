use crate::engine::skills::Skill;
use crate::logging::{query_logs, LogQueryFilters, LogSource};
use crate::models::task::Task;
/// Prompt builder — combines a Skill's SKILL.md content with project context
/// and task metadata to construct a complete agent prompt string.
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

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

/// Build diagnostic context from recent logs for AI analysis.
/// This allows the AI to see what happened before it was invoked,
/// enabling self-diagnosis of issues.
///
/// This version opens the database directly using the project path.
pub fn build_diagnostic_context_from_task(
    task_id: &str,
    _project_path: &str,
    max_entries: usize,
) -> String {
    // Open database connection using default path
    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[diagnostic_context] Failed to open DB: {}", e);
            return String::new(); // Silently fail if DB not available
        }
    };

    let mut lines = vec!["## Diagnostic Context (Recent System Logs)".to_string()];

    // Query recent logs from all sources, filtering by task if possible
    let filters = LogQueryFilters {
        level: None,
        source: None,
        component: Some(task_id.to_string()), // Try to find logs related to this task
        session_id: None,
        search_query: None,
    };

    log::debug!(
        "[diagnostic_context] Querying logs for task_id: {}",
        task_id
    );

    // First try: search for logs mentioning this task ID
    let mut logs = match query_logs(&conn, &filters, max_entries, 0) {
        Ok(l) => {
            log::debug!("[diagnostic_context] Found {} task-specific logs", l.len());
            l
        }
        Err(e) => {
            log::warn!("[diagnostic_context] Failed to query task logs: {}", e);
            Vec::new()
        }
    };

    // If not enough task-specific logs, get general recent logs
    if logs.len() < 10 {
        log::debug!(
            "[diagnostic_context] Only {} task logs, fetching general logs",
            logs.len()
        );
        let general_filters = LogQueryFilters {
            level: Some(crate::logging::LogLevel::Info),
            source: None,
            component: None,
            session_id: None,
            search_query: None,
        };
        match query_logs(&conn, &general_filters, max_entries, 0) {
            Ok(general_logs) => {
                log::debug!(
                    "[diagnostic_context] Found {} general logs",
                    general_logs.len()
                );
                // Merge and deduplicate
                for log_entry in general_logs {
                    if !logs.iter().any(|l| l.id == log_entry.id) {
                        logs.push(log_entry);
                    }
                }
            }
            Err(e) => {
                log::warn!("[diagnostic_context] Failed to query general logs: {}", e);
            }
        }
    }

    if logs.is_empty() {
        log::debug!("[diagnostic_context] No logs found");
        lines.push("No recent logs available.".to_string());
    } else {
        lines.push(format!(
            "Showing last {} log entries:\n",
            logs.len().min(max_entries)
        ));

        for (i, log) in logs.iter().take(max_entries).rev().enumerate() {
            let source_icon = match log.source {
                LogSource::Frontend => "[UI]",
                LogSource::Backend => "[SYS]",
                LogSource::Agent => "[AI]",
                LogSource::System => "[SYS]",
            };
            lines.push(format!(
                "{}. {} {} | {} | {}: {}",
                i + 1,
                &log.timestamp[11..19], // HH:MM:SS
                source_icon,
                log.level,
                log.component,
                log.message.lines().next().unwrap_or(&log.message)
            ));
            // Include metadata if present (truncated)
            if let Some(ref meta) = log.metadata {
                let meta_str = serde_json::to_string(meta).unwrap_or_default();
                if !meta_str.is_empty() && meta_str != "null" {
                    let preview = if meta_str.len() > 100 {
                        format!("{}…", &meta_str[..100])
                    } else {
                        meta_str
                    };
                    lines.push(format!("   └─ {}", preview));
                }
            }
        }

        lines.push("\n".to_string());
        lines.push("Analyze these logs to understand:".to_string());
        lines.push("- What steps have already been executed".to_string());
        lines.push("- Any errors or warnings that occurred".to_string());
        lines.push("- The current state of the task".to_string());
        lines.push("- What action should be taken next".to_string());
    }

    lines.join("\n")
}

/// Build diagnostic context from recent logs for AI analysis.
/// This allows the AI to see what happened before it was invoked,
/// enabling self-diagnosis of issues.
pub fn build_diagnostic_context(conn: &Connection, task: &Task, max_entries: usize) -> String {
    let mut lines = vec!["## Diagnostic Context (Recent System Logs)".to_string()];

    // Query recent logs from all sources, filtering by task if possible
    let filters = LogQueryFilters {
        level: None,
        source: None,
        component: Some(format!("{}", task.id)), // Try to find logs related to this task
        session_id: None,
        search_query: None,
    };

    // First try: search for logs mentioning this task ID
    let mut logs = query_logs(conn, &filters, max_entries, 0).unwrap_or_default();

    // If not enough task-specific logs, get general recent logs
    if logs.len() < 10 {
        let general_filters = LogQueryFilters {
            level: Some(crate::logging::LogLevel::Info),
            source: None,
            component: None,
            session_id: None,
            search_query: None,
        };
        let general_logs = query_logs(conn, &general_filters, max_entries, 0).unwrap_or_default();
        // Merge and deduplicate
        for log in general_logs {
            if !logs.iter().any(|l| l.id == log.id) {
                logs.push(log);
            }
        }
    }

    if logs.is_empty() {
        lines.push("No recent logs available.".to_string());
    } else {
        lines.push(format!(
            "Showing last {} log entries:\n",
            logs.len().min(max_entries)
        ));

        for (i, log) in logs.iter().take(max_entries).rev().enumerate() {
            let source_icon = match log.source {
                LogSource::Frontend => "[UI]",
                LogSource::Backend => "[SYS]",
                LogSource::Agent => "[AI]",
                LogSource::System => "[SYS]",
            };
            lines.push(format!(
                "{}. {} {} | {} | {}: {}",
                i + 1,
                &log.timestamp[11..19], // HH:MM:SS
                source_icon,
                log.level,
                log.component,
                log.message.lines().next().unwrap_or(&log.message)
            ));
            // Include metadata if present (truncated)
            if let Some(ref meta) = log.metadata {
                let meta_str = serde_json::to_string(meta).unwrap_or_default();
                if !meta_str.is_empty() && meta_str != "null" {
                    let preview = if meta_str.len() > 100 {
                        format!("{}…", &meta_str[..100])
                    } else {
                        meta_str
                    };
                    lines.push(format!("   └─ {}", preview));
                }
            }
        }

        lines.push("\n".to_string());
        lines.push("Analyze these logs to understand:".to_string());
        lines.push("- What steps have already been executed".to_string());
        lines.push("- Any errors or warnings that occurred".to_string());
        lines.push("- The current state of the task".to_string());
        lines.push("- What action should be taken next".to_string());
    }

    lines.join("\n")
}
