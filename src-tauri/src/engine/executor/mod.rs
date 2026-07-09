/// Workflow execution orchestrator.
///
/// Finds the correct handler for a task, plans the step graph,
/// executes each step sequentially, persists artifacts, and
/// updates task status in SQLite.
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::Emitter as _;

use crate::engine::step_registry::{StepContext, StepRegistry};
use crate::engine::workflows::{handlers::default_handlers, step_params, StepResult, WorkflowStep};
use crate::engine::{agent, task_store};
use crate::models::task::{FollowUpPolicy, Task, TaskArtifact, TaskReviewSurface, TaskStatus};
use ts_rs::TS;

// ─── Event Types ──────────────────────────────────────────────────────────────

// ─── Public Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StepProgress {
    pub step_name: String,
    pub kind: String,
    pub status: String, // "pending" | "running" | "ok" | "failed" | "skipped"
    pub message: String,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExecutionResult {
    pub task_id: String,
    pub success: bool,
    pub message: String,
    pub steps: Vec<StepProgress>,
    #[serde(default)]
    pub follow_up_tasks: Vec<FollowUpTask>,
    pub started_at: String,
    pub finished_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FollowUpTask {
    pub id: String,
    pub task_type: String,
    pub title: String,
    pub status: String,
    pub run_policy: String,
    pub review_surface: String,
    pub follow_up_policy: String,
    pub priority: String,
}

const MAX_PROGRESS_OUTPUT_CHARS: usize = 4_000;

// ─── Engine ───────────────────────────────────────────────────────────────────

pub async fn execute_task(conn: &Connection, task_id: &str) -> Result<ExecutionResult, String> {
    execute_task_with_token(conn, task_id, None, None, false).await
}

/// Run `execute_task_with_token` in dry-run mode — plans steps but does not
/// call any `exec_*` functions or modify database state.
pub async fn dry_run_task(conn: &Connection, task_id: &str) -> Result<ExecutionResult, String> {
    execute_task_with_token(conn, task_id, None, None, true).await
}

pub async fn execute_task_with_token(
    conn: &Connection,
    task_id: &str,
    gsc_token: Option<&str>,
    app_handle: Option<tauri::AppHandle>,
    dry_run: bool,
) -> Result<ExecutionResult, String> {
    let mut task = task_store::get_task(conn, task_id).map_err(|e| e.to_string())?;

    let started_at = Utc::now().to_rfc3339();

    // Transition to in_progress
    if task.status == TaskStatus::Todo
        || task.status == TaskStatus::Review
        || task.status == TaskStatus::Failed
    {
        task.status = TaskStatus::InProgress;
        task.updated_at = started_at.clone();
        task.run.last_error = None;
        task_store::update_task_status(conn, task_id, TaskStatus::InProgress)
            .map_err(|e| e.to_string())?;
    }

    let (project_path, site_url, agent_provider, seo_provider) = {
        use crate::db::global_settings;
        let project = task_store::get_project(conn, &task.project_id).map_err(|e| e.to_string())?;

        // Agent provider is now global (user preference), but check for legacy project-specific setting
        let agent_provider = if let Some(legacy) = &project.agent_provider {
            let valid = matches!(legacy.as_str(), "kimi" | "claude" | "openai" | "ollama");
            if valid {
                log::debug!("[executor] Using legacy project agent_provider: {}", legacy);
                legacy.clone()
            } else {
                log::warn!("[executor] Invalid legacy project agent_provider '{}', falling back to global", legacy);
                global_settings::get_agent_provider(conn)
            }
        } else {
            global_settings::get_agent_provider(conn)
        };

        (
            project.path.clone(),
            project.site_url.clone().unwrap_or_default(),
            agent_provider,
            project
                .seo_provider
                .clone()
                .unwrap_or_else(|| "dataforseo".to_string()),
        )
    };

    let handlers = default_handlers();
    let handler = handlers.iter().find(|h| h.supports(&task));
    let Some(handler) = handler else {
        let msg = format!("No handler found for task type '{}'", task.task_type);
        _fail_task(conn, &mut task, &msg);
        return Ok(ExecutionResult {
            task_id: task_id.to_string(),
            success: false,
            message: msg,
            steps: vec![],
            follow_up_tasks: vec![],
            started_at,
            finished_at: Utc::now().to_rfc3339(),
        });
    };

    let steps = handler.plan(&task);
    let mut progress: Vec<StepProgress> = steps
        .iter()
        .map(|s| StepProgress {
            step_name: s.name.clone(),
            kind: s.kind.to_string(),
            status: "pending".to_string(),
            message: String::new(),
            output: None,
        })
        .collect();

    // Dry-run: return the planned step graph without executing anything.
    if dry_run {
        return Ok(ExecutionResult {
            task_id: task_id.to_string(),
            success: true,
            message: format!(
                "dry-run: {} steps planned for '{}'",
                progress.len(),
                task.task_type
            ),
            steps: progress,
            follow_up_tasks: vec![],
            started_at,
            finished_at: Utc::now().to_rfc3339(),
        });
    }

    let mut all_ok = true;
    let mut last_error = String::new();
    let mut latest_raw_output: Option<String> = None;
    let mut total_prompt_tokens: Option<u64> = None;
    let mut total_completion_tokens: Option<u64> = None;

    for (i, step) in steps.iter().enumerate() {
        progress[i].status = "running".to_string();

        let result = run_step(
            step,
            &task,
            &project_path,
            &site_url,
            &agent_provider,
            &seo_provider,
            latest_raw_output.as_deref(),
            gsc_token,
            conn,
        )
        .await;

        // Capture token usage from any agentic step that used a rig backend
        let (pt, ct) = agent::take_last_tokens();
        total_prompt_tokens = add_optional(total_prompt_tokens, pt);
        total_completion_tokens = add_optional(total_completion_tokens, ct);

        // Apply the step's latest_raw_policy to the pipeline variable.
        match step.latest_raw_policy {
            crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput => {
                if let Some(ref out) = result.output {
                    let preview = crate::engine::text::char_prefix(out, 300);
                    log::info!(
                        "[executor] step '{}' sets latest_raw ({} chars): {:?}",
                        step.name,
                        out.len(),
                        preview
                    );
                    latest_raw_output = Some(out.clone());
                } else {
                    log::warn!(
                        "[executor] step '{}' expected output for latest_raw but produced none",
                        step.name
                    );
                    latest_raw_output = None;
                }
            }
            crate::engine::workflows::LatestRawPolicy::Clear => {
                log::info!("[executor] step '{}' clears latest_raw", step.name);
                latest_raw_output = None;
            }
            crate::engine::workflows::LatestRawPolicy::Preserve => {
                // Nothing to do — downstream steps see the previous latest_raw.
            }
        }

        progress[i].status = if result.success {
            "ok".to_string()
        } else {
            "failed".to_string()
        };
        progress[i].message = result.message.clone();
        progress[i].output = result.output.as_deref().map(compact_progress_output);

        // Persist step output as the durable artifact. Keep the in-memory task
        // in sync for downstream steps, but replace by key so reruns do not
        // accumulate duplicate historical payloads.
        if let Some(ref out) = result.output {
            let artifact_key = step
                .params
                .get(step_params::ARTIFACT_NAME)
                .cloned()
                .unwrap_or_else(|| step.name.clone());
            let artifact = TaskArtifact {
                key: artifact_key,
                path: None,
                artifact_type: Some(step.kind.to_string()),
                source: Some(step.kind.to_string()),
                content: Some(out.clone()),
            };
            let _ = task_store::upsert_task_artifact(conn, task_id, &artifact);
            upsert_artifact_in_memory(&mut task.artifacts, artifact);
        }

        // Run domain-specific post-step side effects.
        let post = crate::engine::post_actions::after_step(
            &crate::engine::post_actions::PostStepContext {
                conn,
                task: &task,
                step,
                result: &result,
                project_path: &project_path,
                agent_provider: &agent_provider,
            },
        );
        if let Some(status) = post.status {
            progress[i].status = status;
        }
        if let Some(message) = post.message {
            progress[i].message = message;
        }
        if let Some(output) = post.output {
            progress[i].output = Some(compact_progress_output(&output));
        }
        if let Some(artifact) = post.artifact {
            let _ = task_store::upsert_task_artifact(conn, task_id, &artifact);
            upsert_artifact_in_memory(&mut task.artifacts, artifact);
        }

        if !result.success {
            if step.optional {
                log::warn!(
                    "[executor] optional step '{}' failed (skipped): {}",
                    step.name,
                    result.message
                );
                progress[i].status = "skipped".to_string();
            } else {
                all_ok = false;
                last_error = result.message.clone();
                break;
            }
        }
    }

    // Explicitly drop any remaining large raw output before creating follow-up tasks.
    drop(latest_raw_output);

    let finished_at = Utc::now().to_rfc3339();
    let new_status = completed_task_status(&task.task_type, all_ok);

    task_store::update_task_status(conn, task_id, new_status.clone()).map_err(|e| e.to_string())?;

    let mut follow_up_ids: Vec<String> = vec![];

    // Run domain-specific post-task side effects.
    if all_ok {
        follow_up_ids.extend(crate::engine::post_actions::after_task_success(
            &crate::engine::post_actions::PostTaskContext {
                conn,
                task: &task,
                project_path: &project_path,
                progress: &progress,
            },
        ));
    }
    let follow_up_tasks: Vec<FollowUpTask> = follow_up_ids
        .iter()
        .filter_map(|id| task_store::get_task(conn, id).ok())
        .map(|t| FollowUpTask {
            id: t.id,
            task_type: t.task_type,
            title: t.title.unwrap_or_else(|| "Untitled task".to_string()),
            status: t.status.to_string(),
            run_policy: t.run_policy.to_string(),
            review_surface: t.review_surface.to_string(),
            follow_up_policy: t.follow_up_policy.to_string(),
            priority: t.priority.to_string(),
        })
        .collect();

    // For tasks that go to "review" (e.g. keyword research), include the task
    // itself as a follow-up so the runner UI shows a "Select keywords" action
    // that navigates to the task detail with the review picker open.
    let follow_up_tasks = if new_status == TaskStatus::Review && all_ok {
        let mut fups = follow_up_tasks;
        fups.push(FollowUpTask {
            id: task_id.to_string(),
            task_type: task.task_type.clone(),
            title: task
                .title
                .clone()
                .unwrap_or_else(|| "Review results".to_string()),
            status: "review".to_string(),
            run_policy: "user_enqueue".to_string(),
            review_surface: task.review_surface.to_string(),
            follow_up_policy: task.follow_up_policy.to_string(),
            priority: task.priority.to_string(),
        });
        fups
    } else {
        follow_up_tasks
    };

    if !all_ok {
        task_store::record_task_run(
            conn,
            task_id,
            false,
            Some(&last_error),
            None,
            total_prompt_tokens,
            total_completion_tokens,
        )
        .map_err(|e| e.to_string())?;
    } else {
        task_store::record_task_run(
            conn,
            task_id,
            true,
            None,
            None,
            total_prompt_tokens,
            total_completion_tokens,
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(ExecutionResult {
        task_id: task_id.to_string(),
        success: all_ok,
        message: if all_ok {
            "Task completed".to_string()
        } else {
            last_error
        },
        steps: progress,
        follow_up_tasks,
        started_at,
        finished_at,
    })
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

async fn run_step(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    site_url: &str,
    agent_provider: &str,
    seo_provider: &str,
    latest_raw: Option<&str>,
    gsc_token: Option<&str>,
    conn: &Connection,
) -> crate::engine::workflows::StepResult {
    let registry = StepRegistry::new();
    let ctx = StepContext {
        task,
        project_path,
        site_url,
        agent_provider,
        seo_provider,
        latest_raw,
        gsc_token,
        conn,
    };

    if let Some(handler) = registry.get(&step.kind) {
        handler(step, &ctx).await
    } else {
        StepResult {
            success: false,
            message: format!("Unknown step kind '{}'", step.kind),
            output: None,
        }
    }
}

fn _fail_task(conn: &Connection, task: &mut Task, msg: &str) {
    let _ = task_store::update_task_status(conn, &task.id, TaskStatus::Failed);
    let _ = task_store::record_task_run(conn, &task.id, false, Some(msg), None, None, None);
}

/// Add two optional u64 values, returning Some if either is Some.
fn add_optional(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

fn compact_progress_output(output: &str) -> String {
    if output.chars().count() <= MAX_PROGRESS_OUTPUT_CHARS {
        return output.to_string();
    }

    let preview: String = output.chars().take(MAX_PROGRESS_OUTPUT_CHARS).collect();
    format!(
        "{}\n\n[output truncated in execution progress; full output is stored as a task artifact]",
        preview
    )
}

fn upsert_artifact_in_memory(artifacts: &mut Vec<TaskArtifact>, artifact: TaskArtifact) {
    if let Some(existing) = artifacts.iter_mut().find(|a| a.key == artifact.key) {
        *existing = artifact;
    } else {
        artifacts.push(artifact);
    }
}

/// Determine the final task status after all steps have run.
/// Extracted as a named function so it can be unit-tested.
pub(crate) fn completed_task_status(task_type: &str, all_ok: bool) -> TaskStatus {
    if all_ok {
        if crate::config::default_review_surface(task_type)
            != crate::models::task::TaskReviewSurface::None
        {
            TaskStatus::Review
        } else {
            TaskStatus::Done
        }
    } else {
        // Per-article fix tasks that fail verification land in Review (soft failure,
        // retryable) rather than Failed, so they don't get blindly re-queued but are
        // visible as needing attention.
        if task_type == "fix_ctr_article" || task_type == "fix_content_article" {
            TaskStatus::Review
        } else {
            TaskStatus::Failed
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
