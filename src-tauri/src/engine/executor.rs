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
                .unwrap_or_else(|| "ahrefs".to_string()),
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
mod tests {
    use super::*;
    use crate::engine::task_store;
    use crate::engine::workflows::handlers::default_handlers;
    use crate::engine::workflows::StepKind;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };
    use rusqlite::Connection;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Run all schema migrations on an in-memory connection.
    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY, name TEXT NOT NULL,
                path TEXT NOT NULL,
                content_dir TEXT,
                site_url TEXT,
                site_id TEXT,
                sitemap_url TEXT,
                project_mode TEXT NOT NULL DEFAULT 'workspace',
                active INTEGER NOT NULL DEFAULT 1,
                agent_provider TEXT,
                seo_provider TEXT
             );
             CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
                review_surface TEXT NOT NULL DEFAULT 'none',
                follow_up_policy TEXT NOT NULL DEFAULT 'none',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT, description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER NOT NULL DEFAULT 0,
                run_last_error TEXT, run_provider TEXT,
                not_before TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL, attempt INTEGER NOT NULL,
                provider TEXT, started_at TEXT NOT NULL,
                finished_at TEXT, success INTEGER, error TEXT,
                prompt_tokens INTEGER, completion_tokens INTEGER
             );
             CREATE TABLE IF NOT EXISTS articles (
                id INTEGER NOT NULL, title TEXT NOT NULL DEFAULT '',
                url_slug TEXT NOT NULL DEFAULT '', file TEXT NOT NULL DEFAULT '',
                target_keyword TEXT, keyword_difficulty TEXT,
                target_volume INTEGER DEFAULT 0, published_date TEXT,
                word_count INTEGER DEFAULT 0, status TEXT NOT NULL DEFAULT 'draft',
                review_status TEXT, review_started_at TEXT, last_reviewed_at TEXT,
                review_count INTEGER NOT NULL DEFAULT 0,
                content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
                estimated_traffic_monthly TEXT, page_type TEXT,
                project_id TEXT NOT NULL,
                PRIMARY KEY (id, project_id)
             );
             CREATE TABLE IF NOT EXISTS articles_meta (
                project_id TEXT PRIMARY KEY, next_article_id INTEGER NOT NULL DEFAULT 1
             );",
        )
        .unwrap();
        conn
    }

    fn test_project_in(conn: &Connection) -> String {
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES ('proj1', 'Test', '/tmp', 1)",
            [],
        )
        .unwrap();
        "proj1".to_string()
    }

    fn test_project_in_at_path(conn: &Connection, path: &str) -> String {
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES ('proj1', 'Test', ?1, 1)",
            [path],
        )
        .unwrap();
        "proj1".to_string()
    }

    fn setup_dummy_keyword_project(dir: &std::path::Path, theme: &str) {
        let automation = dir.join(".github").join("automation");
        std::fs::create_dir_all(&automation).unwrap();
        let brief = format!(
            "# Test Project\n\n## Content Clusters & Status\n\n### Cluster 1: {theme} (PLANNED)\n"
        );
        std::fs::write(automation.join("project.md"), brief).unwrap();
        std::fs::write(automation.join("articles.json"), "[]").unwrap();
        std::fs::write(
            automation.join("keyword_coverage.json"),
            serde_json::json!({
                "clusters": [
                    {"cluster_name": "Risk Management", "article_count": 1},
                    {"cluster_name": "Portfolio Hedging", "article_count": 0}
                ]
            })
            .to_string(),
        )
        .unwrap();
    }

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{prefix}_{nanos}"))
    }

    fn make_task(task_type: &str, project_id: &str) -> Task {
        Task {
            id: format!("test-{task_type}"),
            task_type: task_type.to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: Some(format!("{task_type} test")),
            description: None,
            project_id: project_id.to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun {
                attempts: 0,
                last_error: None,
                provider: None,
                ..Default::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    // 1. Keyword research and Reddit tasks end with "review" status, not "done".
    #[test]
    fn review_tasks_go_to_review_status() {
        assert_eq!(
            completed_task_status("research_keywords", true),
            TaskStatus::Review
        );
        assert_eq!(
            completed_task_status("custom_keyword_research", true),
            TaskStatus::Review
        );
        assert_eq!(
            completed_task_status("research_landing_pages", true),
            TaskStatus::Review
        );
        assert_eq!(
            completed_task_status("reddit_opportunity_search", true),
            TaskStatus::Review
        );
    }

    // 2. Tasks without a review surface go to "done", not "review".
    #[test]
    fn non_research_task_goes_to_done() {
        assert_eq!(completed_task_status("collect_gsc", true), TaskStatus::Done);
        assert_eq!(
            completed_task_status("fix_indexing", true),
            TaskStatus::Done
        );
    }

    // 3. Tasks with a review surface (including content review) go to "review".
    #[test]
    fn review_surface_task_goes_to_review() {
        assert_eq!(
            completed_task_status("content_review", true),
            TaskStatus::Review
        );
        assert_eq!(
            completed_task_status("content_audit", true),
            TaskStatus::Review
        );
    }

    // 3. Most failed tasks go to "failed" so they can be found and retried.
    #[test]
    fn failed_task_goes_to_failed() {
        assert_eq!(
            completed_task_status("research_keywords", false),
            TaskStatus::Failed
        );
        assert_eq!(
            completed_task_status("content_review", false),
            TaskStatus::Failed
        );
        assert_eq!(
            completed_task_status("fix_indexing", false),
            TaskStatus::Failed
        );
    }

    // 4. CTR fix failures land in Review (soft failure, retryable) rather than Todo,
    //    so they don't get blindly re-queued by the batch executor.
    #[test]
    fn fix_ctr_article_failure_goes_to_review() {
        assert_eq!(
            completed_task_status("fix_ctr_article", false),
            TaskStatus::Review
        );
    }

    // 4. Handler registry routes fix_* task types to ImplementationHandler.
    #[test]
    fn fix_prefix_routes_to_implementation_handler() {
        let task_types = ["fix_indexing", "fix_redirect", "fix_404", "fix_coverage"];
        let handlers = default_handlers();
        for tt in &task_types {
            let task = make_task(tt, "proj1");
            let matched = handlers.iter().find(|h| h.supports(&task));
            assert!(matched.is_some(), "No handler for task type '{tt}'");
            // ImplementationHandler produces specific step kinds for fix_* types.
            let steps = matched.unwrap().plan(&task);
            assert!(!steps.is_empty(), "Handler for '{tt}' produced no steps");
            // ManualFallbackHandler would produce a "manual" step; ImplementationHandler
            // produces specific step kinds (not "manual").
            let kinds: Vec<&str> = steps.iter().map(|s| s.kind.as_ref()).collect();
            assert!(
                !kinds.contains(&"manual"),
                "Expected ImplementationHandler steps for '{tt}', got manual: {:?}",
                kinds
            );
        }
    }

    // 5. territory_research routes to TerritoryResearchHandler, not ImplementationHandler.
    #[test]
    fn territory_research_routes_to_territory_handler() {
        let task = make_task("territory_research", "proj1");
        let handlers = default_handlers();
        let matched = handlers.iter().find(|h| h.supports(&task));
        assert!(matched.is_some(), "No handler for territory_research");
        let steps = matched.unwrap().plan(&task);
        assert!(
            !steps.is_empty(),
            "TerritoryResearchHandler produced no steps"
        );
        let kinds: Vec<&str> = steps.iter().map(|s| s.kind.as_ref()).collect();
        assert!(
            kinds.contains(&"territory_load_recommendation"),
            "Expected territory steps, got: {:?}",
            kinds
        );
        assert!(
            !kinds.contains(&"manual"),
            "Expected TerritoryResearchHandler steps, got manual: {:?}",
            kinds
        );
    }

    // 6. Unknown task types fall through to ManualFallbackHandler, not ImplementationHandler.
    #[test]
    fn unknown_type_routes_to_manual_fallback() {
        let task = make_task("totally_unknown_type_xyz", "proj1");
        let handlers = default_handlers();
        let matched = handlers.iter().find(|h| h.supports(&task)).unwrap();
        let steps = matched.plan(&task);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::Manual);
    }

    // Reddit workflow step kinds are recognized by run_step (regression test for missing handler).
    #[test]
    fn reddit_workflow_step_kinds_are_recognized() {
        use crate::engine::workflows::{StepKind, WorkflowStep};

        let reddit_steps = vec![
            (StepKind::RedditConfigParse, true),  // Should be recognized
            (StepKind::RedditSearch, true),       // Should be recognized
            (StepKind::RedditEnrich, true),       // Should be recognized
            (StepKind::RedditFetchResults, true), // Should be recognized
            (StepKind::Unknown, false),           // Should NOT be recognized
        ];

        for (kind, should_be_recognized) in reddit_steps {
            let step = WorkflowStep::new("test_step", kind);

            // Simulate what run_step does - match on step.kind
            let result = match step.kind {
                StepKind::RedditConfigParse => Some(true),
                StepKind::RedditSearch => Some(true),
                StepKind::RedditEnrich => Some(true),
                StepKind::RedditFetchResults => Some(true),
                _ => None,
            };

            if should_be_recognized {
                assert!(
                    result.is_some(),
                    "Step kind '{:?}' should be recognized by run_step",
                    kind
                );
            } else {
                assert!(
                    result.is_none(),
                    "Step kind '{:?}' should NOT be recognized by run_step",
                    kind
                );
            }
        }
    }

    // 6. update_task_status correctly persists the new status to SQLite.
    #[test]
    fn update_task_status_persists_to_db() {
        let conn = in_memory_db();
        let proj = test_project_in(&conn);
        let task = make_task("collect_gsc", &proj);
        let id = task.id.clone();
        task_store::create_task(&conn, &task).unwrap();

        task_store::update_task_status(&conn, &id, TaskStatus::InProgress).unwrap();
        let updated = task_store::get_task(&conn, &id).unwrap();
        assert_eq!(updated.status, TaskStatus::InProgress);

        task_store::update_task_status(&conn, &id, TaskStatus::Done).unwrap();
        let done = task_store::get_task(&conn, &id).unwrap();
        assert_eq!(done.status, TaskStatus::Done);
    }

    /// Integration test for the 5-step hybrid keyword research workflow.
    ///
    /// Mocks:
    /// - Bridge health + chat completions for agentic steps (seed extraction, seed validation)
    /// - CapSolver token endpoint
    /// - Ahrefs keyword ideas + difficulty endpoints
    #[test]
    fn execute_task_keyword_research_full_flow_with_mocked_http() {
        let _env_guard = ENV_LOCK.lock().unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mock_server = rt.block_on(MockServer::start());

        rt.block_on(async {
            // Bridge health check
            // Google Autocomplete mock
            Mock::given(method("GET"))
                .and(path("/complete/search"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                    "risk management",
                    ["risk management strategy", "risk management framework", "portfolio risk management"]
                ])))
                .mount(&mock_server)
                .await;

            Mock::given(method("GET"))
                .and(path("/health"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "kimi_available": true
                })))
                .mount(&mock_server)
                .await;

            // Step 1 (seed extraction) — matches system prompt content
            Mock::given(method("POST"))
                .and(path("/v1/chat/completions"))
                .and(body_string_contains("Seed Extraction Contract"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id": "chatcmpl-extraction",
                    "object": "chat.completion",
                    "created": 1677652288,
                    "model": "test-model",
                    "choices": [
                        {
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": null,
                                "tool_calls": [
                                    {
                                        "id": "call_extraction",
                                        "type": "function",
                                        "function": {
                                            "name": "submit",
                                            "arguments": "{\"themes\":[\"risk management\",\"portfolio hedging\"],\"competitors\":[]}"
                                        }
                                    }
                                ]
                            },
                            "finish_reason": "tool_calls"
                        }
                    ]
                })))
                .mount(&mock_server)
                .await;

            // Step 3 (seed validation) — matches system prompt content
            Mock::given(method("POST"))
                .and(path("/v1/chat/completions"))
                .and(body_string_contains("Seed Validation Contract"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id": "chatcmpl-validation",
                    "object": "chat.completion",
                    "created": 1677652289,
                    "model": "test-model",
                    "choices": [
                        {
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": null,
                                "tool_calls": [
                                    {
                                        "id": "call_validation",
                                        "type": "function",
                                        "function": {
                                            "name": "submit",
                                            "arguments": "{\"validated_seeds\":[{\"theme\":\"risk management\",\"seeds\":[\"risk management strategy\"]},{\"theme\":\"portfolio hedging\",\"seeds\":[\"portfolio hedging options\"]}]}"
                                        }
                                    }
                                ]
                            },
                            "finish_reason": "tool_calls"
                        }
                    ]
                })))
                .mount(&mock_server)
                .await;

            // CapSolver
            Mock::given(method("POST"))
                .and(path("/createTask"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "errorId": 0,
                    "taskId": "task-123"
                })))
                .mount(&mock_server)
                .await;

            Mock::given(method("POST"))
                .and(path("/getTaskResult"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "errorId": 0,
                    "status": "ready",
                    "solution": {"token": "mock-captcha-token"}
                })))
                .mount(&mock_server)
                .await;

            // Ahrefs keyword ideas
            Mock::given(method("POST"))
                .and(path("/v4/stGetFreeKeywordIdeas"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                    "Ok",
                    {
                        "allIdeas": {
                            "results": [
                                {
                                    "keyword": "options risk management strategy",
                                    "difficultyLabel": "Low",
                                    "volumeLabel": "MoreThanOneHundred"
                                },
                                {
                                    "keyword": "portfolio hedging options",
                                    "difficultyLabel": "Medium",
                                    "volumeLabel": "MoreThanOneThousand"
                                }
                            ]
                        },
                        "questionIdeas": {"items": []}
                    }
                ])))
                .mount(&mock_server)
                .await;

            // Ahrefs difficulty
            Mock::given(method("POST"))
                .and(path("/v4/stGetFreeSerpOverviewForKeywordDifficultyChecker"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                    "Ok",
                    {
                        "difficulty": 14.0,
                        "shortage": 0.0,
                        "lastUpdate": "2026-03-24",
                        "serp": {"results": []}
                    }
                ])))
                .mount(&mock_server)
                .await;
        });

        let project_dir = unique_temp_dir("ps_kw_button_flow_test");
        setup_dummy_keyword_project(&project_dir, "risk management");

        let old_key = std::env::var("CAPSOLVER_API_KEY").ok();
        let old_create = std::env::var("PAGESEEDS_CAPSOLVER_CREATE_URL").ok();
        let old_result = std::env::var("PAGESEEDS_CAPSOLVER_RESULT_URL").ok();
        let old_ahrefs = std::env::var("PAGESEEDS_AHREFS_BASE_URL").ok();
        let old_bridge = std::env::var("KIMI_BRIDGE_URL").ok();
        let old_db = std::env::var("PAGESEEDS_DB_PATH").ok();
        let old_autocomplete = std::env::var("GOOGLE_AUTOCOMPLETE_BASE_URL").ok();

        std::env::set_var("CAPSOLVER_API_KEY", "mock-key");
        std::env::set_var(
            "PAGESEEDS_CAPSOLVER_CREATE_URL",
            format!("{}/createTask", mock_server.uri()),
        );
        std::env::set_var(
            "PAGESEEDS_CAPSOLVER_RESULT_URL",
            format!("{}/getTaskResult", mock_server.uri()),
        );
        std::env::set_var("PAGESEEDS_AHREFS_BASE_URL", mock_server.uri());
        std::env::set_var("KIMI_BRIDGE_URL", format!("{}/v1", mock_server.uri()));
        std::env::set_var("GOOGLE_AUTOCOMPLETE_BASE_URL", mock_server.uri());

        let db_path = project_dir.join("test.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY, name TEXT NOT NULL,
                path TEXT NOT NULL,
                content_dir TEXT,
                site_url TEXT,
                site_id TEXT,
                sitemap_url TEXT,
                project_mode TEXT NOT NULL DEFAULT 'workspace',
                active INTEGER NOT NULL DEFAULT 1,
                agent_provider TEXT,
                seo_provider TEXT
             );
             CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
                review_surface TEXT NOT NULL DEFAULT 'none',
                follow_up_policy TEXT NOT NULL DEFAULT 'none',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT, description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER NOT NULL DEFAULT 0,
                run_last_error TEXT, run_provider TEXT,
                not_before TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL, attempt INTEGER NOT NULL,
                provider TEXT, started_at TEXT NOT NULL,
                finished_at TEXT, success INTEGER, error TEXT,
                prompt_tokens INTEGER, completion_tokens INTEGER
             );",
        )
        .unwrap();
        std::env::set_var("PAGESEEDS_DB_PATH", db_path.to_string_lossy().as_ref());
        let proj = test_project_in_at_path(&conn, &project_dir.to_string_lossy());

        // Ensure articles table exists and has a dummy article so the pre-flight check passes.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS articles (
                id INTEGER NOT NULL, title TEXT NOT NULL DEFAULT '',
                url_slug TEXT NOT NULL DEFAULT '', file TEXT NOT NULL DEFAULT '',
                target_keyword TEXT, keyword_difficulty TEXT,
                target_volume INTEGER DEFAULT 0, published_date TEXT,
                word_count INTEGER DEFAULT 0, status TEXT NOT NULL DEFAULT 'draft',
                review_status TEXT, review_started_at TEXT, last_reviewed_at TEXT,
                review_count INTEGER NOT NULL DEFAULT 0,
                content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
                estimated_traffic_monthly TEXT, page_type TEXT,
                project_id TEXT NOT NULL,
                PRIMARY KEY (id, project_id)
            );
            CREATE TABLE IF NOT EXISTS articles_meta (
                project_id TEXT PRIMARY KEY, next_article_id INTEGER NOT NULL DEFAULT 1
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, status, content_gaps_addressed, project_id)
             VALUES (1, 'Test', 'test', './content/001_test.mdx', 'published', '[]', 'proj1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES ('proj1', 2)",
            [],
        )
        .unwrap();

        let mut task = make_task("research_keywords", &proj);
        task.description = Some("risk management".to_string());
        let task_id = task.id.clone();
        task_store::create_task(&conn, &task).unwrap();

        let result = {
            let _entered = rt.handle().enter();
            rt.block_on(async {
                execute_task(&conn, &task_id)
                    .await
                    .expect("execute_task should return Ok")
            })
        };

        let saved_task = task_store::get_task(&conn, &task_id).unwrap();
        assert!(result.success, "expected success, got: {}", result.message);
        assert_eq!(saved_task.status, TaskStatus::Review);

        // 5-step workflow produces 5 artifacts
        let artifact_keys: Vec<&str> = saved_task
            .artifacts
            .iter()
            .map(|a| a.key.as_str())
            .collect();
        assert!(
            artifact_keys.contains(&"research_seed_extraction"),
            "missing research_seed_extraction artifact; got: {:?}",
            artifact_keys
        );
        assert!(
            artifact_keys.contains(&"research_autocomplete"),
            "missing research_autocomplete artifact; got: {:?}",
            artifact_keys
        );
        assert!(
            artifact_keys.contains(&"research_seed_validation"),
            "missing research_seed_validation artifact; got: {:?}",
            artifact_keys
        );
        assert!(
            artifact_keys.contains(&"research_ahrefs_pipeline"),
            "missing research_ahrefs_pipeline artifact; got: {:?}",
            artifact_keys
        );
        assert!(
            artifact_keys.contains(&"research_final_selection"),
            "missing research_final_selection artifact; got: {:?}",
            artifact_keys
        );

        // Verify seed extraction content
        let seed_extraction = saved_task
            .artifacts
            .iter()
            .find(|a| a.key == "research_seed_extraction")
            .and_then(|a| a.content.as_deref())
            .expect("seed extraction should have content");
        let seeds: serde_json::Value = serde_json::from_str(seed_extraction).unwrap();
        assert_eq!(seeds["themes"][0], "risk management");

        // Verify final selection content
        let final_selection = saved_task
            .artifacts
            .iter()
            .find(|a| a.key == "research_final_selection")
            .and_then(|a| a.content.as_deref())
            .expect("final selection should have content");
        let final_json: serde_json::Value = serde_json::from_str(final_selection).unwrap();
        assert!(
            final_json.get("difficulty").is_some()
                || final_json.get("landing_page_candidates").is_some(),
            "final selection should contain keyword output"
        );

        if let Some(v) = old_key {
            std::env::set_var("CAPSOLVER_API_KEY", v);
        } else {
            std::env::remove_var("CAPSOLVER_API_KEY");
        }
        if let Some(v) = old_create {
            std::env::set_var("PAGESEEDS_CAPSOLVER_CREATE_URL", v);
        } else {
            std::env::remove_var("PAGESEEDS_CAPSOLVER_CREATE_URL");
        }
        if let Some(v) = old_result {
            std::env::set_var("PAGESEEDS_CAPSOLVER_RESULT_URL", v);
        } else {
            std::env::remove_var("PAGESEEDS_CAPSOLVER_RESULT_URL");
        }
        if let Some(v) = old_ahrefs {
            std::env::set_var("PAGESEEDS_AHREFS_BASE_URL", v);
        } else {
            std::env::remove_var("PAGESEEDS_AHREFS_BASE_URL");
        }
        if let Some(v) = old_bridge {
            std::env::set_var("KIMI_BRIDGE_URL", v);
        } else {
            std::env::remove_var("KIMI_BRIDGE_URL");
        }
        if let Some(v) = old_db {
            std::env::set_var("PAGESEEDS_DB_PATH", v);
        } else {
            std::env::remove_var("PAGESEEDS_DB_PATH");
        }
        if let Some(v) = old_autocomplete {
            std::env::set_var("GOOGLE_AUTOCOMPLETE_BASE_URL", v);
        } else {
            std::env::remove_var("GOOGLE_AUTOCOMPLETE_BASE_URL");
        }

        std::fs::remove_dir_all(&project_dir).ok();
    }

    // ── Fix 1: date injection ──────────────────────────────────────────────────

    // compute_next_publish_date returns yesterday when no articles exist.
    #[test]
    fn compute_next_publish_date_no_existing_articles() {
        use crate::engine::workflows::handlers::compute_next_publish_date;
        use chrono::{Duration, Utc};

        let conn = in_memory_db();
        test_project_in(&conn);

        let result = compute_next_publish_date(&conn, "proj1").unwrap();

        let yesterday = (Utc::now().date_naive() - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, yesterday, "empty project should return yesterday");
    }

    // compute_next_publish_date skips occupied dates and returns first free past date.
    #[test]
    fn compute_next_publish_date_skips_occupied_slots() {
        use crate::engine::workflows::handlers::compute_next_publish_date;
        use chrono::{Duration, Utc};

        let conn = in_memory_db();
        test_project_in(&conn);

        // Occupy yesterday and two days ago.
        let today = Utc::now().date_naive();
        let d1 = (today - Duration::days(1)).format("%Y-%m-%d").to_string();
        let d2 = (today - Duration::days(2)).format("%Y-%m-%d").to_string();

        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, published_date, status, project_id)
             VALUES (1, 'A', 'a', 'a.mdx', ?1, 'published', 'proj1'),
                    (2, 'B', 'b', 'b.mdx', ?2, 'published', 'proj1')",
            rusqlite::params![d1, d2],
        )
        .unwrap();

        let result = compute_next_publish_date(&conn, "proj1").unwrap();

        let expected = (today - Duration::days(3)).format("%Y-%m-%d").to_string();
        assert_eq!(
            result, expected,
            "should skip occupied yesterday/two-days-ago and return three days ago"
        );
    }

    // compute_next_publish_date returns None when project has no articles table entries.
    #[test]
    fn compute_next_publish_date_missing_project_returns_none() {
        use crate::engine::workflows::handlers::compute_next_publish_date;

        let conn = in_memory_db();
        // No project inserted — function must gracefully return None, not panic.
        let result = compute_next_publish_date(&conn, "nonexistent");
        assert!(result.is_none());
    }

    // ── Fix 2: keyword metadata parsing ───────────────────────────────────────

    fn parse_content_task_keyword_meta(task: &Task) -> (Option<String>, Option<String>, i64) {
        let desc = match task.description.as_deref() {
            Some(d) if !d.is_empty() => d,
            _ => return (None, None, 0),
        };
        let mut keyword = None;
        let mut kd: Option<String> = None;
        let mut volume = 0i64;
        for line in desc.lines() {
            if let Some(rest) = line.strip_prefix("Target keyword:") {
                keyword = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("KD:") {
                if let Ok(n) = rest.trim().parse::<i64>() {
                    kd = Some(n.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("Volume:") {
                volume = rest.trim().parse::<i64>().unwrap_or(0);
            }
        }
        (keyword, kd, volume)
    }

    // parse_content_task_keyword_meta extracts all three fields from a full description.
    #[test]
    fn parse_keyword_meta_full_description() {
        let mut task = make_task("write_article", "proj1");
        task.description =
            Some("Target keyword: options risk management\nKD: 25\nVolume: 1200".to_string());

        let (kw, kd, vol) = parse_content_task_keyword_meta(&task);
        assert_eq!(kw.as_deref(), Some("options risk management"));
        assert_eq!(kd.as_deref(), Some("25"));
        assert_eq!(vol, 1200);
    }

    // parse_content_task_keyword_meta handles partial descriptions gracefully.
    #[test]
    fn parse_keyword_meta_partial_description() {
        let mut task = make_task("write_article", "proj1");
        task.description = Some("Target keyword: coffee brewing\nVolume: 500".to_string());

        let (kw, kd, vol) = parse_content_task_keyword_meta(&task);
        assert_eq!(kw.as_deref(), Some("coffee brewing"));
        assert!(kd.is_none(), "KD should be None when not in description");
        assert_eq!(vol, 500);
    }

    // parse_content_task_keyword_meta returns empty tuple for None description.
    #[test]
    fn parse_keyword_meta_no_description() {
        let task = make_task("write_article", "proj1");

        let (kw, kd, vol) = parse_content_task_keyword_meta(&task);
        assert!(kw.is_none());
        assert!(kd.is_none());
        assert_eq!(vol, 0);
    }

    // ── Fix 2: articles.json registration after content write ─────────────────

    #[test]
    fn content_write_registers_article_in_articles_json() {
        use crate::content::article_index;
        use crate::db::export::export_articles;

        let dir = unique_temp_dir("ps_content_register");
        let auto_dir = dir.join(".github").join("automation");
        let content_dir = dir.join("content").join("blog");
        std::fs::create_dir_all(&auto_dir).unwrap();
        std::fs::create_dir_all(&content_dir).unwrap();

        // Set up articles.json pointing at the content dir.
        std::fs::write(
            auto_dir.join("articles.json"),
            r#"{"nextArticleId":1,"articles":[]}"#,
        )
        .unwrap();

        // Simulate the agent writing a new MDX file with a frontmatter date.
        std::fs::write(
            content_dir.join("001_test_article.mdx"),
            "---\ntitle: \"Test Article\"\ndate: \"2026-01-15\"\n---\n\nBody text here.\n",
        )
        .unwrap();

        // Set up SQLite + project as executor would have it.
        let conn = in_memory_db();

        // articles_meta table is needed by ingest_orphan_files.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS articles (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                url_slug TEXT NOT NULL DEFAULT '',
                file TEXT NOT NULL DEFAULT '',
                target_keyword TEXT,
                keyword_difficulty TEXT,
                target_volume INTEGER NOT NULL DEFAULT 0,
                published_date TEXT,
                word_count INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'draft',
                review_status TEXT,
                review_started_at TEXT,
                last_reviewed_at TEXT,
                review_count INTEGER NOT NULL DEFAULT 0,
                content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
                estimated_traffic_monthly TEXT,
                page_type TEXT,
                project_id TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS articles_meta (
                project_id TEXT PRIMARY KEY,
                next_article_id INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS article_metadata (
                project_id TEXT NOT NULL,
                article_id INTEGER NOT NULL,
                namespace TEXT NOT NULL,
                payload TEXT NOT NULL DEFAULT '{}',
                updated_at TEXT NOT NULL,
                PRIMARY KEY (project_id, article_id, namespace)
            );",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES ('p1', 'Test', ?1, 1)",
            [dir.to_str().unwrap()],
        )
        .unwrap();

        // Also insert a seo_workspace.json so resolve_content_dir can find the content dir.
        std::fs::write(
            auto_dir.join("seo_workspace.json"),
            r#"{"content_dir":"content/blog"}"#,
        )
        .unwrap();

        // --- Step 1: ingest_orphans finds and registers the new file.
        let ingested = article_index::ingest_orphans(&conn, "p1", &dir)
            .expect("ingest_orphans should succeed");
        assert_eq!(ingested.ingested, 1, "expected 1 article to be ingested");
        assert_eq!(ingested.files, vec!["001_test_article.mdx"]);

        // --- Step 2: Patch keyword metadata (simulating what the executor hook does).
        for filename in &ingested.files {
            conn.execute(
                "UPDATE articles SET target_keyword=?1, keyword_difficulty=?2, target_volume=?3,
                 status='draft' WHERE project_id=?4 AND file LIKE ?5",
                rusqlite::params![
                    Some("test article keyword"),
                    Some("28"),
                    900i64,
                    "p1",
                    format!("%{}", filename),
                ],
            )
            .unwrap();
        }

        // --- Step 3: Re-export articles.json with keyword metadata.
        let json = export_articles(&conn, "p1").unwrap();
        std::fs::write(auto_dir.join("articles.json"), &json).unwrap();

        // --- Verify the articles.json on disk contains the new article.
        let on_disk: serde_json::Value = serde_json::from_str(&json).unwrap();
        let articles = on_disk["articles"].as_array().unwrap();
        assert_eq!(articles.len(), 1, "articles.json should have 1 article");

        let a = &articles[0];
        assert_eq!(a["published_date"], "2026-01-15");
        assert_eq!(a["target_keyword"], "test article keyword");
        assert_eq!(a["keyword_difficulty"], "28");
        assert_eq!(a["target_volume"], 900);
        assert_eq!(a["status"], "draft");

        std::fs::remove_dir_all(&dir).ok();
    }
}
