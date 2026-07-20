/// Scheduler — rule evaluation, automatic task creation, and background timer.
///
/// Rules are stored in the `scheduler_rules` SQLite table. On each tick the
/// scheduler evaluates all enabled rules for every active project, creates
/// tasks when due, and records the cycle in the ledger.
///
/// The background timer runs on a dedicated Tokio task (spawned once at app
/// startup via `start_background_scheduler`). It replaces launchd/launchctl.
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::engine::{batch, task_store};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerRule {
    pub rule_id: String,
    pub project_id: String,
    pub task_type: String,
    pub action: String, // "create_task" | "reminder_only"
    pub interval_hours: i64,
    pub priority: String,
    pub phase: String,
    pub enabled: bool,
    pub last_run_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DueRuleResult {
    pub rule_id: String,
    pub is_due: bool,
    pub next_due_at: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerCycleResult {
    pub started_at: String,
    pub finished_at: String,
    pub project_id: String,
    pub rules_evaluated: usize,
    pub tasks_created: usize,
    pub errors: Vec<String>,
    pub due_rules: Vec<DueRuleResult>,
}

// ─── SQLite CRUD ──────────────────────────────────────────────────────────────

pub fn list_rules(conn: &Connection, project_id: &str) -> Result<Vec<SchedulerRule>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT rule_id, project_id, task_type, action, interval_hours, priority, phase, enabled, last_run_at
             FROM scheduler_rules WHERE project_id = ?1 ORDER BY phase ASC, task_type ASC",
        )
        .map_err(|e| e.to_string())?;

    let rules: Vec<SchedulerRule> = stmt
        .query_map([project_id], |row| {
            Ok(SchedulerRule {
                rule_id: row.get(0)?,
                project_id: row.get(1)?,
                task_type: row.get(2)?,
                action: row.get(3)?,
                interval_hours: row.get(4)?,
                priority: row.get(5)?,
                phase: row.get(6)?,
                enabled: row.get::<_, i64>(7)? != 0,
                last_run_at: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rules)
}

pub fn upsert_rule(conn: &Connection, rule: &SchedulerRule) -> Result<(), String> {
    conn.execute(
        "INSERT INTO scheduler_rules
             (rule_id, project_id, task_type, action, interval_hours, priority, phase, enabled, last_run_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(rule_id) DO UPDATE SET
             task_type    = excluded.task_type,
             action       = excluded.action,
             interval_hours = excluded.interval_hours,
             priority     = excluded.priority,
             phase        = excluded.phase,
             enabled      = excluded.enabled,
             last_run_at  = excluded.last_run_at",
        rusqlite::params![
            rule.rule_id,
            rule.project_id,
            rule.task_type,
            rule.action,
            rule.interval_hours,
            rule.priority,
            rule.phase,
            rule.enabled as i64,
            rule.last_run_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Seed default scheduler rules for a project if none exist yet.
/// Only creates rules that are not already configured.
pub fn seed_default_rules(conn: &Connection, project_id: &str) -> Result<(), String> {
    let existing = list_rules(conn, project_id)?;
    let existing_types: std::collections::HashSet<&str> =
        existing.iter().map(|r| r.task_type.as_str()).collect();

    let defaults: &[(&str, i64, &str)] = &[
        ("collect_gsc", 24, "high"),
        ("collect_clarity", 24, "high"),
        ("ctr_audit", 168, "medium"),
        ("update_research_shortlist", 168, "medium"),
    ];

    for &(task_type, interval_hours, priority) in defaults {
        if existing_types.contains(task_type) {
            continue;
        }
        let rule_id = format!("seed:{project_id}:{task_type}");
        let rule = SchedulerRule {
            rule_id,
            project_id: project_id.to_string(),
            task_type: task_type.to_string(),
            action: "create_task".to_string(),
            interval_hours,
            priority: priority.to_string(),
            phase: crate::config::default_phase(task_type).to_string(),
            enabled: true,
            last_run_at: None,
        };
        upsert_rule(conn, &rule)?;
    }

    Ok(())
}

pub fn delete_rule(conn: &Connection, rule_id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM scheduler_rules WHERE rule_id = ?1", [rule_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_rule_enabled(conn: &Connection, rule_id: &str, enabled: bool) -> Result<(), String> {
    conn.execute(
        "UPDATE scheduler_rules SET enabled = ?1 WHERE rule_id = ?2",
        rusqlite::params![enabled as i64, rule_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn mark_rule_ran(conn: &Connection, rule_id: &str, now: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE scheduler_rules SET last_run_at = ?1 WHERE rule_id = ?2",
        rusqlite::params![now, rule_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Due evaluation ───────────────────────────────────────────────────────────

fn evaluate_rule(rule: &SchedulerRule, now: DateTime<Utc>) -> DueRuleResult {
    if !rule.enabled {
        return DueRuleResult {
            rule_id: rule.rule_id.clone(),
            is_due: false,
            next_due_at: now.to_rfc3339(),
            reason: "Rule disabled".to_string(),
        };
    }

    let anchor: DateTime<Utc> = match &rule.last_run_at {
        Some(s) => s
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| now - Duration::hours(rule.interval_hours + 1)),
        None => now - Duration::hours(rule.interval_hours + 1), // never run → treat as overdue
    };

    let next_due = anchor + Duration::hours(rule.interval_hours);
    let is_due = now >= next_due;

    DueRuleResult {
        rule_id: rule.rule_id.clone(),
        is_due,
        next_due_at: next_due.to_rfc3339(),
        reason: if is_due {
            "Due for execution".to_string()
        } else {
            format!("Next due at {}", next_due.to_rfc3339())
        },
    }
}

// ─── Cycle runner (called by background timer or manually) ────────────────────

/// Run a scheduler cycle synchronously.
/// This function is sync because SQLite connections are not Send.
/// The batch execution inside uses its own runtime.
pub fn run_cycle(conn: &Connection, project_id: &str) -> Result<SchedulerCycleResult, String> {
    let started_at = Utc::now().to_rfc3339();
    let now = Utc::now();

    let rules = list_rules(conn, project_id)?;
    let mut tasks_created = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let mut due_rules: Vec<DueRuleResult> = Vec::new();

    for rule in &rules {
        let result = evaluate_rule(rule, now);
        if result.is_due && rule.action == "create_task" {
            // Create a task for this rule
            match create_task_for_rule(conn, rule) {
                Ok(_) => {
                    tasks_created += 1;
                    if let Err(e) = mark_rule_ran(conn, &rule.rule_id, &now.to_rfc3339()) {
                        errors.push(format!("mark_rule_ran {}: {}", rule.rule_id, e));
                    }
                }
                Err(e) => errors.push(format!("create task for rule {}: {}", rule.rule_id, e)),
            }
        }
        due_rules.push(result);
    }

    let cycle_result = SchedulerCycleResult {
        started_at: started_at.clone(),
        finished_at: Utc::now().to_rfc3339(),
        project_id: project_id.to_string(),
        rules_evaluated: rules.len(),
        tasks_created,
        errors: errors.clone(),
        due_rules,
    };

    // Kick off batch if any tasks were created
    if tasks_created > 0 {
        log::info!("[scheduler] {tasks_created} tasks created — triggering batch for {project_id}");
        // Create a new runtime for the async batch execution
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        match rt.block_on(async {
            batch::run_batch(conn, project_id, &batch::BatchConfig::default()).await
        }) {
            Ok(batch_result) => {
                log::info!(
                    "[scheduler] batch complete: {} processed",
                    batch_result.processed
                );
            }
            Err(e) => {
                log::warn!("[scheduler] batch failed: {e}");
            }
        }
    }

    Ok(cycle_result)
}

fn create_task_for_rule(conn: &Connection, rule: &SchedulerRule) -> Result<String, String> {
    use crate::engine::spawner::{TaskSpawner, TaskSpec};
    use crate::models::task::{AgentPolicy, Priority};

    let priority_enum = match rule.priority.as_str() {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };

    let idempotency_key = format!("scheduler:{}:{}", rule.rule_id, Utc::now().format("%Y%m%d"));

    let spec = TaskSpec {
        project_id: rule.project_id.clone(),
        task_type: rule.task_type.clone(),
        title: Some(format!("Scheduled: {}", rule.task_type.replace('_', " "))),
        description: Some(format!("Auto-created by scheduler rule '{}'", rule.rule_id)),
        priority: priority_enum,
        agent_policy: AgentPolicy::None,
        idempotency_key: Some(idempotency_key),
        ..Default::default()
    };

    let task = TaskSpawner::spawn(conn, spec).map_err(|e| e.to_string())?;
    Ok(task.id)
}

// ─── Background timer ─────────────────────────────────────────────────────────

/// Shared state for the background scheduler.
pub struct SchedulerState {
    pub db_path: std::path::PathBuf,
    pub interval_secs: u64,
    pub running: bool,
}

/// Spawn a background Tokio task that runs `run_cycle` for all active projects
/// at a configurable interval (default every 3600 s = 1 hour).
///
/// `db_path` must be the same database file opened by the main app state.
pub fn start_background_scheduler(
    db_path: std::path::PathBuf,
    interval_secs: u64,
) -> Arc<Mutex<SchedulerState>> {
    let state = Arc::new(Mutex::new(SchedulerState {
        db_path: db_path.clone(),
        interval_secs,
        running: true,
    }));

    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
        interval.tick().await; // skip the first immediate tick

        loop {
            interval.tick().await;

            let (db_path, running) = match state_clone.lock() {
                Ok(g) => (g.db_path.clone(), g.running),
                Err(_) => break,
            };

            if !running {
                break;
            }

            // `run_cycle` creates its own Tokio runtime (`Runtime::new().block_on()`)
            // for the batch execution, which panics if invoked from within an async
            // context ("Cannot start a runtime from within a runtime"). Blocking
            // threads have no entered runtime, so running the tick work via
            // `spawn_blocking` keeps this legal. The std Mutex guard is dropped
            // above, before the `.await`.
            let tick_result = tokio::task::spawn_blocking(move || {
                // Open a fresh connection for this tick (the main connection is Mutex-locked in AppState)
                match rusqlite::Connection::open(&db_path) {
                    Ok(conn) => {
                        if let Ok(projects) = task_store::list_projects_raw(&conn) {
                            for project_id in projects {
                                if let Err(e) = run_cycle(&conn, &project_id) {
                                    log::warn!("[scheduler] cycle error for {project_id}: {e}");
                                }
                            }
                        }
                    }
                    Err(e) => log::error!("[scheduler] cannot open DB: {e}"),
                }
            })
            .await;

            if let Err(e) = tick_result {
                log::error!("[scheduler] tick task failed: {e}");
            }
        }
    });

    state
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory DB with the full migrated schema (see `db::init_with_conn`)
    /// plus one project row (tasks and scheduler_rules have FK to projects).
    fn test_db() -> (Connection, String) {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        let project_id = "proj-test".to_string();
        conn.execute(
            "INSERT INTO projects (id, name, path) VALUES (?1, ?2, ?3)",
            rusqlite::params![project_id, "Test Project", "/tmp/test"],
        )
        .unwrap();
        (conn, project_id)
    }

    /// A due rule for a task type with no task definition. Unknown types default
    /// to `UserEnqueue`, so the batch kick-off finds no AutoEnqueue-ready tasks
    /// and executes nothing (no network, no agent calls).
    fn due_noop_rule(rule_id: &str, project_id: &str) -> SchedulerRule {
        SchedulerRule {
            rule_id: rule_id.to_string(),
            project_id: project_id.to_string(),
            task_type: "test_noop_task".to_string(),
            action: "create_task".to_string(),
            interval_hours: 24,
            priority: "medium".to_string(),
            phase: "collection".to_string(),
            enabled: true,
            last_run_at: None, // never run → overdue
        }
    }

    #[test]
    fn run_cycle_creates_task_for_due_rule() {
        let (conn, project_id) = test_db();
        upsert_rule(&conn, &due_noop_rule("rule-sync", &project_id)).unwrap();

        let result = run_cycle(&conn, &project_id).unwrap();

        assert_eq!(result.rules_evaluated, 1);
        assert_eq!(result.tasks_created, 1);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let task_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id = ?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(task_count, 1);
    }

    /// Regression test for the runtime-within-runtime trap: when a cycle creates
    /// tasks, `run_cycle` builds its own Tokio runtime for the batch kick-off,
    /// which panics if called from within an async context. The background loop
    /// therefore runs the tick via `spawn_blocking` (blocking threads carry no
    /// entered runtime) — this exercises exactly that mechanism from inside a
    /// Tokio runtime, as `start_background_scheduler` does.
    #[tokio::test]
    async fn run_cycle_via_spawn_blocking_inside_runtime_does_not_panic() {
        let (conn, project_id) = test_db();
        upsert_rule(&conn, &due_noop_rule("rule-blocking", &project_id)).unwrap();

        let result = tokio::task::spawn_blocking(move || run_cycle(&conn, &project_id))
            .await
            .expect("run_cycle panicked inside runtime context")
            .expect("run_cycle failed");

        assert_eq!(result.tasks_created, 1);
        assert!(result.due_rules[0].is_due);
    }
}
