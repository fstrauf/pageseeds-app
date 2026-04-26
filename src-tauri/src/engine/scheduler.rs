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
    pub action: String,        // "create_task" | "reminder_only"
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
                log::info!("[scheduler] batch complete: {} processed", batch_result.processed);
            }
            Err(e) => {
                log::warn!("[scheduler] batch failed: {e}");
            }
        }
    }

    Ok(cycle_result)
}

fn create_task_for_rule(conn: &Connection, rule: &SchedulerRule) -> Result<String, String> {
    use crate::config::{default_execution_mode, default_phase};
    use crate::models::task::{AgentPolicy, Priority, TaskStatus};

    let now = Utc::now().to_rfc3339();
    let id = format!("sched-{}-{}", rule.task_type.replace('_', "-"), Utc::now().timestamp());
    let priority_enum = match rule.priority.as_str() {
        "high" => Priority::High,
        "low" => Priority::Low,
        _ => Priority::Medium,
    };

    let task = crate::models::task::Task {
        id: id.clone(),
        task_type: rule.task_type.clone(),
        phase: default_phase(&rule.task_type).to_string(),
        status: TaskStatus::Todo,
        priority: priority_enum,
        execution_mode: default_execution_mode(&rule.task_type),
        agent_policy: AgentPolicy::None,
        title: Some(format!("Scheduled: {}", rule.task_type.replace('_', " "))),
        description: Some(format!("Auto-created by scheduler rule '{}'", rule.rule_id)),
        project_id: rule.project_id.clone(),
        depends_on: vec![],
        artifacts: vec![],
        run: Default::default(),
        created_at: now.clone(),
        updated_at: now,
    };

    task_store::create_task(conn, &task).map_err(|e| e.to_string())?;
    Ok(id)
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
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
        interval.tick().await; // skip the first immediate tick

        loop {
            interval.tick().await;

            let state_guard = state_clone.lock();
            let (db_path, running) = match state_guard {
                Ok(g) => (g.db_path.clone(), g.running),
                Err(_) => break,
            };

            if !running {
                break;
            }

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
        }
    });

    state
}
