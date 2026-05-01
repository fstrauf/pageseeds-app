/// Centralized task creation with idempotency guarantees.
///
/// The TaskSpawner is the ONLY module that should create tasks programmatically.
/// All follow-up task creation, scheduler task creation, and batch task creation
/// flows through here to ensure consistent deduplication and validation.
use rusqlite::{Connection, OptionalExtension};

use crate::engine::task_store;
use crate::error::{Error, Result};
use crate::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, Task, TaskArtifact, TaskRun, TaskReviewSurface,
    TaskRunPolicy, TaskStatus,
};

/// How the spawner should behave when an idempotency key matches an existing task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeduplicationPolicy {
    /// Always create a new task. Use only for genuinely one-off tasks.
    AlwaysCreate,

    /// Skip creation if ANY task exists with this key, regardless of status.
    SkipIfAnyExists,

    /// Skip only if an active (todo / queued / in_progress / review) task exists.
    /// If the existing task is done / failed / cancelled, create a new one.
    SkipIfActive,

    /// Skip if active. If done/failed/cancelled, allow re-creation only after
    /// the cooldown has expired. When expired, the old idempotency key is deleted.
    Cooldown {
        /// Cooldown duration in days.
        days: u32,
    },
}

impl Default for DeduplicationPolicy {
    fn default() -> Self {
        DeduplicationPolicy::SkipIfActive
    }
}

/// Specification for creating a task.
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub project_id: String,
    pub task_type: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// If None, uses default_phase() for the task_type
    pub phase: Option<String>,
    /// If None, uses default_run_policy() for the task_type
    pub run_policy: Option<TaskRunPolicy>,
    /// If None, uses default_review_surface() for the task_type
    pub review_surface: Option<TaskReviewSurface>,
    /// If None, uses default_follow_up_policy() for the task_type
    pub follow_up_policy: Option<FollowUpPolicy>,
    pub priority: Priority,
    pub agent_policy: AgentPolicy,
    pub depends_on: Vec<String>,
    pub artifacts: Vec<TaskArtifact>,
    /// For idempotency - prevents duplicate creation
    pub idempotency_key: Option<String>,
    /// How to handle duplicate keys. Defaults to SkipIfActive if a key is provided.
    pub dedup_policy: Option<DeduplicationPolicy>,
}

impl Default for TaskSpec {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            task_type: String::new(),
            title: None,
            description: None,
            phase: None,
            run_policy: None,
            review_surface: None,
            follow_up_policy: None,
            priority: Priority::Medium,
            agent_policy: AgentPolicy::None,
            depends_on: vec![],
            artifacts: vec![],
            idempotency_key: None,
            dedup_policy: None,
        }
    }
}

/// Centralized task creation.
pub struct TaskSpawner;

/// Result of evaluating an idempotency key against the database and policy.
enum IdempotencyResolution {
    /// No existing task, or policy allows creation. Proceed.
    Create,
    /// Existing task matches and policy says return it.
    ReturnExisting(Task),
    /// Existing task exists but is expired/stale. Delete old key then create.
    DeleteAndCreate,
}

impl TaskSpawner {
    /// Primary creation method. All task creation should go through here.
    ///
    /// # Arguments
    /// * `conn` - SQLite connection
    /// * `spec` - Task specification
    ///
    /// # Returns
    /// * `Ok(Task)` - Created task (or existing task if idempotency key matches)
    /// * `Err(Error)` - Validation failed or database error
    pub fn spawn(conn: &Connection, spec: TaskSpec) -> Result<Task> {
        // 1. Check idempotency if key provided
        if let Some(ref key) = spec.idempotency_key {
            let policy = spec.dedup_policy.unwrap_or_default();
            match Self::resolve_idempotency(conn, key, policy)? {
                IdempotencyResolution::Create => {
                    // Proceed to creation
                }
                IdempotencyResolution::ReturnExisting(task) => {
                    log::info!(
                        "[spawner] Idempotency key '{}' exists (policy: {:?}), returning existing task {}",
                        key,
                        policy,
                        task.id
                    );
                    return Ok(task);
                }
                IdempotencyResolution::DeleteAndCreate => {
                    // Old key expired or policy allows re-creation; delete stale key
                    let _ = conn.execute(
                        "DELETE FROM task_idempotency_keys WHERE key = ?1",
                        [key],
                    );
                }
            }
        }

        // 2. Validate dependencies exist and are in the same project
        Self::validate_dependencies(conn, &spec.depends_on, &spec.project_id)?;

        // 3. Generate ID and timestamps
        let now = chrono::Utc::now().to_rfc3339();
        let id = format!("task-{}", uuid::Uuid::new_v4());

        // 4. Resolve defaults
        let phase = spec
            .phase
            .unwrap_or_else(|| crate::config::default_phase(&spec.task_type).to_string());
        let run_policy = spec
            .run_policy
            .unwrap_or_else(|| crate::config::default_run_policy(&spec.task_type));
        let review_surface = spec
            .review_surface
            .unwrap_or_else(|| crate::config::default_review_surface(&spec.task_type));
        let follow_up_policy = spec
            .follow_up_policy
            .unwrap_or_else(|| crate::config::default_follow_up_policy(&spec.task_type));

        // 5. Build task
        let task = Task {
            id: id.clone(),
            project_id: spec.project_id,
            task_type: spec.task_type,
            phase,
            status: TaskStatus::Todo,
            priority: spec.priority,
            run_policy,
            review_surface,
            follow_up_policy,
            agent_policy: spec.agent_policy,
            title: spec.title,
            description: spec.description,
            depends_on: spec.depends_on,
            artifacts: spec.artifacts,
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
        };

        // 6. Persist
        task_store::create_task(conn, &task)?;

        // 7. Record idempotency key if provided
        if let Some(key) = spec.idempotency_key {
            let expires_at = spec.dedup_policy.and_then(|p| match p {
                DeduplicationPolicy::Cooldown { days } => {
                    Some((chrono::Utc::now() + chrono::Duration::days(days as i64)).to_rfc3339())
                }
                _ => None,
            });
            Self::record_idempotency_key(conn, &key, &id, expires_at.as_deref())?;
        }

        log::info!("[spawner] Created task {} (type: {})", id, task.task_type);
        Ok(task)
    }

    /// Convenience method for follow-up tasks with built-in idempotency.
    ///
    /// Uses SkipIfActive policy: blocks if an active task exists, but allows
    /// re-creation if the previous task is done / failed / cancelled.
    ///
    /// # Arguments
    /// * `conn` - SQLite connection
    /// * `parent` - The parent task that triggered this follow-up
    /// * `task_type` - Type of follow-up task to create
    /// * `title` - Title for the new task
    ///
    /// # Returns
    /// * `Ok(Some(Task))` - Task was created
    /// * `Ok(None)` - Task already exists (not an error)
    /// * `Err(Error)` - Database or validation error
    pub fn spawn_follow_up(
        conn: &Connection,
        parent: &Task,
        task_type: &str,
        title: &str,
    ) -> Result<Option<Task>> {
        // Generate deterministic idempotency key
        let key = format!("followup:{}:{}:{}", parent.id, task_type, title);

        match Self::resolve_idempotency(conn, &key, DeduplicationPolicy::SkipIfActive)? {
            IdempotencyResolution::Create => {
                // Proceed
            }
            IdempotencyResolution::ReturnExisting(existing) => {
                log::info!(
                    "[spawner] Follow-up '{}' already exists as active task {}, skipping",
                    key,
                    existing.id
                );
                return Ok(None);
            }
            IdempotencyResolution::DeleteAndCreate => {
                let _ = conn.execute(
                    "DELETE FROM task_idempotency_keys WHERE key = ?1",
                    [&key],
                );
            }
        }

        let task = Self::spawn(
            conn,
            TaskSpec {
                project_id: parent.project_id.clone(),
                task_type: task_type.to_string(),
                title: Some(title.to_string()),
                description: Some(format!("Follow-up from task {}", parent.id)),
                depends_on: vec![parent.id.clone()],
                idempotency_key: Some(key),
                dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
                priority: Priority::Medium,
                agent_policy: AgentPolicy::Optional,
                ..Default::default()
            },
        )?;

        Ok(Some(task))
    }

    /// Evaluate whether a task should be created, returned, or have its stale key deleted.
    fn resolve_idempotency(
        conn: &Connection,
        key: &str,
        policy: DeduplicationPolicy,
    ) -> Result<IdempotencyResolution> {
        // Look up key + expiration
        let row: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT task_id, expires_at FROM task_idempotency_keys WHERE key = ?1",
                [key],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()?;

        let (task_id, expires_at) = match row {
            Some((id, exp)) => (id, exp),
            None => return Ok(IdempotencyResolution::Create),
        };

        // Check if key is expired
        if let Some(exp) = expires_at {
            if let Ok(exp_dt) = chrono::DateTime::parse_from_rfc3339(&exp) {
                if chrono::Utc::now() > exp_dt.with_timezone(&chrono::Utc) {
                    log::info!(
                        "[spawner] Idempotency key '{}' expired ({}), allowing re-creation",
                        key,
                        exp
                    );
                    return Ok(IdempotencyResolution::DeleteAndCreate);
                }
            }
        }

        // Load the task
        let task = match task_store::get_task(conn, &task_id) {
            Ok(t) => t,
            Err(_) => {
                // Task was deleted but key remains - clean it up and allow creation
                let _ = conn.execute(
                    "DELETE FROM task_idempotency_keys WHERE key = ?1",
                    [key],
                );
                return Ok(IdempotencyResolution::Create);
            }
        };

        match policy {
            DeduplicationPolicy::AlwaysCreate => Ok(IdempotencyResolution::Create),
            DeduplicationPolicy::SkipIfAnyExists => {
                Ok(IdempotencyResolution::ReturnExisting(task))
            }
            DeduplicationPolicy::SkipIfActive => {
                if matches!(
                    task.status,
                    TaskStatus::Todo | TaskStatus::Queued | TaskStatus::InProgress | TaskStatus::Review
                ) {
                    Ok(IdempotencyResolution::ReturnExisting(task))
                } else {
                    // Task is done/failed/cancelled - allow re-creation
                    Ok(IdempotencyResolution::DeleteAndCreate)
                }
            }
            DeduplicationPolicy::Cooldown { .. } => {
                if matches!(
                    task.status,
                    TaskStatus::Todo | TaskStatus::Queued | TaskStatus::InProgress | TaskStatus::Review
                ) {
                    Ok(IdempotencyResolution::ReturnExisting(task))
                } else {
                    // For Cooldown, expiration is checked above. If not expired,
                    // return existing; if expired, DeleteAndCreate was already returned.
                    Ok(IdempotencyResolution::ReturnExisting(task))
                }
            }
        }
    }

    /// Record an idempotency key for a task.
    fn record_idempotency_key(
        conn: &Connection,
        key: &str,
        task_id: &str,
        expires_at: Option<&str>,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO task_idempotency_keys (key, task_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                key,
                task_id,
                chrono::Utc::now().to_rfc3339(),
                expires_at,
            ],
        )?;
        Ok(())
    }

    /// Validate that all dependencies exist and are in the same project.
    fn validate_dependencies(conn: &Connection, deps: &[String], project_id: &str) -> Result<()> {
        for dep_id in deps {
            let dep = task_store::get_task(conn, dep_id)?;
            if dep.project_id != project_id {
                return Err(Error::Other(format!(
                    "Cross-project dependency not allowed: task {} is in project {} but depends on {} in project {}",
                    dep_id, dep.project_id, dep_id, project_id
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{AgentPolicy, Priority, TaskStatus, TaskReviewSurface, FollowUpPolicy};

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Minimal schema for testing
        conn.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                active INTEGER DEFAULT 1
            );
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
                review_surface TEXT NOT NULL DEFAULT 'none',
                follow_up_policy TEXT NOT NULL DEFAULT 'none',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT,
                description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER DEFAULT 0,
                run_last_error TEXT,
                run_provider TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE task_idempotency_keys (
                key TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT
            );
            CREATE TABLE task_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id TEXT NOT NULL,
                attempt INTEGER NOT NULL,
                provider TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                success INTEGER,
                error TEXT,
                prompt_tokens INTEGER,
                completion_tokens INTEGER
            );",
        )
        .unwrap();
        conn
    }

    fn create_test_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES (?1, ?2, ?3, 1)",
            [id, "Test Project", "/tmp/test"],
        )
        .unwrap();
    }

    #[test]
    fn spawn_creates_task() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        let task = TaskSpawner::spawn(
            &conn,
            TaskSpec {
                project_id: "proj1".to_string(),
                task_type: "test_task".to_string(),
                title: Some("Test".to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(task.task_type, "test_task");
        assert_eq!(task.status, TaskStatus::Todo);
        assert_eq!(task.project_id, "proj1");
    }

    #[test]
    fn spawn_with_idempotency_key_prevents_duplicates() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        let spec = TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "test_task".to_string(),
            title: Some("Test".to_string()),
            idempotency_key: Some("unique-key-123".to_string()),
            ..Default::default()
        };

        let task1 = TaskSpawner::spawn(&conn, spec.clone()).unwrap();
        let task2 = TaskSpawner::spawn(&conn, spec).unwrap();

        assert_eq!(task1.id, task2.id); // Same task returned
    }

    #[test]
    fn spawn_follow_up_is_idempotent() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        // Create parent task directly
        let parent = Task {
            id: "parent-123".to_string(),
            project_id: "proj1".to_string(),
            task_type: "parent_task".to_string(),
            phase: "test".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: Some("Parent".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        task_store::create_task(&conn, &parent).unwrap();

        // First call creates
        let follow1 = TaskSpawner::spawn_follow_up(&conn, &parent, "child_task", "Child").unwrap();
        assert!(follow1.is_some());

        // Second call returns None (already exists)
        let follow2 = TaskSpawner::spawn_follow_up(&conn, &parent, "child_task", "Child").unwrap();
        assert!(follow2.is_none());
    }

    #[test]
    fn cross_project_dependency_fails() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");
        create_test_project(&conn, "proj2");

        // Create task in proj1
        let dep_task = Task {
            id: "dep-123".to_string(),
            project_id: "proj1".to_string(), // Different project
            task_type: "dep_task".to_string(),
            phase: "test".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: None,
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        task_store::create_task(&conn, &dep_task).unwrap();

        // Try to create task in proj2 that depends on proj1 task
        let result = TaskSpawner::spawn(
            &conn,
            TaskSpec {
                project_id: "proj2".to_string(),
                task_type: "test_task".to_string(),
                depends_on: vec!["dep-123".to_string()],
                ..Default::default()
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn spawn_with_skip_if_active_allows_recreate_after_done() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        let spec = TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "test_task".to_string(),
            title: Some("Test".to_string()),
            idempotency_key: Some("skip-active-key".to_string()),
            dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
            ..Default::default()
        };

        let task1 = TaskSpawner::spawn(&conn, spec.clone()).unwrap();
        assert_eq!(task1.status, TaskStatus::Todo);

        // Same key, active status → returns existing
        let task2 = TaskSpawner::spawn(&conn, spec.clone()).unwrap();
        assert_eq!(task1.id, task2.id);

        // Mark as done
        task_store::update_task_status(&conn, &task1.id, TaskStatus::Done).unwrap();

        // Same key, done status → creates new task
        let task3 = TaskSpawner::spawn(&conn, spec).unwrap();
        assert_ne!(task1.id, task3.id);
    }

    #[test]
    fn spawn_with_skip_if_any_exists_blocks_forever() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        let spec = TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "test_task".to_string(),
            title: Some("Test".to_string()),
            idempotency_key: Some("skip-any-key".to_string()),
            dedup_policy: Some(DeduplicationPolicy::SkipIfAnyExists),
            ..Default::default()
        };

        let task1 = TaskSpawner::spawn(&conn, spec.clone()).unwrap();
        task_store::update_task_status(&conn, &task1.id, TaskStatus::Done).unwrap();

        // Even though task is done, SkipIfAnyExists blocks re-creation
        let task2 = TaskSpawner::spawn(&conn, spec).unwrap();
        assert_eq!(task1.id, task2.id);
    }

    #[test]
    fn spawn_with_cooldown_blocks_within_period() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        let spec = TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "test_task".to_string(),
            title: Some("Test".to_string()),
            idempotency_key: Some("cooldown-key".to_string()),
            dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 7 }),
            ..Default::default()
        };

        let task1 = TaskSpawner::spawn(&conn, spec.clone()).unwrap();
        task_store::update_task_status(&conn, &task1.id, TaskStatus::Done).unwrap();

        // Within cooldown → returns existing
        let task2 = TaskSpawner::spawn(&conn, spec.clone()).unwrap();
        assert_eq!(task1.id, task2.id);
    }

    #[test]
    fn spawn_with_cooldown_allows_recreate_after_expiry() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        let key = "cooldown-expired-key";
        let spec = TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "test_task".to_string(),
            title: Some("Test".to_string()),
            idempotency_key: Some(key.to_string()),
            dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 7 }),
            ..Default::default()
        };

        let task1 = TaskSpawner::spawn(&conn, spec.clone()).unwrap();
        task_store::update_task_status(&conn, &task1.id, TaskStatus::Done).unwrap();

        // Manually expire the idempotency key
        let expired = (chrono::Utc::now() - chrono::Duration::days(8)).to_rfc3339();
        conn.execute(
            "UPDATE task_idempotency_keys SET expires_at = ?1 WHERE key = ?2",
            [&expired, key],
        )
        .unwrap();

        // After expiry → creates new task
        let task2 = TaskSpawner::spawn(&conn, spec).unwrap();
        assert_ne!(task1.id, task2.id);
    }

    #[test]
    fn spawn_follow_up_allows_recreate_after_failure() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        let parent = Task {
            id: "parent-456".to_string(),
            project_id: "proj1".to_string(),
            task_type: "parent_task".to_string(),
            phase: "test".to_string(),
            status: TaskStatus::Done,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: Some("Parent".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        task_store::create_task(&conn, &parent).unwrap();

        // First follow-up creates
        let follow1 = TaskSpawner::spawn_follow_up(&conn, &parent, "child_task", "Child").unwrap();
        assert!(follow1.is_some());
        let child_id = follow1.unwrap().id;

        // Mark child as failed
        task_store::update_task_status(&conn, &child_id, TaskStatus::Failed).unwrap();

        // Second follow-up should create a new one (SkipIfActive allows re-creation)
        let follow2 = TaskSpawner::spawn_follow_up(&conn, &parent, "child_task", "Child").unwrap();
        assert!(follow2.is_some());
        assert_ne!(child_id, follow2.unwrap().id);
    }

    #[test]
    fn orphan_idempotency_key_is_cleaned_up() {
        let conn = in_memory_db();
        create_test_project(&conn, "proj1");

        // Insert an orphan key manually (no matching task)
        conn.execute(
            "INSERT INTO task_idempotency_keys (key, task_id, created_at) VALUES (?1, ?2, ?3)",
            ["orphan-key", "nonexistent-task", &chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();

        let spec = TaskSpec {
            project_id: "proj1".to_string(),
            task_type: "test_task".to_string(),
            title: Some("Test".to_string()),
            idempotency_key: Some("orphan-key".to_string()),
            ..Default::default()
        };

        // Should create a new task, not fail
        let task = TaskSpawner::spawn(&conn, spec).unwrap();
        assert_eq!(task.task_type, "test_task");
    }
}
