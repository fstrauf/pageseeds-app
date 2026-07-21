use rusqlite::Connection;

use crate::error::Result;

use super::get_task;

// ─── Artifact helpers (used by executor) ─────────────────────────────────────

use crate::models::task::TaskArtifact;

pub fn append_task_artifact(
    conn: &Connection,
    task_id: &str,
    artifact: &TaskArtifact,
) -> Result<()> {
    // Load current artifacts, append, save back
    let task = get_task(conn, task_id)?;
    let mut artifacts = task.artifacts;
    artifacts.push(artifact.clone());
    let json = serde_json::to_string(&artifacts)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE tasks SET artifacts = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![json, now, task_id],
    )?;
    Ok(())
}

pub fn upsert_task_artifact(
    conn: &Connection,
    task_id: &str,
    artifact: &TaskArtifact,
) -> Result<()> {
    let task = get_task(conn, task_id)?;
    let mut artifacts = task.artifacts;
    if let Some(existing) = artifacts.iter_mut().find(|a| a.key == artifact.key) {
        *existing = artifact.clone();
    } else {
        artifacts.push(artifact.clone());
    }
    let json = serde_json::to_string(&artifacts)?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE tasks SET artifacts = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![json, now, task_id],
    )?;
    Ok(())
}

/// Record a task_run row and bump the attempt counter on the task.
pub fn record_task_run(
    conn: &Connection,
    task_id: &str,
    success: bool,
    error: Option<&str>,
    provider: Option<&str>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO task_runs (task_id, attempt, provider, started_at, finished_at, success, error, prompt_tokens, completion_tokens)
         SELECT ?1,
                COALESCE((SELECT MAX(attempt) FROM task_runs WHERE task_id = ?1), 0) + 1,
                ?2, ?3, ?3, ?4, ?5, ?6, ?7",
        rusqlite::params![task_id, provider, now, success as i64, error, prompt_tokens, completion_tokens],
    )?;
    conn.execute(
        "UPDATE tasks SET run_attempts = run_attempts + 1, run_last_error = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![error, now, task_id],
    )?;
    Ok(())
}

/// Return all active project IDs (used by the background scheduler).
pub fn list_projects_raw(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM projects WHERE active = 1")?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(ids)
}
