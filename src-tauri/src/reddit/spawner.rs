use chrono::Utc;
use rusqlite::Connection;

use crate::engine::task_store;
use crate::models::reddit::RedditOpportunity;
use crate::models::task::{AgentPolicy, ExecutionMode, Priority, Task, TaskRun, TaskStatus};

/// Create `reddit_reply` tasks from selected opportunities in a completed search task.
///
/// Parses the `reddit_results_stage` artifact from the parent task, filters to the
/// selected `post_ids`, and creates one task per opportunity. Marks the parent task
/// as Done on success.
pub fn create_reply_tasks_from_opportunities(
    conn: &Connection,
    task_id: &str,
    post_ids: &[String],
) -> Result<Vec<Task>, String> {
    if post_ids.is_empty() {
        return Err("No opportunities selected".to_string());
    }

    let parent_task = task_store::get_task(conn, task_id)
        .map_err(|e| format!("Failed to get parent task: {}", e))?;

    let results_artifact = parent_task.artifacts.iter()
        .find(|a| a.key == "reddit_results_stage")
        .ok_or_else(|| "No reddit_results_stage artifact found. Run the search first.".to_string())?;

    let artifact_content = results_artifact.content.as_ref()
        .ok_or_else(|| "reddit_results_stage artifact has no content".to_string())?;

    let opportunities: Vec<RedditOpportunity> = serde_json::from_str(artifact_content)
        .map_err(|e| format!("Failed to parse opportunities: {}", e))?;

    let selected_opps: Vec<RedditOpportunity> = opportunities.into_iter()
        .filter(|o| post_ids.contains(&o.post_id))
        .collect();

    if selected_opps.is_empty() {
        return Err("None of the selected post IDs were found in the search results".to_string());
    }

    let mut created_tasks = Vec::new();
    let now = Utc::now().to_rfc3339();

    for (idx, opp) in selected_opps.iter().enumerate() {
        let task_id = format!("task-{}", Utc::now().timestamp_millis() + idx as i64);

        let priority = match opp.severity.as_deref() {
            Some("CRITICAL") | Some("HIGH") => Priority::High,
            _ => Priority::Medium,
        };

        let title = format!(
            "Reply to: {}",
            opp.title.as_deref().unwrap_or("Reddit post").chars().take(50).collect::<String>()
        );

        let description = format!(
            "**Subreddit:** r/{}\n\n**Post URL:** {}\n\n**Why Relevant:** {}\n\n**Draft Reply:**\n{}\n\n**Post ID:** {}",
            opp.subreddit.as_deref().unwrap_or("unknown"),
            opp.url.as_deref().unwrap_or(""),
            opp.why_relevant.as_deref().unwrap_or(""),
            opp.reply_text.as_deref().unwrap_or("(no draft reply)"),
            opp.post_id
        );

        let task = Task {
            id: task_id,
            project_id: parent_task.project_id.clone(),
            task_type: "reddit_reply".to_string(),
            phase: "engagement".to_string(),
            status: TaskStatus::Todo,
            priority,
            execution_mode: ExecutionMode::Manual,
            agent_policy: AgentPolicy::Optional,
            title: Some(title),
            description: Some(description),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun { attempts: 0, last_error: None, provider: None, ..Default::default() },
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        task_store::create_task(conn, &task)
            .map_err(|e| format!("Failed to create task: {}", e))?;

        created_tasks.push(task);
    }

    task_store::update_task_status(conn, &parent_task.id, TaskStatus::Done)
        .map_err(|e| format!("Failed to update parent task status: {}", e))?;

    log::info!(
        "[create_reply_tasks] created {} reply tasks from parent {} and marked parent as Done",
        created_tasks.len(),
        parent_task.id
    );

    Ok(created_tasks)
}
