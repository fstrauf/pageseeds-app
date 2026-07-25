use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::models::task::{Priority, Task, TaskStatus};

mod articles;
mod artifacts;
mod overview;
mod projects;

pub use articles::*;
pub use artifacts::*;
pub use overview::*;
pub use projects::*;

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    let depends_on_str: String = row.get(12)?;
    let artifacts_str: String = row.get(13)?;
    let run_attempts: i64 = row.get(14)?;
    let run_last_error: Option<String> = row.get(15)?;
    let run_provider: Option<String> = row.get(16)?;

    let depends_on: Vec<String> = serde_json::from_str(&depends_on_str).unwrap_or_default();
    let artifacts = serde_json::from_str(&artifacts_str).unwrap_or_default();

    Ok(Task {
        id: row.get(0)?,
        task_type: row.get(1)?,
        phase: row.get(2)?,
        status: row.get(3)?,
        priority: row.get(4)?,
        run_policy: row.get(5)?,
        review_surface: row.get(6)?,
        follow_up_policy: row.get(7)?,
        agent_policy: row.get(8)?,
        title: row.get(9)?,
        description: row.get(10)?,
        project_id: row.get(11)?,
        depends_on,
        artifacts,
        run: crate::models::task::TaskRun {
            attempts: run_attempts as u32,
            last_error: run_last_error,
            provider: run_provider,
            ..Default::default()
        },
        not_before: row.get(17).ok(),
        created_at: row.get(18)?,
        updated_at: row.get(19)?,
    })
}

/// Lightweight variant: skips deserialising the `artifacts` JSON blob.
/// Use this when you only need task metadata (status, type, title, etc.)
/// and don't want to pay the memory cost of large artifact payloads.
fn row_to_task_light(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    let depends_on_str: String = row.get(12)?;
    let run_attempts: i64 = row.get(14)?;
    let run_last_error: Option<String> = row.get(15)?;
    let run_provider: Option<String> = row.get(16)?;

    let depends_on: Vec<String> = serde_json::from_str(&depends_on_str).unwrap_or_default();

    Ok(Task {
        id: row.get(0)?,
        task_type: row.get(1)?,
        phase: row.get(2)?,
        status: row.get(3)?,
        priority: row.get(4)?,
        run_policy: row.get(5)?,
        review_surface: row.get(6)?,
        follow_up_policy: row.get(7)?,
        agent_policy: row.get(8)?,
        title: row.get(9)?,
        description: row.get(10)?,
        project_id: row.get(11)?,
        depends_on,
        artifacts: vec![], // Skip — saves memory on large artifact columns
        run: crate::models::task::TaskRun {
            attempts: run_attempts as u32,
            last_error: run_last_error,
            provider: run_provider,
            ..Default::default()
        },
        not_before: row.get(17).ok(),
        created_at: row.get(18)?,
        updated_at: row.get(19)?,
    })
}

const SELECT_COLS: &str = "
    id, type, phase, status, priority, run_policy, review_surface, follow_up_policy, agent_policy,
    title, description, project_id, depends_on, artifacts,
    run_attempts, run_last_error, run_provider, not_before, created_at, updated_at";

pub fn list_tasks(conn: &Connection, project_id: &str) -> Result<Vec<Task>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM tasks WHERE project_id = ?1 ORDER BY
         CASE priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
         updated_at DESC, created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tasks: Vec<Task> = stmt
        .query_map([project_id], row_to_task)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(tasks)
}

/// Lightweight variant of `list_tasks` that skips artifact deserialization.
/// Use for list views and batch scheduling where only metadata is needed.
pub fn list_tasks_light(conn: &Connection, project_id: &str) -> Result<Vec<Task>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM tasks WHERE project_id = ?1 ORDER BY
         CASE priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
         updated_at DESC, created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tasks: Vec<Task> = stmt
        .query_map([project_id], row_to_task_light)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(tasks)
}

pub fn list_tasks_filtered(
    conn: &Connection,
    project_id: &str,
    status: Option<&str>,
    phase: Option<&str>,
) -> Result<Vec<Task>> {
    let mut conditions = vec!["project_id = ?1".to_string()];
    let mut idx = 2;
    let mut binds: Vec<String> = vec![project_id.to_string()];

    if let Some(s) = status {
        // The frontend treats 'queued' as part of the todo bucket — mirror that in SQL.
        if s == "todo" {
            conditions.push(format!("status IN (?{idx}, ?{})", idx + 1));
            binds.push("todo".to_string());
            binds.push("queued".to_string());
            idx += 2;
        } else {
            conditions.push(format!("status = ?{idx}"));
            binds.push(s.to_string());
            idx += 1;
        }
    }
    if let Some(p) = phase {
        conditions.push(format!("phase = ?{idx}"));
        binds.push(p.to_string());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT {SELECT_COLS} FROM tasks WHERE {where_clause} ORDER BY
         CASE priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
            updated_at DESC, created_at DESC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let tasks: Vec<Task> = stmt
        .query_map(rusqlite::params_from_iter(binds.iter()), row_to_task)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(tasks)
}

/// Lightweight variant of `list_tasks_filtered` that skips artifact deserialization.
pub fn list_tasks_filtered_light(
    conn: &Connection,
    project_id: &str,
    status: Option<&str>,
    phase: Option<&str>,
) -> Result<Vec<Task>> {
    let mut conditions = vec!["project_id = ?1".to_string()];
    let mut idx = 2;
    let mut binds: Vec<String> = vec![project_id.to_string()];

    if let Some(s) = status {
        conditions.push(format!("status = ?{idx}"));
        binds.push(s.to_string());
        idx += 1;
    }
    if let Some(p) = phase {
        conditions.push(format!("phase = ?{idx}"));
        binds.push(p.to_string());
    }

    let where_clause = conditions.join(" AND ");
    let sql = format!(
        "SELECT {SELECT_COLS} FROM tasks WHERE {where_clause} ORDER BY
         CASE priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
            updated_at DESC, created_at DESC"
    );

    let mut stmt = conn.prepare(&sql)?;
    let tasks: Vec<Task> = stmt
        .query_map(rusqlite::params_from_iter(binds.iter()), row_to_task_light)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(tasks)
}

/// List tasks across ALL projects filtered by one or more statuses.
/// Used to rehydrate the task queue after a frontend reload.
pub fn list_all_tasks_by_statuses(conn: &Connection, statuses: &[&str]) -> Result<Vec<Task>> {
    if statuses.is_empty() {
        return Ok(vec![]);
    }
    let placeholders: Vec<String> = statuses
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        "SELECT {SELECT_COLS} FROM tasks WHERE status IN ({}) ORDER BY
         CASE priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
            updated_at DESC, created_at DESC",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;
    let tasks: Vec<Task> = stmt
        .query_map(
            rusqlite::params_from_iter(statuses.iter()),
            row_to_task_light,
        )?
        .filter_map(|r| r.ok())
        .collect();
    Ok(tasks)
}

pub fn get_task(conn: &Connection, id: &str) -> Result<Task> {
    let sql = format!("SELECT {SELECT_COLS} FROM tasks WHERE id = ?1");
    conn.query_row(&sql, [id], row_to_task)
        .map_err(|_| Error::Other(format!("Task '{id}' not found")))
}

/// Lightweight variant of `get_task` that skips artifact deserialization.
pub fn get_task_light(conn: &Connection, id: &str) -> Result<Task> {
    let sql = format!("SELECT {SELECT_COLS} FROM tasks WHERE id = ?1");
    conn.query_row(&sql, [id], row_to_task_light)
        .map_err(|_| Error::Other(format!("Task '{id}' not found")))
}

pub fn create_task(conn: &Connection, task: &Task) -> Result<Task> {
    let depends_on = serde_json::to_string(&task.depends_on)?;
    let artifacts = serde_json::to_string(&task.artifacts)?;
    conn.execute(
        "INSERT INTO tasks (
            id, type, phase, status, priority, run_policy, review_surface, follow_up_policy, agent_policy,
            title, description, project_id, depends_on, artifacts,
            run_attempts, run_last_error, run_provider, not_before, created_at, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        rusqlite::params![
            task.id,
            task.task_type,
            task.phase,
            task.status,
            task.priority,
            task.run_policy,
            task.review_surface,
            task.follow_up_policy,
            task.agent_policy,
            task.title,
            task.description,
            task.project_id,
            depends_on,
            artifacts,
            task.run.attempts as i64,
            task.run.last_error,
            task.run.provider,
            task.not_before,
            task.created_at,
            task.updated_at,
        ],
    )?;
    get_task(conn, &task.id)
}

pub fn update_task_status(conn: &Connection, id: &str, status: TaskStatus) -> Result<Task> {
    let now = chrono::Utc::now().to_rfc3339();
    let rows = conn.execute(
        "UPDATE tasks SET status = ?1, updated_at = ?2, run_last_error = CASE WHEN ?1 = 'in_progress' THEN NULL ELSE run_last_error END WHERE id = ?3",
        rusqlite::params![status, now, id],
    )?
    ;
    if rows == 0 {
        return Err(Error::Other(format!("Task '{id}' not found")));
    }
    get_task(conn, id)
}

pub fn update_task(
    conn: &Connection,
    id: &str,
    title: Option<&str>,
    description: Option<&str>,
    priority: Priority,
) -> Result<Task> {
    let now = chrono::Utc::now().to_rfc3339();
    let rows = conn.execute(
        "UPDATE tasks SET title = ?1, description = ?2, priority = ?3, updated_at = ?4 WHERE id = ?5",
        rusqlite::params![title, description, priority, now, id],
    )?;
    if rows == 0 {
        return Err(Error::Other(format!("Task '{id}' not found")));
    }
    get_task(conn, id)
}

/// Find the first active (todo or in_progress) task of a given type for a project.
/// Used by `quick_run_workflow` to avoid creating duplicate tasks.
pub fn find_active_task_by_type(
    conn: &Connection,
    project_id: &str,
    task_type: &str,
) -> Result<Option<Task>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM tasks
         WHERE project_id = ?1 AND type = ?2 AND status IN ('todo', 'in_progress')
         ORDER BY created_at DESC LIMIT 1"
    );
    match conn.query_row(&sql, rusqlite::params![project_id, task_type], row_to_task) {
        Ok(task) => Ok(Some(task)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Clear the last error on a task so it can be retried cleanly.
pub fn reset_task_error(conn: &Connection, id: &str) -> Result<Task> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE tasks SET run_last_error = NULL, updated_at = ?1 WHERE id = ?2",
        rusqlite::params![now, id],
    )?;
    get_task(conn, id)
}

pub fn delete_task(conn: &Connection, id: &str) -> Result<()> {
    // task_runs has a foreign key to tasks without ON DELETE CASCADE.
    // Remove dependent rows first so task deletion succeeds consistently.
    conn.execute("DELETE FROM task_runs WHERE task_id = ?1", [id])?;
    let rows = conn.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
    if rows == 0 {
        return Err(Error::Other(format!("Task '{id}' not found")));
    }
    Ok(())
}


#[cfg(test)]
mod tests;
