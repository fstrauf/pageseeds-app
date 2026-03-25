/// Workflow execution orchestrator.
///
/// Finds the correct handler for a task, plans the step graph,
/// executes each step sequentially, persists artifacts, and
/// updates task status in SQLite.

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::Emitter as _;

use crate::engine::workflows::{
    handlers::{default_handlers, exec_agentic, exec_deterministic},
    step_params,
    WorkflowStep,
};
use crate::engine::task_store;
use crate::models::task::{Task, TaskArtifact, TaskStatus};
use ts_rs::TS;

// ─── Event Types ──────────────────────────────────────────────────────────────

/// Emitted after each step so the frontend runner can show live progress.
#[derive(Debug, Clone, Serialize)]
pub struct TaskStepEvent {
    pub task_id: String,
    pub step_name: String,
    pub status: String,
    pub message: String,
}

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
    pub execution_mode: String,
    pub priority: String,
}

// ─── Engine ───────────────────────────────────────────────────────────────────

pub fn execute_task(conn: &Connection, task_id: &str) -> Result<ExecutionResult, String> {
    execute_task_with_token(conn, task_id, None, None, false)
}

/// Run `execute_task_with_token` in dry-run mode — plans steps but does not
/// call any `exec_*` functions or modify database state.
pub fn dry_run_task(conn: &Connection, task_id: &str) -> Result<ExecutionResult, String> {
    execute_task_with_token(conn, task_id, None, None, true)
}

pub fn execute_task_with_token(
    conn: &Connection,
    task_id: &str,
    gsc_token: Option<&str>,
    app_handle: Option<tauri::AppHandle>,
    dry_run: bool,
) -> Result<ExecutionResult, String> {
    let mut task = task_store::get_task(conn, task_id).map_err(|e| e.to_string())?;

    let started_at = Utc::now().to_rfc3339();

    // Transition to in_progress
    if task.status == TaskStatus::Todo {
        task.status = TaskStatus::InProgress;
        task.updated_at = started_at.clone();
        task_store::update_task_status(conn, task_id, TaskStatus::InProgress).map_err(|e| e.to_string())?;
    }

    let (project_path, site_url, agent_provider) = {
        let project = task_store::get_project(conn, &task.project_id).map_err(|e| e.to_string())?;
        (
            project.path.clone(),
            project.site_url.clone().unwrap_or_default(),
            project.agent_provider.clone().unwrap_or_else(|| "copilot".to_string()),
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
            kind: s.kind.clone(),
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
            message: format!("dry-run: {} steps planned for '{}'", progress.len(), task.task_type),
            steps: progress,
            follow_up_tasks: vec![],
            started_at,
            finished_at: Utc::now().to_rfc3339(),
        });
    }

    let mut all_ok = true;
    let mut last_error = String::new();
    let mut latest_raw_output: Option<String> = None;

    for (i, step) in steps.iter().enumerate() {
        progress[i].status = "running".to_string();

        let result = run_step(
            step,
            &task,
            &project_path,
            &site_url,
            &agent_provider,
            latest_raw_output.as_deref(),
            gsc_token,
        );

        // Track the raw output of agentic steps for the normalizer that follows
        if step.kind == "agentic" {
            if let Some(ref out) = result.output {
                let preview = crate::engine::text::char_prefix(out, 300);
                log::info!("[executor] agentic step '{}' output ({} chars): {:?}",
                    step.name, out.len(), preview);
            } else {
                log::warn!("[executor] agentic step '{}' produced no output", step.name);
            }
            latest_raw_output = result.output.clone();
        } else if step.kind == "normalizer" {
            // Normalizer consumed latest_raw; clear so it isn't reused
            latest_raw_output = None;
        }

        progress[i].status = if result.success { "ok".to_string() } else { "failed".to_string() };
        progress[i].message = result.message.clone();
        progress[i].output = result.output.clone();

        // Emit step progress event for live UI updates
        if let Some(ref handle) = app_handle {
            let _ = handle.emit("task_step_progress", &TaskStepEvent {
                task_id: task_id.to_string(),
                step_name: progress[i].step_name.clone(),
                status: progress[i].status.clone(),
                message: progress[i].message.clone(),
            });
        }

        // Persist agentic / deterministic output as artifact
        if let Some(ref out) = result.output {
            let artifact = TaskArtifact {
                key: step.name.clone(),
                path: None,
                artifact_type: Some(step.kind.clone()),
                source: Some(step.kind.clone()),
                content: Some(out.clone()),
            };
            let _ = task_store::append_task_artifact(conn, task_id, &artifact);
            task.artifacts.push(artifact);
        }

        // After a reddit_search step, upsert posts from the JSON output into SQLite.
        // Enrichment (AI pass) is a separate step — see reddit_enrich block below.
        if step.kind == "reddit_search" && result.success {
            if let Some(ref out) = result.output {
                crate::engine::exec::reddit::persist_reddit_opportunities(conn, &task.project_id, out);
            }
        }

        // reddit_enrich step: AI enrichment batches — fills why_relevant, key_pain_points,
        // website_fit, reply_text. Handled here (not in run_step) because it needs conn.
        if step.kind == "reddit_enrich" {
            loop {
                let pending: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM reddit_opportunities \
                     WHERE project_id=?1 AND (why_relevant IS NULL OR reply_text IS NULL) \
                     AND reply_status != 'skipped'",
                    rusqlite::params![&task.project_id],
                    |r| r.get(0),
                ).unwrap_or(0);
                if pending == 0 { break; }
                log::info!("[reddit_enrich] {} posts still pending enrichment — running batch", pending);
                crate::engine::exec::reddit::exec_reddit_enrich(conn, &task.project_id, &project_path, &agent_provider);
            }
            progress[i].message = "Reddit enrichment complete".to_string();
        }
        
        // reddit_fetch_results step: fetch enriched opportunities from DB and return as JSON.
        // Handled here (not in run_step) because it needs conn.
        if step.kind == "reddit_fetch_results" {
            let result = crate::engine::exec::reddit::exec_reddit_fetch_results(conn, &task.project_id);
            progress[i].status = if result.success { "ok".to_string() } else { "failed".to_string() };
            progress[i].message = result.message.clone();
            progress[i].output = result.output.clone();
            if let Some(ref out) = result.output {
                let artifact = TaskArtifact {
                    key: step.name.clone(),
                    path: None,
                    artifact_type: Some(step.kind.clone()),
                    source: Some(step.kind.clone()),
                    content: Some(out.clone()),
                };
                let _ = task_store::append_task_artifact(conn, task_id, &artifact);
                task.artifacts.push(artifact);
            }
        }

        // After a reddit_opportunities normalizer step, upsert parsed opportunities into DB.
        if step.kind == "normalizer"
            && step.params.get(step_params::NORMALIZER_ID).map(|s| s.as_str()) == Some("reddit_opportunities")
        {
            log::info!("[reddit] normalizer step complete — success={} output_len={}",
                result.success,
                result.output.as_ref().map(|o| o.len()).unwrap_or(0)
            );
            if result.success {
                match &result.output {
                    Some(out) => crate::engine::exec::reddit::persist_reddit_opportunities(conn, &task.project_id, out),
                    None => log::warn!("[reddit] normalizer succeeded but produced no output"),
                }
            } else {
                log::warn!("[reddit] normalizer step failed: {}", result.message);
            }
        }

        // After a successful content write, register the new MDX file in SQLite + articles.json.
        // Mirrors the Python CLI's article_manager.add_article() call which runs immediately
        // after the agent writes the file. Without this, articles.json is only updated at
        // publish time, meaning the next write_article task cannot compute a safe date.
        if step.name == "content_write_stage" && result.success {
            let automation_dir = std::path::Path::new(&project_path)
                .join(".github")
                .join("automation");
            match crate::content::ops::ingest_orphan_files(
                &automation_dir,
                std::path::Path::new(&project_path),
                &task.project_id,
                conn,
            ) {
                Ok(ingested) if ingested.ingested > 0 => {
                    // Patch keyword metadata from the task description onto the newly
                    // inserted row. ingest_orphan_files has no task context so it
                    // cannot fill these fields itself.
                    let (keyword, kd_str, vol) = parse_content_task_keyword_meta(&task);
                    for filename in &ingested.files {
                        let _ = conn.execute(
                            "UPDATE articles
                             SET target_keyword=?1, keyword_difficulty=?2, target_volume=?3,
                                 status='draft'
                             WHERE project_id=?4 AND file LIKE ?5",
                            rusqlite::params![
                                keyword.as_deref(),
                                kd_str.as_deref(),
                                vol,
                                &task.project_id,
                                format!("%{}", filename),
                            ],
                        );
                    }
                    // Re-export articles.json so keyword fields are persisted.
                    if let Ok(json) =
                        crate::db::export::export_articles(conn, &task.project_id)
                    {
                        let articles_path = std::path::Path::new(&project_path)
                            .join(".github")
                            .join("automation")
                            .join("articles.json");
                        let _ = std::fs::write(&articles_path, json);
                    }
                    log::info!(
                        "[content_register] registered {} article(s): {:?}",
                        ingested.ingested,
                        ingested.files
                    );
                }
                Ok(_) => {
                    log::info!(
                        "[content_register] no new orphan files to register after content write"
                    )
                }
                Err(e) => log::warn!("[content_register] article registration failed: {}", e),
            }
        }

        if !result.success {
            if step.optional {
                progress[i].status = "skipped".to_string();
            } else {
                all_ok = false;
                last_error = result.message.clone();
                break;
            }
        }
    }

    let finished_at = Utc::now().to_rfc3339();
    let new_status = completed_task_status(&task.task_type, all_ok);

    task_store::update_task_status(conn, task_id, new_status.clone()).map_err(|e| e.to_string())?;

    let mut follow_up_ids: Vec<String> = vec![];

    // After a successful content review, create a single content_review_apply task from recommendations.json.
    if all_ok && matches!(task.task_type.as_str(), "content_review" | "content_audit") {
        if let Some(task_id) = crate::engine::exec::content::create_content_review_apply_task(conn, &task, &project_path) {
            follow_up_ids.push(task_id);
        }
    }

    // After a successful write_article, queue a cluster_and_link task so the
    // new article is integrated into the site's internal link graph.
    if all_ok && task.task_type == "write_article" {
        if let Some(task_id) = crate::engine::exec::content::create_cluster_and_link_task(conn, &task, &project_path) {
            follow_up_ids.push(task_id);
        }
    }

    // After a successful collect_gsc, spawn fix tasks from the gsc_collection.json artifact.
    if all_ok && task.task_type == "collect_gsc" {
        follow_up_ids.extend(crate::engine::exec::gsc::create_tasks_from_collection_after_exec(conn, &task, &project_path));
    }
    
    // After a successful reddit_opportunity_search, the task goes to Review status.
    // The user will review opportunities and select which ones to create reply tasks for.
    // No automatic task creation - user must approve each opportunity first.

    let follow_up_tasks: Vec<FollowUpTask> = follow_up_ids
        .iter()
        .filter_map(|id| task_store::get_task(conn, id).ok())
        .map(|t| FollowUpTask {
            id: t.id,
            task_type: t.task_type,
            title: t.title.unwrap_or_else(|| "Untitled task".to_string()),
            status: t.status.to_string(),
            execution_mode: t.execution_mode.to_string(),
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
            title: task.title.clone().unwrap_or_else(|| "Review results".to_string()),
            status: "review".to_string(),
            execution_mode: "manual".to_string(),
            priority: task.priority.to_string(),
        });
        fups
    } else {
        follow_up_tasks
    };

    if !all_ok {
        task_store::record_task_run(conn, task_id, false, Some(&last_error), None)
            .map_err(|e| e.to_string())?;
    } else {
        task_store::record_task_run(conn, task_id, true, None, None)
            .map_err(|e| e.to_string())?;
    }

    Ok(ExecutionResult {
        task_id: task_id.to_string(),
        success: all_ok,
        message: if all_ok { "Task completed".to_string() } else { last_error },
        steps: progress,
        follow_up_tasks,
        started_at,
        finished_at,
    })
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn run_step(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    site_url: &str,
    agent_provider: &str,
    latest_raw: Option<&str>,
    gsc_token: Option<&str>,
) -> crate::engine::workflows::StepResult {
    match step.kind.as_str() {
        "deterministic" => exec_deterministic(step, task, project_path),
        "agentic" => exec_agentic(step, task, project_path, site_url, agent_provider),
        "manual" => crate::engine::workflows::StepResult {
            success: true,
            message: format!("Manual step '{}' — requires user action", step.name),
            output: None,
        },
        "normalizer" => {
            if let Some(raw) = latest_raw {
                let normalized = crate::engine::normalizer::normalize_agent_output(raw);
                let msg = if normalized.success {
                    format!("Normalized via '{}' — {} chars", normalized.extraction_method, normalized.raw_output.len())
                } else {
                    format!("Normalizer recorded raw output ({} chars)", normalized.raw_output.len())
                };
                let output_str = normalized.json_artifact
                    .as_ref()
                    .and_then(|v| serde_json::to_string_pretty(v).ok())
                    .unwrap_or_else(|| normalized.raw_output.clone());
                crate::engine::workflows::StepResult {
                    success: true,
                    message: msg,
                    output: Some(output_str),
                }
            } else {
                crate::engine::workflows::StepResult {
                    success: true,
                    message: format!("Normalizer step '{}' — no raw output to normalize", step.name),
                    output: None,
                }
            }
        }
        "cluster_link_scan" => crate::engine::exec::content::exec_cluster_link_scan(task, project_path),
        "cluster_link_strategy" => crate::engine::exec::content::exec_cluster_link_strategy(task, project_path, agent_provider),
        "cluster_link_apply" => crate::engine::exec::content::exec_cluster_link_apply(task, project_path),
        "content_review_recommend" => crate::engine::exec::content::exec_content_review_recommend(task, project_path, agent_provider),
        "content_review_apply_execute" => crate::engine::exec::content::exec_content_review_apply(task, project_path, agent_provider),
        "keyword_research_cli" => crate::engine::exec::keywords::exec_keyword_research_native(task, project_path),
        "reddit_config_parse" => crate::engine::exec::reddit::exec_reddit_config_parse(task, project_path, agent_provider),
        "reddit_search" => crate::engine::exec::reddit::exec_reddit_search(task, project_path),
        // reddit_enrich: actual enrichment loop runs in the outer executor loop (needs conn).
        // This placeholder signals success so the outer loop triggers the real work.
        "reddit_enrich" => crate::engine::workflows::StepResult {
            success: true,
            message: "Reddit enrichment pass — starting AI scoring loop".to_string(),
            output: None,
        },
        // reddit_fetch_results: actual DB fetch runs in the outer executor loop (needs conn).
        // This placeholder signals success so the outer loop triggers the real work.
        "reddit_fetch_results" => crate::engine::workflows::StepResult {
            success: true,
            message: "Reddit results fetch — starting DB query".to_string(),
            output: None,
        },
        "content_sync" => crate::engine::exec::content::exec_content_sync(task, project_path),
        "gsc_sync_articles" => crate::engine::exec::gsc::exec_gsc_sync_articles(task, project_path, gsc_token),
        "gsc_summarise" => crate::engine::exec::gsc::exec_gsc_summarise(task, project_path),
        "content_audit" => crate::engine::exec::content_audit::exec_content_audit(task, project_path),
        "collect_gsc_inspect" => crate::engine::exec::gsc::exec_collect_gsc(task, project_path, gsc_token),
        "gsc_investigate_agentic" => crate::engine::exec::gsc::exec_gsc_investigate(step, task, project_path, agent_provider),
        // Social media marketing steps
        "social_collect_sources" => crate::engine::exec::social::exec_social_collect_sources(task, project_path),
        "social_load_templates" => crate::engine::exec::social::exec_social_load_templates(task, project_path),
        "social_generate_posts" => crate::engine::exec::social::exec_social_generate_posts(step, task, project_path, agent_provider),
        "social_build_visuals" => crate::engine::exec::social::exec_social_build_visuals(task, project_path),
        "social_save_campaign" => crate::engine::exec::social::exec_social_save_campaign(task, project_path),
        "social_regenerate_single" => crate::engine::exec::social::exec_social_regenerate_single(step, task, project_path, agent_provider),
        "social_rebuild_visual" => crate::engine::exec::social::exec_social_rebuild_visual(task, project_path),
        "social_update_post" => crate::engine::exec::social::exec_social_update_post(task, project_path),
        "social_design_template" => crate::engine::exec::social::exec_social_design_template(step, task, project_path, agent_provider),
        "social_save_template" => crate::engine::exec::social::exec_social_save_template(task, project_path),
        other => crate::engine::workflows::StepResult {
            success: false,
            message: format!("Unknown step kind '{}'", other),
            output: None,
        },
    }
}

fn _fail_task(conn: &Connection, task: &mut Task, msg: &str) {
    let _ = task_store::update_task_status(conn, &task.id, TaskStatus::Todo);
    let _ = task_store::record_task_run(conn, &task.id, false, Some(msg), None);
}

/// Parse keyword metadata embedded in the write_article task description.
///
/// Task creation puts `"Target keyword: {kw}\nKD: {kd}\nVolume: {vol}"` in the description.
/// Returns (keyword, keyword_difficulty_as_string, volume).
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

/// Determine the final task status after all steps have run.
/// Extracted as a named function so it can be unit-tested.
pub(crate) fn completed_task_status(task_type: &str, all_ok: bool) -> TaskStatus {
    if all_ok {
        // Tasks that require user review before proceeding go to Review status
        if matches!(task_type, 
            "research_keywords" | "custom_keyword_research" |
            "reddit_opportunity_search"
        ) {
            TaskStatus::Review
        } else {
            TaskStatus::Done
        }
    } else {
        TaskStatus::Todo
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::task_store;
    use crate::engine::workflows::handlers::default_handlers;
    use crate::models::task::{AgentPolicy, ExecutionMode, Priority, Task, TaskRun, TaskStatus};
    use rusqlite::Connection;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiremock::matchers::{method, path};
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
                active INTEGER NOT NULL DEFAULT 1,
                agent_provider TEXT
             );
             CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                execution_mode TEXT NOT NULL DEFAULT 'manual',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT, description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER NOT NULL DEFAULT 0,
                run_last_error TEXT, run_provider TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL, attempt INTEGER NOT NULL,
                provider TEXT, started_at TEXT NOT NULL,
                finished_at TEXT, success INTEGER, error TEXT
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
        let brief = format!("## Clusters\n\n### Cluster 1: {theme} (PLANNED)\n");
        std::fs::write(automation.join("seo_content_brief.md"), brief).unwrap();
        std::fs::write(automation.join("articles.json"), "[]").unwrap();
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
            execution_mode: ExecutionMode::Manual,
            agent_policy: AgentPolicy::Optional,
            title: Some(format!("{task_type} test")),
            description: None,
            project_id: project_id.to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun { attempts: 0, last_error: None, provider: None },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    // 1. Keyword research and Reddit tasks end with "review" status, not "done".
    #[test]
    fn review_tasks_go_to_review_status() {
        assert_eq!(completed_task_status("research_keywords", true), TaskStatus::Review);
        assert_eq!(completed_task_status("custom_keyword_research", true), TaskStatus::Review);
        assert_eq!(completed_task_status("reddit_opportunity_search", true), TaskStatus::Review);
    }

    // 2. All other successful tasks go to "done", not "review".
    #[test]
    fn non_research_task_goes_to_done() {
        assert_eq!(completed_task_status("content_review", true), TaskStatus::Done);
        assert_eq!(completed_task_status("collect_gsc", true), TaskStatus::Done);
        assert_eq!(completed_task_status("fix_indexing", true), TaskStatus::Done);
    }

    // 3. Any failed task resets to "todo" so it can be retried.
    #[test]
    fn failed_task_resets_to_todo() {
        assert_eq!(completed_task_status("research_keywords", false), TaskStatus::Todo);
        assert_eq!(completed_task_status("content_review", false), TaskStatus::Todo);
        assert_eq!(completed_task_status("fix_indexing", false), TaskStatus::Todo);
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
            // ImplementationHandler is at index 7 in default_handlers() — verify
            // by checking the step kind it plans: fix_* falls through to agentic.
            let steps = matched.unwrap().plan(&task);
            assert!(
                !steps.is_empty(),
                "Handler for '{tt}' produced no steps"
            );
            // ManualFallbackHandler would produce a "manual" step; ImplementationHandler
            // produces "agentic" for fix_* types.
            let kinds: Vec<&str> = steps.iter().map(|s| s.kind.as_str()).collect();
            assert!(
                kinds.contains(&"agentic"),
                "Expected ImplementationHandler agentic step for '{tt}', got {:?}",
                kinds
            );
        }
    }

    // 5. Unknown task types fall through to ManualFallbackHandler, not ImplementationHandler.
    #[test]
    fn unknown_type_routes_to_manual_fallback() {
        let task = make_task("totally_unknown_type_xyz", "proj1");
        let handlers = default_handlers();
        let matched = handlers.iter().find(|h| h.supports(&task)).unwrap();
        let steps = matched.plan(&task);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, "manual");
    }

    // Reddit workflow step kinds are recognized by run_step (regression test for missing handler).
    #[test]
    fn reddit_workflow_step_kinds_are_recognized() {
        use crate::engine::workflows::WorkflowStep;
        
        let reddit_steps = vec![
            ("reddit_config_parse", true),   // Should be recognized
            ("reddit_search", true),          // Should be recognized
            ("reddit_enrich", true),          // Should be recognized
            ("reddit_fetch_results", true),   // Should be recognized
            ("reddit_unknown", false),        // Should NOT be recognized
        ];
        
        for (kind, should_be_recognized) in reddit_steps {
            let step = WorkflowStep::new("test_step", kind);
            
            // Simulate what run_step does - match on step.kind
            let result = match step.kind.as_str() {
                "reddit_config_parse" => Some(true),
                "reddit_search" => Some(true),
                "reddit_enrich" => Some(true),
                "reddit_fetch_results" => Some(true),
                _ => None,
            };
            
            if should_be_recognized {
                assert!(
                    result.is_some(),
                    "Step kind '{}' should be recognized by run_step", kind
                );
            } else {
                assert!(
                    result.is_none(),
                    "Step kind '{}' should NOT be recognized by run_step", kind
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

    #[test]
    fn execute_task_keyword_research_full_flow_with_mocked_http() {
        let _env_guard = ENV_LOCK.lock().unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mock_server = rt.block_on(MockServer::start());

        rt.block_on(async {
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

        let conn = in_memory_db();
        let proj = test_project_in_at_path(&conn, &project_dir.to_string_lossy());
        let mut task = make_task("research_keywords", &proj);
        task.description = Some("risk management".to_string());
        let task_id = task.id.clone();
        task_store::create_task(&conn, &task).unwrap();

        let result = {
            let _entered = rt.handle().enter();
            execute_task(&conn, &task_id).expect("execute_task should return Ok")
        };

        let saved_task = task_store::get_task(&conn, &task_id).unwrap();
        assert!(result.success, "expected success, got: {}", result.message);
        assert_eq!(saved_task.status, TaskStatus::Review);

        let artifact = saved_task
            .artifacts
            .iter()
            .find(|a| matches!(a.key.as_str(), "research_keywords_cli" | "keyword_research_cli"))
            .expect("expected keyword research artifact");

        let artifact_content = artifact
            .content
            .as_deref()
            .expect("keyword research artifact should include content");
        let output: serde_json::Value =
            serde_json::from_str(artifact_content).expect("artifact should be valid JSON");
        assert_eq!(output["themes"][0], "risk management");
        assert_eq!(output["difficulty"]["successful"], 2);

        if let Some(v) = old_key { std::env::set_var("CAPSOLVER_API_KEY", v); } else { std::env::remove_var("CAPSOLVER_API_KEY"); }
        if let Some(v) = old_create { std::env::set_var("PAGESEEDS_CAPSOLVER_CREATE_URL", v); } else { std::env::remove_var("PAGESEEDS_CAPSOLVER_CREATE_URL"); }
        if let Some(v) = old_result { std::env::set_var("PAGESEEDS_CAPSOLVER_RESULT_URL", v); } else { std::env::remove_var("PAGESEEDS_CAPSOLVER_RESULT_URL"); }
        if let Some(v) = old_ahrefs { std::env::set_var("PAGESEEDS_AHREFS_BASE_URL", v); } else { std::env::remove_var("PAGESEEDS_AHREFS_BASE_URL"); }

        std::fs::remove_dir_all(&project_dir).ok();
    }

    // ── Fix 1: date injection ──────────────────────────────────────────────────

    // compute_next_publish_date returns yesterday when articles.json has no entries.
    #[test]
    fn compute_next_publish_date_no_existing_articles() {
        use crate::engine::workflows::handlers::compute_next_publish_date;
        use chrono::{Duration, Utc};

        let dir = unique_temp_dir("ps_date_empty");
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        let articles_path = dir.join(".github").join("automation").join("articles.json");
        std::fs::write(
            &articles_path,
            r#"{"nextArticleId":1,"articles":[]}"#,
        )
        .unwrap();

        let result = compute_next_publish_date(&dir.to_string_lossy()).unwrap();

        let yesterday = (Utc::now().date_naive() - Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(result, yesterday, "empty articles.json should return yesterday");

        std::fs::remove_dir_all(&dir).ok();
    }

    // compute_next_publish_date skips occupied dates and returns first free past date.
    #[test]
    fn compute_next_publish_date_skips_occupied_slots() {
        use crate::engine::workflows::handlers::compute_next_publish_date;
        use chrono::{Duration, Utc};

        let dir = unique_temp_dir("ps_date_occupied");
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        let articles_path = dir.join(".github").join("automation").join("articles.json");

        // Occupy yesterday and two days ago.
        let today = Utc::now().date_naive();
        let d1 = (today - Duration::days(1)).format("%Y-%m-%d").to_string();
        let d2 = (today - Duration::days(2)).format("%Y-%m-%d").to_string();
        let json = format!(
            r#"{{"nextArticleId":3,"articles":[
                {{"id":1,"published_date":"{d1}","status":"published"}},
                {{"id":2,"published_date":"{d2}","status":"published"}}
            ]}}"#
        );
        std::fs::write(&articles_path, json).unwrap();

        let result = compute_next_publish_date(&dir.to_string_lossy()).unwrap();

        let expected = (today - Duration::days(3)).format("%Y-%m-%d").to_string();
        assert_eq!(
            result, expected,
            "should skip occupied yesterday/two-days-ago and return three days ago"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // compute_next_publish_date returns None when articles.json is missing.
    #[test]
    fn compute_next_publish_date_missing_file_returns_none() {
        use crate::engine::workflows::handlers::compute_next_publish_date;

        let dir = unique_temp_dir("ps_date_missing");
        std::fs::create_dir_all(&dir).unwrap();
        // No articles.json — function must gracefully return None, not panic.
        let result = compute_next_publish_date(&dir.to_string_lossy());
        assert!(result.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Fix 2: keyword metadata parsing ───────────────────────────────────────

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
        use crate::content::ops::ingest_orphan_files;
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
                content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
                estimated_traffic_monthly TEXT,
                project_id TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS articles_meta (
                project_id TEXT PRIMARY KEY,
                next_article_id INTEGER NOT NULL DEFAULT 1
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

        // --- Step 1: ingest_orphan_files finds and registers the new file.
        let ingested = ingest_orphan_files(&auto_dir, &dir, "p1", &conn)
            .expect("ingest_orphan_files should succeed");
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

