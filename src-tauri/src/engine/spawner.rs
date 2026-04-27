/// Centralized task creation with idempotency guarantees.
///
/// The TaskSpawner is the ONLY module that should create tasks programmatically.
/// All follow-up task creation, scheduler task creation, and batch task creation
/// flows through here to ensure consistent deduplication and validation.

use rusqlite::{Connection, OptionalExtension};

use crate::error::{Error, Result};
use crate::engine::task_store;
use crate::models::task::{
    AgentPolicy, ExecutionMode, Priority, Task, TaskArtifact, TaskRun, TaskStatus,
};

/// Specification for creating a task.
#[derive(Debug, Clone)]
pub struct TaskSpec {
    pub project_id: String,
    pub task_type: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// If None, uses default_phase() for the task_type
    pub phase: Option<String>,
    /// If None, uses default_execution_mode() for the task_type
    pub execution_mode: Option<ExecutionMode>,
    pub priority: Priority,
    pub agent_policy: AgentPolicy,
    pub depends_on: Vec<String>,
    pub artifacts: Vec<TaskArtifact>,
    /// For idempotency - prevents duplicate creation
    pub idempotency_key: Option<String>,
}

impl Default for TaskSpec {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            task_type: String::new(),
            title: None,
            description: None,
            phase: None,
            execution_mode: None,
            priority: Priority::Medium,
            agent_policy: AgentPolicy::None,
            depends_on: vec![],
            artifacts: vec![],
            idempotency_key: None,
        }
    }
}

/// Centralized task creation.
pub struct TaskSpawner;

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
            if let Some(existing) = Self::find_by_idempotency_key(conn, key)? {
                log::info!("[spawner] Idempotency key '{}' exists, returning existing task {}", 
                    key, existing.id);
                return Ok(existing);
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
        let execution_mode = spec
            .execution_mode
            .unwrap_or_else(|| crate::config::default_execution_mode(&spec.task_type));

        // 5. Build task
        let task = Task {
            id: id.clone(),
            project_id: spec.project_id,
            task_type: spec.task_type,
            phase,
            status: TaskStatus::Todo,
            priority: spec.priority,
            execution_mode,
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
            Self::record_idempotency_key(conn, &key, &id)?;
        }

        log::info!(
            "[spawner] Created task {} (type: {})",
            id,
            task.task_type
        );
        Ok(task)
    }

    /// Convenience method for follow-up tasks with built-in idempotency.
    ///
    /// This method checks if a follow-up task already exists for this parent
    /// and type before creating. If one exists in 'todo', 'in_progress', or 'review'
    /// status, it returns None.
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

        // Check for existing active task via idempotency key
        if let Some(existing) = Self::find_by_idempotency_key(conn, &key)? {
            // Check if existing task is still active
            if matches!(
                existing.status,
                TaskStatus::Todo | TaskStatus::InProgress | TaskStatus::Review
            ) {
                log::info!(
                    "[spawner] Follow-up '{}' already exists as active task {}, skipping",
                    key,
                    existing.id
                );
                return Ok(None);
            }
            // Task exists but is done/cancelled - we could create a new one,
            // but for now treat as duplicate to avoid spam
            log::info!(
                "[spawner] Follow-up '{}' exists but is {:?}, skipping to avoid spam",
                key,
                existing.status
            );
            return Ok(None);
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
                priority: Priority::Medium,
                agent_policy: AgentPolicy::Optional,
                ..Default::default()
            },
        )?;

        Ok(Some(task))
    }

    /// Find a task by its idempotency key.
    fn find_by_idempotency_key(conn: &Connection, key: &str) -> Result<Option<Task>> {
        let task_id: Option<String> = conn
            .query_row(
                "SELECT task_id FROM task_idempotency_keys WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;

        match task_id {
            Some(id) => match task_store::get_task(conn, &id) {
                Ok(task) => Ok(Some(task)),
                Err(_) => {
                    // Task was deleted but key remains - clean it up
                    let _ = conn.execute(
                        "DELETE FROM task_idempotency_keys WHERE key = ?1",
                        [key],
                    );
                    Ok(None)
                }
            },
            None => Ok(None),
        }
    }

    /// Record an idempotency key for a task.
    fn record_idempotency_key(conn: &Connection, key: &str, task_id: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO task_idempotency_keys (key, task_id, created_at) VALUES (?1, ?2, ?3)",
            [key, task_id, &chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Validate that all dependencies exist and are in the same project.
    fn validate_dependencies(
        conn: &Connection,
        deps: &[String],
        project_id: &str,
    ) -> Result<()> {
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
    use crate::models::task::{AgentPolicy, ExecutionMode, Priority, TaskStatus};

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
                execution_mode TEXT NOT NULL DEFAULT 'manual',
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
                created_at TEXT NOT NULL
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
            execution_mode: ExecutionMode::Automatic,
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
            execution_mode: ExecutionMode::Automatic,
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
}
