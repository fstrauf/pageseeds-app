/// Backend-owned task queue.
///
/// The queue persists in SQLite (queue_runs + queue_items tables) and survives
/// frontend reloads and app restarts. The frontend is a projection cache only.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use serde_json;
use tauri::{AppHandle, Emitter};

use crate::engine::executor::{self, ExecutionResult};
use crate::engine::queue_runner;
use crate::engine::task_store;
use crate::error::Result;
use crate::models::queue::{
    EnqueueItem, EnqueueMode, QueueItem, QueueItemStatus, QueueRun, QueueRunStatus, QueueSnapshot,
};
use crate::models::task::TaskStatus;

static RUNNER_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Default behavior: the queue should continue processing even when individual
/// tasks fail. Only pause when the user explicitly requests it.
const DEFAULT_PAUSE_ON_ERROR: bool = false;

/// Guard that ensures RUNNER_ACTIVE is reset when the runner exits,
/// even if the runner task panics.
struct RunnerGuard;

impl Drop for RunnerGuard {
    fn drop(&mut self) {
        RUNNER_ACTIVE.store(false, Ordering::SeqCst);
        log::info!("[queue] Runner guard dropped, RUNNER_ACTIVE reset to false");
    }
}

// ─── Queue Run CRUD ─────────────────────────────────────────────────────────

fn get_active_run(conn: &Connection) -> Result<Option<QueueRun>> {
    let mut stmt = conn.prepare(
        "SELECT id, status, pause_on_error, created_at, updated_at, started_at, finished_at
         FROM queue_runs
         WHERE status IN ('idle', 'running', 'paused')
         ORDER BY created_at DESC
         LIMIT 1",
    )?;
    let row = stmt
        .query_row([], |row| {
            Ok(QueueRun {
                id: row.get(0)?,
                status: row.get(1)?,
                pause_on_error: row.get::<_, i64>(2)? != 0,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
            })
        })
        .optional()?;
    Ok(row)
}

/// Get the most recent finished/failed run that still has items.
/// Used so the UI can show completion state before dismissal.
fn get_recent_finished_run(conn: &Connection) -> Result<Option<QueueRun>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.status, r.pause_on_error, r.created_at, r.updated_at, r.started_at, r.finished_at
         FROM queue_runs r
         JOIN queue_items qi ON qi.run_id = r.id
         WHERE r.status IN ('finished', 'failed')
         GROUP BY r.id
         HAVING COUNT(qi.task_id) > 0
         ORDER BY r.finished_at DESC
         LIMIT 1",
    )?;
    let row = stmt
        .query_row([], |row| {
            Ok(QueueRun {
                id: row.get(0)?,
                status: row.get(1)?,
                pause_on_error: row.get::<_, i64>(2)? != 0,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
            })
        })
        .optional()?;
    Ok(row)
}

fn create_run(conn: &Connection) -> Result<QueueRun> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = format!("run-{}", chrono::Utc::now().timestamp_millis());
    conn.execute(
        "INSERT INTO queue_runs (id, status, pause_on_error, created_at, updated_at)
         VALUES (?1, 'idle', 0, ?2, ?2)",
        [&id, &now],
    )?;
    Ok(QueueRun {
        id,
        status: QueueRunStatus::Idle,
        pause_on_error: DEFAULT_PAUSE_ON_ERROR,
        created_at: now.clone(),
        updated_at: now,
        started_at: None,
        finished_at: None,
    })
}

fn get_or_create_active_run(conn: &Connection) -> Result<QueueRun> {
    match get_active_run(conn)? {
        Some(run) => Ok(run),
        None => {
            // Reuse the most recent finished/failed run so retries don't
            // orphan the rest of the queue in an invisible run.
            match get_recent_finished_run(conn)? {
                Some(run) => Ok(run),
                None => create_run(conn),
            }
        }
    }
}

fn update_run_status(conn: &Connection, run_id: &str, status: QueueRunStatus) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE queue_runs SET status = ?1, updated_at = ?2 WHERE id = ?3",
        [status.as_str(), &now, run_id],
    )?;
    Ok(())
}

fn set_run_started(conn: &Connection, run_id: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE queue_runs SET status = 'running', started_at = ?1, updated_at = ?1 WHERE id = ?2",
        [&now, run_id],
    )?;
    Ok(())
}

fn set_run_finished(conn: &Connection, run_id: &str, status: QueueRunStatus) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE queue_runs SET status = ?1, finished_at = ?2, updated_at = ?2 WHERE id = ?3",
        [status.as_str(), &now, run_id],
    )?;
    Ok(())
}

// ─── Queue Item CRUD ────────────────────────────────────────────────────────

fn max_position(conn: &Connection, run_id: &str) -> Result<i64> {
    let max: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), 0) FROM queue_items WHERE run_id = ?1",
            [run_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(max)
}

fn current_running_position(conn: &Connection, run_id: &str) -> Result<i64> {
    let pos: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), -1) FROM queue_items WHERE run_id = ?1 AND status = 'running'",
            [run_id],
            |row| row.get(0),
        )
        .unwrap_or(-1);
    Ok(pos)
}

fn insert_queue_items(
    conn: &Connection,
    run_id: &str,
    items: Vec<EnqueueItem>,
    mode: EnqueueMode,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // Remove stale items (failed/skipped/completed) for tasks being re-enqueued
    // so retries get fresh pending entries instead of being silently ignored
    // by ON CONFLICT DO NOTHING.
    for item in &items {
        conn.execute(
            "DELETE FROM queue_items WHERE run_id = ?1 AND task_id = ?2 AND status IN ('failed', 'skipped', 'completed')",
            rusqlite::params![run_id, &item.task_id],
        )?;
    }

    match mode {
        EnqueueMode::Append => {
            let mut position = max_position(conn, run_id)?;
            for item in items {
                position += 1;
                conn.execute(
                    "INSERT INTO queue_items (run_id, position, task_id, project_id, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)
                     ON CONFLICT (run_id, task_id) DO NOTHING",
                    rusqlite::params![run_id, position, &item.task_id, &item.project_id, &now],
                )?;
                if let Err(e) =
                    task_store::update_task_status(conn, &item.task_id, TaskStatus::Queued)
                {
                    log::warn!(
                        "[queue] Failed to mark task {} as queued: {}",
                        item.task_id,
                        e
                    );
                }
            }
        }
        EnqueueMode::Next => {
            // Fetch existing items ordered by position, created_at
            let mut stmt = conn.prepare(
                "SELECT task_id, position, created_at FROM queue_items WHERE run_id = ?1 ORDER BY position ASC, created_at ASC"
            )?;
            let existing: Vec<(String, i64, String)> = stmt
                .query_map([run_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            // Use current_running_position to find the split point
            let running_pos = current_running_position(conn, run_id).unwrap_or(-1);
            let split_idx = if running_pos >= 0 {
                existing
                    .iter()
                    .position(|(_, pos, _)| *pos > running_pos)
                    .unwrap_or(existing.len())
            } else {
                0
            };

            // Insert new items right after the running item
            let mut new_order: Vec<(String, i64, String)> =
                Vec::with_capacity(existing.len() + items.len());
            new_order.extend_from_slice(&existing[..split_idx]);
            for item in &items {
                new_order.push((item.task_id.clone(), 0, now.clone()));
            }
            new_order.extend_from_slice(&existing[split_idx..]);

            // Insert new items into DB
            for item in &items {
                conn.execute(
                    "INSERT INTO queue_items (run_id, position, task_id, project_id, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)
                     ON CONFLICT (run_id, task_id) DO NOTHING",
                    rusqlite::params![run_id, 0, &item.task_id, &item.project_id, &now],
                )?;
                if let Err(e) =
                    task_store::update_task_status(conn, &item.task_id, TaskStatus::Queued)
                {
                    log::warn!(
                        "[queue] Failed to mark task {} as queued: {}",
                        item.task_id,
                        e
                    );
                }
            }

            // Renumber everything
            for (idx, (task_id, _, _)) in new_order.iter().enumerate() {
                conn.execute(
                    "UPDATE queue_items SET position = ?1 WHERE run_id = ?2 AND task_id = ?3",
                    [&(idx as i64).to_string(), run_id, task_id],
                )?;
            }
        }
    }

    Ok(())
}

fn renumber_positions(conn: &Connection, run_id: &str) -> Result<()> {
    let items: Vec<(String, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT task_id, position FROM queue_items WHERE run_id = ?1 ORDER BY position ASC, created_at ASC"
        )?;
        let rows = stmt.query_map([run_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    for (idx, (task_id, _)) in items.iter().enumerate() {
        conn.execute(
            "UPDATE queue_items SET position = ?1 WHERE run_id = ?2 AND task_id = ?3",
            [&(idx as i64).to_string(), run_id, task_id],
        )?;
    }
    Ok(())
}

fn get_next_pending_item(conn: &Connection, run_id: &str) -> Result<Option<QueueItem>> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare(
        "SELECT qi.run_id, qi.position, qi.task_id, qi.project_id, qi.status, qi.error,
                qi.result_json, qi.created_at, qi.updated_at,
                t.title, t.type, p.name
         FROM queue_items qi
         JOIN tasks t ON t.id = qi.task_id
         LEFT JOIN projects p ON p.id = qi.project_id
         WHERE qi.run_id = ?1 AND qi.status = 'pending'
           AND (t.not_before IS NULL OR t.not_before <= ?2)
         ORDER BY qi.position ASC
         LIMIT 1",
    )?;
    let row = stmt
        .query_row(rusqlite::params![run_id, &now], |row| {
            Ok(QueueItem {
                run_id: row.get(0)?,
                position: row.get(1)?,
                task_id: row.get(2)?,
                project_id: row.get(3)?,
                status: row.get(4)?,
                error: row.get(5)?,
                result_json: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                title: row.get(9)?,
                task_type: row.get(10)?,
                project_name: row.get(11)?,
            })
        })
        .optional()?;
    Ok(row)
}

/// Get the earliest `not_before` timestamp among pending items in a run.
/// Returns None if there are no pending items with a future `not_before`.
fn get_earliest_not_before(conn: &Connection, run_id: &str) -> Result<Option<String>> {
    let row: Option<String> = conn
        .query_row(
            "SELECT MIN(t.not_before)
             FROM queue_items qi
             JOIN tasks t ON t.id = qi.task_id
             WHERE qi.run_id = ?1 AND qi.status = 'pending'
               AND t.not_before IS NOT NULL
               AND t.not_before > ?2",
            rusqlite::params![run_id, chrono::Utc::now().to_rfc3339()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(row)
}

fn update_item_status(
    conn: &Connection,
    run_id: &str,
    task_id: &str,
    status: QueueItemStatus,
    error: Option<&str>,
    result_json: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE queue_items SET status = ?1, error = ?2, result_json = ?3, updated_at = ?4
         WHERE run_id = ?5 AND task_id = ?6",
        [
            status.as_str(),
            error.unwrap_or(""),
            result_json.unwrap_or(""),
            &now,
            run_id,
            task_id,
        ],
    )?;
    Ok(())
}

fn list_queue_items(conn: &Connection, run_id: &str) -> Result<Vec<QueueItem>> {
    let mut stmt = conn.prepare(
        "SELECT qi.run_id, qi.position, qi.task_id, qi.project_id, qi.status, qi.error,
                qi.result_json, qi.created_at, qi.updated_at,
                t.title, t.type, p.name
         FROM queue_items qi
         JOIN tasks t ON t.id = qi.task_id
         LEFT JOIN projects p ON p.id = qi.project_id
         WHERE qi.run_id = ?1
         ORDER BY qi.position ASC",
    )?;
    let items: Vec<QueueItem> = stmt
        .query_map([run_id], |row| {
            Ok(QueueItem {
                run_id: row.get(0)?,
                position: row.get(1)?,
                task_id: row.get(2)?,
                project_id: row.get(3)?,
                status: row.get(4)?,
                error: row.get(5)?,
                result_json: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                title: row.get(9)?,
                task_type: row.get(10)?,
                project_name: row.get(11)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(items)
}

// ─── Public API ─────────────────────────────────────────────────────────────

pub fn enqueue_tasks(
    conn: &Connection,
    items: Vec<EnqueueItem>,
    mode: EnqueueMode,
) -> Result<QueueSnapshot> {
    if items.is_empty() {
        return get_queue_snapshot(conn);
    }

    let run = get_or_create_active_run(conn)?;
    insert_queue_items(conn, &run.id, items, mode)?;

    // If run was finished/failed/paused, resurrect it as idle so runner can pick it up.
    // Enqueuing new work is an implicit request to run the queue.
    if run.status == QueueRunStatus::Finished
        || run.status == QueueRunStatus::Failed
        || run.status == QueueRunStatus::Paused
    {
        update_run_status(conn, &run.id, QueueRunStatus::Idle)?;
    }

    get_queue_snapshot(conn)
}

pub fn remove_queue_item(conn: &Connection, task_id: &str) -> Result<QueueSnapshot> {
    let run = match get_active_run(conn)? {
        Some(r) => r,
        None => return get_queue_snapshot(conn),
    };

    // Only remove pending items
    let deleted = conn.execute(
        "DELETE FROM queue_items WHERE run_id = ?1 AND task_id = ?2 AND status = 'pending'",
        [&run.id, task_id],
    )?;

    if deleted > 0 {
        // Reset task status back to todo
        if let Err(e) = task_store::update_task_status(conn, task_id, TaskStatus::Todo) {
            log::warn!("[queue] Failed to reset task {} to todo: {}", task_id, e);
        }
        renumber_positions(conn, &run.id)?;
    }

    get_queue_snapshot(conn)
}

pub fn pause_queue(conn: &Connection) -> Result<QueueSnapshot> {
    if let Some(run) = get_active_run(conn)? {
        if run.status == QueueRunStatus::Running {
            update_run_status(conn, &run.id, QueueRunStatus::Paused)?;
        }
    }
    get_queue_snapshot(conn)
}

pub fn resume_queue(conn: &Connection) -> Result<QueueSnapshot> {
    if let Some(run) = get_active_run(conn)? {
        if run.status == QueueRunStatus::Paused || run.status == QueueRunStatus::Idle {
            update_run_status(conn, &run.id, QueueRunStatus::Idle)?;
        }
    }
    get_queue_snapshot(conn)
}

pub fn get_queue_snapshot(conn: &Connection) -> Result<QueueSnapshot> {
    let run = match get_active_run(conn)? {
        Some(r) => Some(r),
        None => get_recent_finished_run(conn)?,
    };
    let items = match &run {
        Some(r) => list_queue_items(conn, &r.id)?,
        None => vec![],
    };
    Ok(QueueSnapshot { run, items })
}

pub fn clear_completed_queue_items(conn: &Connection) -> Result<QueueSnapshot> {
    // Delete completed/failed/skipped items from ALL finished/failed runs so
    // old phantom queues don't surface after clearing the current one.
    conn.execute(
        "DELETE FROM queue_items
         WHERE run_id IN (
             SELECT id FROM queue_runs WHERE status IN ('finished', 'failed')
         )
         AND status IN ('completed', 'failed', 'skipped')",
        [],
    )?;
    // Also clean up any finished/failed runs that now have zero items so they
    // don't hang around in the DB.
    conn.execute(
        "DELETE FROM queue_runs
         WHERE status IN ('finished', 'failed')
         AND id NOT IN (SELECT DISTINCT run_id FROM queue_items)",
        [],
    )?;
    get_queue_snapshot(conn)
}

pub fn dismiss_queue(conn: &Connection) -> Result<()> {
    if let Some(run) = get_active_run(conn)? {
        // Mark run as finished to hide it from the active snapshot
        set_run_finished(conn, &run.id, QueueRunStatus::Finished)?;
        // Reset any remaining queued tasks back to todo
        let queued: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT task_id FROM queue_items WHERE run_id = ?1 AND status = 'pending'",
            )?;
            let rows: rusqlite::Result<Vec<String>> = stmt
                .query_map([&run.id], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect::<Vec<_>>()
                .into_iter()
                .map(Ok)
                .collect();
            rows.unwrap_or_default()
        };
        for task_id in queued {
            let _ = task_store::update_task_status(conn, &task_id, TaskStatus::Todo);
        }
        // Delete all queue items so the dismissed run doesn't reappear
        conn.execute(
            "DELETE FROM queue_items WHERE run_id = ?1",
            [&run.id],
        )?;
    }
    Ok(())
}

/// Recover any queue items left running from a crashed session.
pub fn recover_queue_on_startup(conn: &Connection) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // Reset running queue items back to pending
    conn.execute(
        "UPDATE queue_items SET status = 'pending', updated_at = ?1 WHERE status = 'running'",
        [&now],
    )?;

    // Reset active runs from running to paused so user can decide to resume
    conn.execute(
        "UPDATE queue_runs SET status = 'paused', updated_at = ?1 WHERE status = 'running'",
        [&now],
    )?;

    // Reset orphaned in_progress tasks to todo so they can be re-enqueued.
    // A task can be in_progress with no running queue item if the runner
    // crashed between starting the task and finishing it.
    conn.execute(
        "UPDATE tasks SET status = 'todo', updated_at = ?1 WHERE status = 'in_progress'",
        [&now],
    )?;

    Ok(())
}

// ─── Runner ─────────────────────────────────────────────────────────────────

pub async fn ensure_runner_started(
    db_path: PathBuf,
    app_handle: AppHandle,
    gsc_token: Option<String>,
) {
    if RUNNER_ACTIVE.swap(true, Ordering::SeqCst) {
        log::info!("[queue] Runner already active, skipping spawn");
        return;
    }

    log::info!("[queue] Spawning queue runner");
    tokio::spawn(async move {
        run_queue(db_path, app_handle, gsc_token).await;
        RUNNER_ACTIVE.store(false, Ordering::SeqCst);
        log::info!("[queue] Runner finished");
    });
}

async fn run_queue(db_path: PathBuf, app_handle: AppHandle, gsc_token: Option<String>) {
    let _guard = RunnerGuard;
    log::info!("[queue_runner] ==========================================");
    log::info!("[queue_runner] BACKEND QUEUE RUNNER STARTED");

    loop {
        // Open a fresh connection each iteration so we don't hold a lock
        let mut conn = match Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[queue_runner] Failed to open DB: {}", e);
                break; // terminal: cannot operate without DB
            }
        };

        let run = match get_active_run(&conn) {
            Ok(Some(r)) => r,
            Ok(None) => {
                log::info!("[queue_runner] No active queue run, exiting");
                break; // terminal: nothing to do
            }
            Err(e) => {
                log::error!("[queue_runner] Failed to get active run: {}", e);
                break; // terminal: cannot determine run state
            }
        };

        if run.status == QueueRunStatus::Paused {
            log::info!("[queue_runner] Queue is paused, exiting runner");
            break; // terminal: user paused the queue
        }

        if run.status == QueueRunStatus::Finished || run.status == QueueRunStatus::Failed {
            log::info!("[queue_runner] Queue is finished/failed, exiting runner");
            break; // terminal: run has reached a terminal state
        }

        // Start the run if it's idle
        if run.status == QueueRunStatus::Idle {
            if let Err(e) = set_run_started(&conn, &run.id) {
                log::error!("[queue_runner] Failed to mark run as started: {}", e);
                break; // terminal: cannot transition run to running
            }
        }

        // Lease next pending item
        let item = match get_next_pending_item(&conn, &run.id) {
            Ok(Some(item)) => item,
            Ok(None) => {
                // Check if there are future delayed items (not_before in the future)
                match get_earliest_not_before(&conn, &run.id) {
                    Ok(Some(not_before)) => {
                        log::info!(
                            "[queue_runner] No pending items ready now; earliest delayed item at {}. Sleeping until then.",
                            not_before
                        );
                        // Sleep until the earliest delayed item is due.
                        // Cap to short chunks so we re-check the DB periodically
                        // and can exit promptly if the queue is paused/dismissed.
                        if let Ok(due) = chrono::DateTime::parse_from_rfc3339(&not_before) {
                            let now = chrono::Utc::now();
                            let due_utc = due.with_timezone(&chrono::Utc);
                            if due_utc > now {
                                let sleep_secs = (due_utc - now).num_seconds() as u64;
                                let chunk = std::cmp::min(sleep_secs, 5);
                                tokio::time::sleep(tokio::time::Duration::from_secs(chunk))
                                    .await;
                                continue;
                            }
                        }
                        // If parsing failed or due time is now, continue the loop
                        continue;
                    }
                    _ => {
                        log::info!("[queue_runner] No pending items, marking run as finished");
                        let _ = set_run_finished(&conn, &run.id, QueueRunStatus::Finished);
                        emit_finished(&app_handle);
                        break;
                    }
                }
            }
            Err(e) => {
                // Transient DB error — log and retry rather than killing the entire run.
                log::error!("[queue_runner] Failed to get next item: {}. Will retry on next loop iteration.", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        log::info!(
            "[queue_runner] TASK: {} ({})",
            item.task_id,
            item.title.as_deref().unwrap_or("Untitled")
        );

        // Atomically mark queue item and task as running so they cannot drift
        // if the app crashes between the two updates.
        if let Err(e) = (|| -> Result<()> {
            let tx = conn.transaction()?;
            update_item_status(
                &tx,
                &run.id,
                &item.task_id,
                QueueItemStatus::Running,
                None,
                None,
            )?;
            task_store::update_task_status(&tx, &item.task_id, TaskStatus::InProgress)?;
            tx.commit()?;
            Ok(())
        })() {
            log::error!("[queue_runner] Failed to mark item/task as running: {}", e);
        }

        emit_started(&app_handle, &item);

        // Execute task
        let task_id = item.task_id.clone();
        let db_path_clone = db_path.clone();
        let app_handle_clone = app_handle.clone();
        let gsc_token_clone = gsc_token.clone();

        let result = tokio::task::spawn_blocking(move || {
            let conn = match Connection::open(&db_path_clone) {
                Ok(c) => c,
                Err(e) => return Err(format!("DB error: {}", e)),
            };
            conn.busy_timeout(Duration::from_secs(10))
                .map_err(|e| format!("Busy timeout: {}", e))?;

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("Runtime error: {}", e))?;

            rt.block_on(async {
                executor::execute_task_with_token(
                    &conn,
                    &task_id,
                    gsc_token_clone.as_deref(),
                    Some(app_handle_clone),
                    false,
                )
                .await
            })
        })
        .await;

        // Re-open connection for updates
        let mut conn = match Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[queue_runner] Failed to reopen DB after task: {}", e);
                break; // terminal: cannot persist task result
            }
        };

        match result {
            Ok(Ok(exec_result)) => {
                let result_json = serde_json::to_string(&exec_result).ok();
                if exec_result.success {
                    update_item_status(
                        &conn,
                        &run.id,
                        &item.task_id,
                        QueueItemStatus::Completed,
                        None,
                        result_json.as_deref(),
                    )
                    .ok();
                    emit_completed(&app_handle, &item, &exec_result);

                    // Auto-enqueue follow-ups with auto_enqueue policy
                    for follow_up in &exec_result.follow_up_tasks {
                        if follow_up.run_policy == "auto_enqueue" {
                            let enqueue_item = EnqueueItem {
                                task_id: follow_up.id.clone(),
                                project_id: item.project_id.clone(),
                                title: Some(follow_up.title.clone()),
                                task_type: Some(follow_up.task_type.clone()),
                                project_name: item.project_name.clone(),
                            };
                            if let Err(e) =
                                enqueue_tasks(&conn, vec![enqueue_item], EnqueueMode::Append)
                            {
                                log::error!(
                                    "[queue_runner] Failed to auto-enqueue follow-up: {}",
                                    e
                                );
                            } else {
                                emit_follow_up(&app_handle, &item.project_id, follow_up);
                            }
                        }
                    }
                } else {
                    if let Err(e) = (|| -> Result<()> {
                        let tx = conn.transaction()?;
                        update_item_status(
                            &tx,
                            &run.id,
                            &item.task_id,
                            QueueItemStatus::Failed,
                            Some(&exec_result.message),
                            result_json.as_deref(),
                        )?;
                        task_store::update_task_status(
                            &tx,
                            &item.task_id,
                            crate::engine::executor::completed_task_status(
                                item.task_type.as_deref().unwrap_or(""),
                                false,
                            ),
                        )?;
                        tx.commit()?;
                        Ok(())
                    })() {
                        log::error!("[queue_runner] Failed to persist task failure: {}", e);
                    }
                    emit_failed(&app_handle, &item, &exec_result.message);

                    if run.pause_on_error {
                        log::warn!("[queue_runner] Pausing queue because pause_on_error=true. Task {} failed with: {}", item.task_id, exec_result.message);
                        update_run_status(&conn, &run.id, QueueRunStatus::Paused).ok();
                        emit_finished(&app_handle);
                        break; // terminal: user requested pause on error
                    }
                    log::warn!("[queue_runner] Task {} failed but queue continues (pause_on_error=false). Error: {}", item.task_id, exec_result.message);
                }
            }
            Ok(Err(e)) => {
                if let Err(db_err) = (|| -> Result<()> {
                    let tx = conn.transaction()?;
                    update_item_status(
                        &tx,
                        &run.id,
                        &item.task_id,
                        QueueItemStatus::Failed,
                        Some(&e),
                        None,
                    )?;
                    task_store::update_task_status(&tx, &item.task_id, TaskStatus::Failed)?;
                    tx.commit()?;
                    Ok(())
                })() {
                    log::error!("[queue_runner] Failed to persist task error: {}", db_err);
                }
                emit_failed(&app_handle, &item, &e);

                if run.pause_on_error {
                    log::warn!("[queue_runner] Pausing queue because pause_on_error=true. Task {} errored with: {}", item.task_id, e);
                    update_run_status(&conn, &run.id, QueueRunStatus::Paused).ok();
                    emit_finished(&app_handle);
                    break; // terminal: user requested pause on error
                }
                log::warn!("[queue_runner] Task {} errored but queue continues (pause_on_error=false). Error: {}", item.task_id, e);
            }
            Err(e) => {
                let err = format!("Task panicked: {:?}", e);
                log::error!("TASK PANIC: task_id={} type={} error={}", item.task_id, item.task_type.as_deref().unwrap_or("unknown"), err);
                if let Err(db_err) = (|| -> Result<()> {
                    let tx = conn.transaction()?;
                    update_item_status(
                        &tx,
                        &run.id,
                        &item.task_id,
                        QueueItemStatus::Failed,
                        Some(&err),
                        None,
                    )?;
                    task_store::update_task_status(&tx, &item.task_id, TaskStatus::Failed)?;
                    tx.commit()?;
                    Ok(())
                })() {
                    log::error!("[queue_runner] Failed to persist panic status: {}", db_err);
                }
                emit_failed(&app_handle, &item, &err);

                if run.pause_on_error {
                    log::warn!("[queue_runner] Pausing queue because pause_on_error=true. Task {} panicked.", item.task_id);
                    update_run_status(&conn, &run.id, QueueRunStatus::Paused).ok();
                    emit_finished(&app_handle);
                    break; // terminal: user requested pause on error
                }
                log::warn!("[queue_runner] Task {} panicked but queue continues (pause_on_error=false).", item.task_id);
            }
        }

        // Task-type-specific cooldown to avoid rate-limiting external APIs.
        // Reddit reply tasks are aggressively rate-limited; add a 90s gap.
        let sleep_ms = if item.task_type.as_deref() == Some("reddit_reply") {
            90_000
        } else {
            100
        };
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }

    log::info!("[queue_runner] BACKEND QUEUE RUNNER FINISHED");
    log::info!("[queue_runner] ==========================================");
}

// ─── Event Emission ─────────────────────────────────────────────────────────

fn emit_started(app_handle: &AppHandle, item: &QueueItem) {
    let event = queue_runner::QueueProgressEvent {
        event_type: "started".to_string(),
        task_id: item.task_id.clone(),
        project_id: item.project_id.clone(),
        payload: serde_json::json!({
            "title": item.title,
            "task_type": item.task_type,
        }),
    };
    let _ = app_handle.emit("queue:task-started", &event);
}

fn emit_completed(app_handle: &AppHandle, item: &QueueItem, result: &ExecutionResult) {
    let event = queue_runner::QueueProgressEvent {
        event_type: "completed".to_string(),
        task_id: item.task_id.clone(),
        project_id: item.project_id.clone(),
        payload: serde_json::json!({
            "message": result.message,
            "success": result.success,
            "started_at": result.started_at,
            "finished_at": result.finished_at,
            "follow_up_tasks": result.follow_up_tasks,
        }),
    };
    let _ = app_handle.emit("queue:task-completed", &event);
}

fn emit_failed(app_handle: &AppHandle, item: &QueueItem, error: &str) {
    let event = queue_runner::QueueProgressEvent {
        event_type: "failed".to_string(),
        task_id: item.task_id.clone(),
        project_id: item.project_id.clone(),
        payload: serde_json::json!({
            "error": error,
            "message": error,
        }),
    };
    let _ = app_handle.emit("queue:task-failed", &event);
}

fn emit_finished(app_handle: &AppHandle) {
    let _ = app_handle.emit("queue:finished", ());
}

fn emit_follow_up(app_handle: &AppHandle, project_id: &str, follow_up: &executor::FollowUpTask) {
    let event = queue_runner::FollowUpCreatedEvent {
        task_id: follow_up.id.clone(),
        project_id: project_id.to_string(),
        title: follow_up.title.clone(),
        task_type: follow_up.task_type.clone(),
        run_policy: follow_up.run_policy.clone(),
    };
    let _ = app_handle.emit("queue:follow-up-created", &event);
}
