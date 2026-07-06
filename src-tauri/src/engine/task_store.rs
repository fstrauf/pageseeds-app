use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::models::task::{Priority, Task, TaskStatus};

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

// ─── Project CRUD ─────────────────────────────────────────────────────────────

use crate::models::project::Project;

pub fn list_projects(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, path, content_dir, site_url, site_id, sitemap_url, project_mode, active, agent_provider, seo_provider, clarity_project_id FROM projects ORDER BY name ASC",
    )?;
    let projects: Vec<Project> = stmt
        .query_map([], |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                content_dir: row.get(3)?,
                site_url: row.get(4)?,
                site_id: row.get(5)?,
                sitemap_url: row.get(6)?,
                project_mode: row.get(7)?,
                active: row.get::<_, i64>(8)? != 0,
                agent_provider: row.get(9)?,
                seo_provider: row.get(10)?,
                clarity_project_id: row.get(11)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(projects)
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Project> {
    conn.query_row(
        "SELECT id, name, path, content_dir, site_url, site_id, sitemap_url, project_mode, active, agent_provider, seo_provider, clarity_project_id FROM projects WHERE id = ?1",
        [id],
        |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                content_dir: row.get(3)?,
                site_url: row.get(4)?,
                site_id: row.get(5)?,
                sitemap_url: row.get(6)?,
                project_mode: row.get(7)?,
                active: row.get::<_, i64>(8)? != 0,
                agent_provider: row.get(9)?,
                seo_provider: row.get(10)?,
                clarity_project_id: row.get(11)?,
            })
        },
    )
    .map_err(|_| Error::Other(format!("Project '{id}' not found")))
}

pub fn create_project(conn: &Connection, project: &Project) -> Result<Project> {
    conn.execute(
        "INSERT INTO projects (id, name, path, content_dir, site_url, site_id, sitemap_url, project_mode, active, agent_provider, seo_provider, clarity_project_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            project.id,
            project.name,
            project.path,
            project.content_dir,
            project.site_url,
            project.site_id,
            project.sitemap_url,
            project.project_mode,
            project.active as i64,
            project.agent_provider,
            project.seo_provider,
            project.clarity_project_id,
        ],
    )?;
    get_project(conn, &project.id)
}

pub fn update_project(conn: &Connection, project: &Project) -> Result<Project> {
    let rows = conn.execute(
        "UPDATE projects SET name = ?1, path = ?2, content_dir = ?3, site_url = ?4, site_id = ?5, sitemap_url = ?6, project_mode = ?7, active = ?8, agent_provider = ?9, seo_provider = ?10, clarity_project_id = ?11
         WHERE id = ?12",
        rusqlite::params![
            project.name,
            project.path,
            project.content_dir,
            project.site_url,
            project.site_id,
            project.sitemap_url,
            project.project_mode,
            project.active as i64,
            project.agent_provider,
            project.seo_provider,
            project.clarity_project_id,
            project.id,
        ],
    )?;
    if rows == 0 {
        return Err(Error::Other(format!("Project '{}' not found", project.id)));
    }
    get_project(conn, &project.id)
}

pub fn delete_project(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM projects WHERE id = ?1", [id])?;
    Ok(())
}

// ─── Article queries ──────────────────────────────────────────────────────────

use crate::models::article::Article;

pub fn list_articles(conn: &Connection, project_id: &str) -> Result<Vec<Article>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                review_status, review_started_at, last_reviewed_at, review_count,
                content_gaps_addressed, estimated_traffic_monthly, page_type,
                content_hash, last_edited_at
         FROM articles WHERE project_id = ?1 ORDER BY id ASC",
    )?;
    let articles: Vec<Article> = stmt
        .query_map([project_id], |row| {
            let gaps_str: String = row.get(14)?;
            let gaps: Vec<String> = serde_json::from_str(&gaps_str).unwrap_or_default();
            Ok(Article {
                id: row.get(0)?,
                title: row.get(1)?,
                url_slug: row.get(2)?,
                file: row.get(3)?,
                target_keyword: row.get(4)?,
                keyword_difficulty: row.get(5)?,
                target_volume: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                published_date: row.get(7)?,
                word_count: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                status: row.get(9)?,
                review_status: row.get(10)?,
                review_started_at: row.get(11)?,
                last_reviewed_at: row.get(12)?,
                review_count: row.get::<_, Option<i64>>(13)?.unwrap_or(0),
                content_gaps_addressed: gaps,
                estimated_traffic_monthly: row.get(15)?,
                page_type: row.get(16)?,
                content_hash: row.get(17)?,
                last_edited_at: row.get(18)?,
                project_id: project_id.to_string(),
                quality_score: None,
                quality_grade: None,
                quality_rated_at: None,
                publishing_ready: None,
                quality_breakdown: None,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(articles)
}

/// Load all article url_slugs for a project as a lowercased HashSet.
///
/// This is the single source of truth for "does this slug exist in the project?"
/// checks. Use it instead of re-implementing the list_articles → collect pattern
/// in every module that validates internal link targets.
///
/// # Example
/// ```
/// use pageseeds_lib::engine::task_store::load_project_slug_set;
///
/// let slugs = load_project_slug_set(&conn, "proj-1").unwrap();
/// assert!(slugs.contains("my-post"));
/// ```
pub fn load_project_slug_set(
    conn: &Connection,
    project_id: &str,
) -> Result<std::collections::HashSet<String>> {
    let articles = list_articles(conn, project_id)?;
    Ok(articles.into_iter().map(|a| a.url_slug.to_lowercase()).collect())
}

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

// ─── Project overview stats ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusCounts {
    pub todo: i64,
    pub in_progress: i64,
    pub review: i64,
    pub done: i64,
    pub failed: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentTask {
    pub id: String,
    pub title: Option<String>,
    pub task_type: String,
    pub status: String,
    pub updated_at: String,
}

/// Landing page research task awaiting review with user context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingPageResearchPending {
    pub id: String,
    pub title: Option<String>,
    /// User-provided strategy context (parsed from description JSON)
    pub context: String,
    /// Themes being researched (parsed from description JSON or auto-derived)
    pub themes: Vec<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleStatusCounts {
    pub total: i64,
    pub published: i64,
    pub draft: i64,
    pub last_published_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowActivity {
    pub task_type: String,
    pub label: String,
    pub last_run_at: Option<String>,
    pub next_due_at: Option<String>,
    pub interval_hours: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectOverview {
    pub tasks: TaskStatusCounts,
    pub recent_tasks: Vec<RecentTask>,
    pub articles: ArticleStatusCounts,
    pub ready_task_count: i64,
    pub workflow_activity: Vec<WorkflowActivity>,
    /// Landing page research tasks in 'review' status awaiting user selection
    pub pending_landing_page_research: Vec<LandingPageResearchPending>,
    /// Feature spec tasks in 'review' status awaiting user confirmation
    pub pending_feature_specs: Vec<PendingFeatureSpec>,
    /// Fix tasks completed / failed since the most recent audit
    pub fix_summary: FixSummary,
    /// Comprehensive health snapshot: what still needs attention across all audits
    pub health_snapshot: crate::db::content_audit::HealthSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixSummary {
    pub completed: i64,
    pub failed: i64,
    pub pending: i64,
    /// Total articles with issues found in the most recent content audit
    /// (needs_improvement + poor). 0 if no audit exists.
    pub total_found: i64,
}

/// Comprehensive health snapshot showing outstanding issues across all audit types.
/// Replaces FixSummary as the primary health indicator in Overview.
pub use crate::db::content_audit::HealthSnapshot;

/// A generate_feature_spec task waiting for user confirmation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingFeatureSpec {
    pub id: String,
    pub title: Option<String>,
    pub updated_at: String,
}

pub fn get_project_overview(conn: &Connection, project_id: &str) -> Result<ProjectOverview> {
    // Task counts by status
    let counts: TaskStatusCounts = {
        let mut stmt = conn.prepare(
            "SELECT
               SUM(CASE WHEN status = 'todo' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'review' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END),
               COUNT(*)
             FROM tasks WHERE project_id = ?1",
        )?;
        stmt.query_row([project_id], |row| {
            Ok(TaskStatusCounts {
                todo: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                in_progress: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                review: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                done: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                failed: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                total: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            })
        })?
    };

    // 8 most recently updated tasks
    let recent_tasks: Vec<RecentTask> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, type, status, updated_at
             FROM tasks WHERE project_id = ?1
             ORDER BY updated_at DESC LIMIT 8",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok(RecentTask {
                id: row.get(0)?,
                title: row.get(1)?,
                task_type: row.get(2)?,
                status: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Todo tasks with no unresolved depends_on
    let ready_task_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND status = 'todo' AND depends_on = '[]'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Article stats
    let articles = {
        let mut stmt = conn.prepare(
            "SELECT
               COUNT(*),
               SUM(CASE WHEN status = 'published' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'draft' THEN 1 ELSE 0 END),
               MAX(CASE WHEN status = 'published' THEN published_date ELSE NULL END)
             FROM articles WHERE project_id = ?1",
        )?;
        stmt.query_row([project_id], |row| {
            Ok(ArticleStatusCounts {
                total: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                published: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                draft: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                last_published_date: row.get(3)?,
            })
        })?
    };

    // Workflow activity: last completed run per key workflow type + scheduler interval
    // Must stay in sync with QUICK_ACTIONS in src/components/overview/Overview.tsx
    let key_workflows: &[(&str, &str)] = &[
        ("collect_gsc", "GSC Collection"),
        ("research_keywords", "Keyword Research"),
        ("research_landing_pages", "Landing Page Research"),
        ("reddit_opportunity_search", "Reddit Search"),
        ("content_review", "Content Review"),
        ("cannibalization_audit", "Cannibalization Audit"),
        ("ctr_audit", "CTR Audit"),
        ("indexing_health_campaign", "Indexing Health Campaign"),
        ("content_cleanup", "Content Cleanup"),
        ("sanitize_content", "Sanitize Content"),
    ];

    // Build a map of task_type → last successful run finished_at from task_runs.
    // Using task_runs.finished_at is more accurate than tasks.updated_at,
    // which is also modified by edits, retries, and follow-up task creation.
    let mut last_run_map: std::collections::HashMap<String, String> = {
        let mut stmt = conn.prepare(
            "SELECT t.type, MAX(tr.finished_at)
             FROM tasks t
             JOIN task_runs tr ON t.id = tr.task_id
             WHERE t.project_id = ?1 AND tr.success = 1
             GROUP BY t.type",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Build a map of task_type → interval_hours from scheduler_rules
    let mut interval_map: std::collections::HashMap<String, i64> = {
        let mut stmt = conn.prepare(
            "SELECT task_type, interval_hours FROM scheduler_rules WHERE project_id = ?1",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let workflow_activity: Vec<WorkflowActivity> = key_workflows
        .iter()
        .map(|(task_type, label)| {
            let last_run_at = last_run_map.remove(*task_type);
            let interval_hours = interval_map.remove(*task_type);
            let next_due_at = last_run_at
                .as_ref()
                .zip(interval_hours)
                .and_then(|(ts, hrs)| {
                    ts.parse::<chrono::DateTime<chrono::Utc>>()
                        .ok()
                        .map(|dt| (dt + chrono::Duration::hours(hrs)).to_rfc3339())
                });
            WorkflowActivity {
                task_type: task_type.to_string(),
                label: label.to_string(),
                last_run_at,
                next_due_at,
                interval_hours,
            }
        })
        .collect();

    // Pending landing page research tasks in review status
    let pending_landing_page_research: Vec<LandingPageResearchPending> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, description, updated_at
             FROM tasks 
             WHERE project_id = ?1 AND type = 'research_landing_pages' AND status = 'review'
             ORDER BY updated_at DESC LIMIT 5",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            let description: Option<String> = row.get(2)?;
            let (context, themes) = parse_landing_page_description(description.as_deref());
            Ok(LandingPageResearchPending {
                id: row.get(0)?,
                title: row.get(1)?,
                context,
                themes,
                updated_at: row.get(3)?,
            })
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Pending feature spec tasks in review status — show only the most recent one
    // to avoid cluttering the Overview with old specs from before dedup fixes.
    let pending_feature_specs: Vec<PendingFeatureSpec> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, updated_at
             FROM tasks
             WHERE project_id = ?1 AND type = 'generate_feature_spec' AND status = 'review'
             ORDER BY updated_at DESC LIMIT 1",
        )?;
        let rows = stmt.query_map([project_id], |row| {
            Ok(PendingFeatureSpec {
                id: row.get(0)?,
                title: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;
        rows.filter_map(|r| r.ok()).collect()
    };

    // Count fix tasks since the most recent audit run
    let fix_summary: FixSummary = {
        let last_audit_at: Option<String> = conn.query_row(
            "SELECT MAX(tr.finished_at)
             FROM tasks t
             JOIN task_runs tr ON t.id = tr.task_id
             WHERE t.project_id = ?1 AND t.type IN ('content_review', 'indexing_health_campaign', 'ctr_audit') AND tr.success = 1",
            [project_id],
            |row| row.get(0),
        ).ok().flatten();

        let since_clause = match &last_audit_at {
            Some(ts) => format!("AND updated_at > '{}'", ts),
            None => String::new(),
        };

        let sql = format!(
            "SELECT
               SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END),
               SUM(CASE WHEN status IN ('todo', 'queued', 'in_progress') THEN 1 ELSE 0 END)
             FROM tasks
             WHERE project_id = ?1 AND type LIKE 'fix_%' {}",
            since_clause
        );

        let mut summary = conn.query_row(&sql, [project_id], |row| {
            Ok(FixSummary {
                completed: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                failed: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                pending: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                total_found: 0,
            })
        }).unwrap_or(FixSummary { completed: 0, failed: 0, pending: 0, total_found: 0 });

        // Read latest content audit from DB to get total articles with issues
        if let Ok(count) = crate::db::content_audit::count_unhealthy_articles(conn, project_id) {
            summary.total_found = count;
        }

        summary
    };

    let health_snapshot = crate::db::content_audit::get_health_snapshot(conn, project_id)
        .unwrap_or_default();

    log::info!(
        "[project_overview] tasks: total={} done={} todo={} in_progress={} review={} failed={} ready={}",
        counts.total, counts.done, counts.todo, counts.in_progress, counts.review, counts.failed,
        ready_task_count,
    );
    log::info!(
        "[project_overview] health_snapshot: poor={} needs={} good={} not_indexed={} ctr={} cann={} fix_done={} fix_failed={} fix_pending={} cooldown={} fix_review={}",
        health_snapshot.content_poor,
        health_snapshot.content_needs_improvement,
        health_snapshot.content_good,
        health_snapshot.indexing_not_indexed,
        health_snapshot.ctr_issue_count,
        health_snapshot.cannibalization_clusters,
        health_snapshot.fix_completed,
        health_snapshot.fix_failed,
        health_snapshot.fix_pending,
        health_snapshot.fix_on_cooldown,
        health_snapshot.fix_needs_review,
    );

    Ok(ProjectOverview {
        tasks: counts,
        recent_tasks,
        articles,
        ready_task_count,
        workflow_activity,
        pending_landing_page_research,
        pending_feature_specs,
        fix_summary,
        health_snapshot,
    })
}

/// Parse landing page research description JSON to extract context and themes.
fn parse_landing_page_description(desc: Option<&str>) -> (String, Vec<String>) {
    let desc = desc.unwrap_or("");

    // Try JSON format first
    if desc.trim().starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(desc) {
            let context = parsed
                .get("context")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let themes = parsed
                .get("themes")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();
            return (context, themes);
        }
    }

    // Fall back to treating entire description as context
    (desc.to_string(), vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn make_task(id: &str, project_id: &str, task_type: &str, status: TaskStatus) -> Task {
        let now = chrono::Utc::now().to_rfc3339();
        Task {
            id: id.to_string(),
            project_id: project_id.to_string(),
            task_type: task_type.to_string(),
            phase: "investigation".to_string(),
            status,
            priority: Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test Task".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
            not_before: None,
        }
    }

    /// Creating new tasks increases done count in get_project_overview.
    #[test]
    fn project_overview_task_counts_update_after_creating_tasks() {
        let conn = in_memory_db();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('p1', 'Test', '/tmp/test', 1, 'workspace')",
            [],
        ).unwrap();

        // Insert 3 done tasks from a previous audit
        create_task(&conn, &make_task("t1", "p1", "content_review", TaskStatus::Done)).unwrap();
        create_task(&conn, &make_task("t2", "p1", "indexing_health_campaign", TaskStatus::Done)).unwrap();
        create_task(&conn, &make_task("t3", "p1", "fix_content_article", TaskStatus::Done)).unwrap();

        // Insert 2 todo tasks
        create_task(&conn, &make_task("t4", "p1", "ctr_audit", TaskStatus::Todo)).unwrap();
        create_task(&conn, &make_task("t5", "p1", "fix_content_article", TaskStatus::Todo)).unwrap();

        let before = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(before.tasks.total, 5);
        assert_eq!(before.tasks.done, 3);
        assert_eq!(before.tasks.todo, 2);

        // Simulate a new "Run Full Audit" creating 2 new tasks
        create_task(&conn, &make_task("t6", "p1", "content_review", TaskStatus::Todo)).unwrap();
        create_task(&conn, &make_task("t7", "p1", "indexing_health_campaign", TaskStatus::Todo)).unwrap();

        let after = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(after.tasks.total, 7, "total should increase by 2");
        assert_eq!(after.tasks.todo, 4, "todo should increase by 2");
        assert_eq!(after.tasks.done, 3, "done should remain 3");
    }

    /// Completing the new audit tasks increases done and reduces todo.
    #[test]
    fn project_overview_reflects_completed_tasks() {
        let conn = in_memory_db();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('p1', 'Test', '/tmp/test', 1, 'workspace')",
            [],
        ).unwrap();

        // Initial state: 3 done tasks
        for i in 1..=3 {
            create_task(&conn, &make_task(&format!("t{}", i), "p1", "content_review", TaskStatus::Done)).unwrap();
        }

        let before = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(before.tasks.done, 3);

        // Create 2 new audit tasks as todo
        let t4 = make_task("t4", "p1", "content_review", TaskStatus::Todo);
        let t5 = make_task("t5", "p1", "indexing_health_campaign", TaskStatus::Todo);
        create_task(&conn, &t4).unwrap();
        create_task(&conn, &t5).unwrap();

        let mid = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(mid.tasks.todo, 2, "todo should be 2 after creating new tasks");
        assert_eq!(mid.tasks.total, 5);

        // Simulate queue runner completing both tasks
        update_task_status(&conn, "t4", TaskStatus::Done).unwrap();
        update_task_status(&conn, "t5", TaskStatus::Done).unwrap();

        let after = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(after.tasks.done, 5, "done should be 5 after completing new tasks");
        assert_eq!(after.tasks.todo, 0, "todo should be 0 after completing all");
    }
}
