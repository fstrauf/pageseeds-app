# Task Queue V2 Spec: Centralized Spawner & Cross-Project Queue

**Status:** Draft  
**Date:** 2026-03-25  
**Goal:** Enable cross-project task queuing with automatic follow-up execution through a centralized, idempotent task creation system.

---

## 1. Problem Statement

### Current Pain Points
1. **Inconsistent follow-up task creation**: Logic scattered across `executor.rs`, `content.rs`, `gsc.rs`, and `tasks.rs`
2. **Ad-hoc deduplication**: Each spawner implements its own (different) duplicate check
3. **No auto-queuing**: Follow-up tasks are returned but not automatically executed
4. **Project-scoped batch**: Cannot queue tasks across multiple projects
5. **Silent duplication risk**: Retrying a task can create duplicate follow-ups

### Success Criteria
- [ ] One module owns all task creation (`TaskSpawner`)
- [ ] Idempotent follow-up creation (retry-safe)
- [ ] Cross-project queue execution
- [ ] Automatic follow-up queuing for `automatic`/`batchable` tasks
- [ ] No duplicate tasks when retrying failed parent tasks

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                      FRONTEND (TypeScript)                       │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │  TaskBoard   │  │ TaskDetail   │  │   GlobalQueueStore   │   │
│  │  "Add to Q"  │  │ "Add to Q"   │  │   (Zustand)          │   │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘   │
│         │                 │                     │               │
│         └─────────────────┴─────────────────────┘               │
│                             │                                    │
│                    ┌────────▼────────┐                          │
│                    │  TaskRunner     │                          │
│                    │  (runs queue)   │                          │
│                    └────────┬────────┘                          │
└─────────────────────────────┼──────────────────────────────────┘
                              │ invoke
┌─────────────────────────────┼──────────────────────────────────┐
│                         RUST BACKEND                            │
│                              │                                   │
│  ┌───────────────────────────▼──────────────────────────────┐   │
│  │              commands::execute_queue (NEW)               │   │
│  │         Accepts Vec<QueueItem> across projects           │   │
│  └───────────────────────────┬──────────────────────────────┘   │
│                              │                                   │
│  ┌───────────────────────────▼──────────────────────────────┐   │
│  │              engine::executor (MODIFIED)                 │   │
│  │    Returns ExecutionResult with follow_up_tasks          │   │
│  └───────────────────────────┬──────────────────────────────┘   │
│                              │                                   │
│  ┌───────────────────────────▼──────────────────────────────┐   │
│  │              engine::spawner (NEW MODULE)                │   │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────────────┐  │   │
│  │  │   spawn    │  │find_duplicate│ │  spawn_follow_up   │  │   │
│  │  │            │  │            │  │  (idempotent)      │  │   │
│  │  └────────────┘  └────────────┘  └────────────────────┘  │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                   │
│  ┌───────────────────────────▼──────────────────────────────┐   │
│  │              db::task_store (EXISTING)                   │   │
│  │              SQLite - source of truth                     │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. Rust Implementation

### 3.1 New Module: `engine::spawner`

**Location:** `src-tauri/src/engine/spawner.rs`

**Responsibility:** The ONLY module allowed to create tasks programmatically. All follow-up task creation, batch task creation, and scheduler task creation flows through here.

```rust
use rusqlite::Connection;
use crate::error::Result;
use crate::models::task::{Task, TaskStatus, ExecutionMode, Priority, AgentPolicy, TaskRun};
use crate::engine::task_store;

/// Specification for creating a task
pub struct TaskSpec {
    pub project_id: String,
    pub task_type: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub phase: Option<String>,  // None = use default_phase()
    pub execution_mode: Option<ExecutionMode>, // None = use default
    pub priority: Priority,
    pub agent_policy: AgentPolicy,
    pub depends_on: Vec<String>,
    pub artifacts: Vec<TaskArtifact>,
    /// For idempotency - prevents duplicate creation
    pub idempotency_key: Option<String>,
}

/// Centralized task creation
pub struct TaskSpawner;

impl TaskSpawner {
    /// Primary creation method. All task creation goes through here.
    pub fn spawn(conn: &Connection, spec: TaskSpec) -> Result<Task> {
        // 1. Check idempotency if key provided
        if let Some(key) = &spec.idempotency_key {
            if Self::is_key_seen(conn, key)? {
                log::info!("[spawner] Idempotency key '{}' exists, skipping creation", key);
                return Self::find_by_idempotency_key(conn, key)
                    .ok_or_else(|| Error::Other("Idempotency key exists but task not found".into()));
            }
        }
        
        // 2. Validate dependencies exist and are completed (or same project)
        Self::validate_dependencies(conn, &spec.depends_on, &spec.project_id)?;
        
        // 3. Generate ID and timestamps
        let now = chrono::Utc::now().to_rfc3339();
        let id = format!("task-{}", chrono::Utc::now().timestamp_millis());
        
        // 4. Resolve defaults
        let phase = spec.phase.unwrap_or_else(|| 
            crate::config::default_phase(&spec.task_type).to_string()
        );
        let execution_mode = spec.execution_mode.unwrap_or_else(||
            crate::config::default_execution_mode(&spec.task_type)
        );
        
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
        
        log::info!("[spawner] Created task {} (type: {})", id, task.task_type);
        Ok(task)
    }
    
    /// Convenience method for follow-up tasks with built-in idempotency
    pub fn spawn_follow_up(
        conn: &Connection,
        parent: &Task,
        task_type: &str,
        title: &str,
    ) -> Result<Option<Task>> {
        // Generate deterministic idempotency key
        let key = format!("followup:{}:{}:{}", parent.id, task_type, title);
        
        // Check for existing active task
        let existing = conn.query_row(
            "SELECT t.id FROM tasks t 
             JOIN task_idempotency_keys k ON t.id = k.task_id 
             WHERE k.key = ?1 AND t.status IN ('todo', 'in_progress', 'review')",
            [&key],
            |r| r.get::<_, String>(0),
        ).optional()?;
        
        if existing.is_some() {
            log::info!("[spawner] Follow-up '{}' already exists, skipping", key);
            return Ok(None);
        }
        
        let task = Self::spawn(conn, TaskSpec {
            project_id: parent.project_id.clone(),
            task_type: task_type.to_string(),
            title: Some(title.to_string()),
            description: Some(format!("Follow-up from task {}", parent.id)),
            depends_on: vec![parent.id.clone()],
            idempotency_key: Some(key),
            priority: Priority::Medium,
            agent_policy: AgentPolicy::Optional,
            ..Default::default()
        })?;
        
        Ok(Some(task))
    }
    
    /// Spawn tasks from GSC collection results (migrates existing logic)
    pub fn spawn_from_gsc_collection(
        conn: &Connection,
        parent: &Task,
        collection: &serde_json::Value,
    ) -> Result<Vec<Task>> {
        // Migrate logic from engine/exec/gsc.rs::create_tasks_from_collection
        // But use Self::spawn() for each task creation
    }
    
    /// Spawn article tasks from keyword research (migrates existing logic)
    pub fn spawn_article_tasks_from_keywords(
        conn: &Connection,
        research_task: &Task,
        keywords: &[String],
    ) -> Result<Vec<Task>> {
        // Migrate logic from commands/tasks.rs::create_article_tasks_from_keywords
        // But use Self::spawn() for each task creation
    }
    
    // Private helpers
    fn is_key_seen(conn: &Connection, key: &str) -> Result<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM task_idempotency_keys WHERE key = ?1",
            [key],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }
    
    fn record_idempotency_key(conn: &Connection, key: &str, task_id: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO task_idempotency_keys (key, task_id, created_at) VALUES (?1, ?2, ?3)",
            [key, task_id, &chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
    
    fn validate_dependencies(
        conn: &Connection,
        deps: &[String],
        project_id: &str,
    ) -> Result<()> {
        for dep_id in deps {
            let dep = task_store::get_task(conn, dep_id)?;
            if dep.project_id != project_id {
                return Err(Error::Other(format!(
                    "Cross-project dependency not allowed: {} -> {}",
                    dep_id, project_id
                )));
            }
        }
        Ok(())
    }
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
```

### 3.2 Database Migration

**Add to `db/mod.rs`:**

```rust
const MIGRATION_VX: &str = r#"
-- Idempotency tracking for task creation
CREATE TABLE IF NOT EXISTS task_idempotency_keys (
    key TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

-- Index for fast lookups
CREATE INDEX IF NOT EXISTS idx_idempotency_task ON task_idempotency_keys(task_id);

-- Cleanup old keys (keep 30 days)
-- Note: We'll add a pruning function, not a trigger, to avoid complexity
"#;
```

### 3.3 Modified: `engine::executor`

**Changes to `execute_task_with_token`:**

```rust
// In the follow-up task creation section (around line 346-366)

// After a successful content review, create a single content_review_apply task
if all_ok && matches!(task.task_type.as_str(), "content_review" | "content_audit") {
    if let Some(task) = crate::engine::spawner::TaskSpawner::spawn_follow_up(
        conn, 
        &task, 
        "content_review_apply",
        "Apply content review recommendations"
    )? {
        follow_up_ids.push(task.id);
    }
}

// After a successful write_article, queue a cluster_and_link task
if all_ok && task.task_type == "write_article" {
    if let Some(task) = crate::engine::spawner::TaskSpawner::spawn_follow_up(
        conn,
        &task,
        "cluster_and_link",
        &format!("Cluster and link: {}", task.title.as_deref().unwrap_or("article"))
    )? {
        follow_up_ids.push(task.id);
    }
}

// After a successful collect_gsc, spawn fix tasks
if all_ok && task.task_type == "collect_gsc" {
    let spawned = crate::engine::spawner::TaskSpawner::spawn_from_gsc_collection(
        conn, &task, &collection_json
    )?;
    follow_up_ids.extend(spawned.into_iter().map(|t| t.id));
}
```

**Remove:** Direct calls to `task_store::create_task` for follow-ups (move to spawner).

### 3.4 New Command: `execute_queue`

**Location:** `src-tauri/src/commands/executor.rs` (new file) or add to existing commands

```rust
use tauri::State;
use serde::{Deserialize, Serialize};
use crate::AppState;
use crate::engine::executor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub task_id: String,
    pub project_id: String,
    pub title: String,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueProgressEvent {
    pub event_type: String, // "started", "step_progress", "completed", "failed"
    pub task_id: String,
    pub project_id: String,
    pub payload: serde_json::Value,
}

/// Execute a queue of tasks across projects
/// This runs in a background thread and emits events via Tauri
#[tauri::command]
pub async fn execute_queue(
    items: Vec<QueueItem>,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let db_path = state.db_path.clone();
    
    // Spawn background task
    tokio::spawn(async move {
        for (index, item) in items.iter().enumerate() {
            // Emit "started" event
            let _ = app_handle.emit("queue:task-started", &QueueProgressEvent {
                event_type: "started".to_string(),
                task_id: item.task_id.clone(),
                project_id: item.project_id.clone(),
                payload: serde_json::json!({ "index": index }),
            });
            
            // Open fresh connection per task (avoids lock contention)
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => c,
                Err(e) => {
                    let _ = app_handle.emit("queue:task-failed", &QueueProgressEvent {
                        event_type: "failed".to_string(),
                        task_id: item.task_id.clone(),
                        project_id: item.project_id.clone(),
                        payload: serde_json::json!({ "error": e.to_string() }),
                    });
                    continue;
                }
            };
            
            // Execute task
            match executor::execute_task_with_token(
                &conn,
                &item.task_id,
                None, // gsc_token - could be passed per-task if needed
                Some(app_handle.clone()),
                false,
            ) {
                Ok(result) => {
                    let event_type = if result.success { "completed" } else { "failed" };
                    let _ = app_handle.emit("queue:task-completed", &QueueProgressEvent {
                        event_type: event_type.to_string(),
                        task_id: item.task_id.clone(),
                        project_id: item.project_id.clone(),
                        payload: serde_json::to_value(&result).unwrap_or_default(),
                    });
                    
                    // Handle auto-queueing of follow-ups
                    if result.success {
                        for follow_up in &result.follow_up_tasks {
                            if follow_up.execution_mode == "automatic" || follow_up.execution_mode == "batchable" {
                                let _ = app_handle.emit("queue:follow-up-created", &follow_up);
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = app_handle.emit("queue:task-failed", &QueueProgressEvent {
                        event_type: "failed".to_string(),
                        task_id: item.task_id.clone(),
                        project_id: item.project_id.clone(),
                        payload: serde_json::json!({ "error": e }),
                    });
                }
            }
        }
        
        // Emit queue finished
        let _ = app_handle.emit("queue:finished", ());
    });
    
    Ok(())
}
```

---

## 4. Frontend Implementation

### 4.1 Global Queue Store

**Location:** `src/stores/queueStore.ts`

```typescript
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Task, ExecutionResult, FollowUpTask } from '@/lib/types';

interface QueueItem {
  taskId: string;
  projectId: string;
  projectName: string;
  title: string;
  taskType: string;
  status: 'pending' | 'running' | 'completed' | 'failed';
  result?: ExecutionResult;
  liveSteps?: StepProgress[];
}

interface QueueState {
  items: QueueItem[];
  currentIndex: number;
  isRunning: boolean;
  isPaused: boolean;
  
  // Actions
  addTask: (task: Task, projectName: string) => void;
  addTasks: (tasks: Task[], projectName: string) => void;
  removeTask: (taskId: string) => void;
  reorderTasks: (newOrder: QueueItem[]) => void;
  clearCompleted: () => void;
  clearAll: () => void;
  
  // Execution
  startQueue: () => Promise<void>;
  pauseQueue: () => void;
  resumeQueue: () => void;
  stopQueue: () => void;
  
  // Event handlers (internal)
  onTaskStarted: (taskId: string) => void;
  onTaskCompleted: (taskId: string, result: ExecutionResult) => void;
  onTaskFailed: (taskId: string, error: string) => void;
  onFollowUpCreated: (followUp: FollowUpTask, projectId: string) => void;
}

export const useQueueStore = create<QueueState>((set, get) => ({
  items: [],
  currentIndex: 0,
  isRunning: false,
  isPaused: false,
  
  addTask: (task, projectName) => {
    set(state => ({
      items: [...state.items, {
        taskId: task.id,
        projectId: task.project_id,
        projectName,
        title: task.title || task.task_type,
        taskType: task.task_type,
        status: 'pending',
      }]
    }));
  },
  
  addTasks: (tasks, projectName) => {
    const newItems = tasks.map(task => ({
      taskId: task.id,
      projectId: task.project_id,
      projectName,
      title: task.title || task.task_type,
      taskType: task.task_type,
      status: 'pending' as const,
    }));
    set(state => ({ items: [...state.items, ...newItems] }));
  },
  
  removeTask: (taskId) => {
    set(state => ({
      items: state.items.filter(i => i.taskId !== taskId)
    }));
  },
  
  clearCompleted: () => {
    set(state => ({
      items: state.items.filter(i => i.status !== 'completed' && i.status !== 'failed')
    }));
  },
  
  clearAll: () => {
    set({ items: [], currentIndex: 0, isRunning: false, isPaused: false });
  },
  
  startQueue: async () => {
    const { items } = get();
    if (items.length === 0) return;
    
    set({ isRunning: true, isPaused: false });
    
    // Subscribe to events
    const unlistenStarted = await listen('queue:task-started', (event) => {
      const payload = event.payload as QueueProgressEvent;
      get().onTaskStarted(payload.task_id);
    });
    
    const unlistenCompleted = await listen('queue:task-completed', (event) => {
      const payload = event.payload as QueueProgressEvent;
      if (payload.event_type === 'completed') {
        get().onTaskCompleted(payload.task_id, payload.payload as ExecutionResult);
      } else {
        get().onTaskFailed(payload.task_id, 'Task execution failed');
      }
    });
    
    const unlistenFollowUp = await listen('queue:follow-up-created', (event) => {
      const followUp = event.payload as FollowUpTask;
      // Need to determine projectId - this comes from the parent task
      // The payload should include it
    });
    
    const unlistenFinished = await listen('queue:finished', () => {
      set({ isRunning: false, currentIndex: 0 });
      unlistenStarted();
      unlistenCompleted();
      unlistenFollowUp();
      unlistenFinished();
    });
    
    // Invoke Rust command
    await invoke('execute_queue', { 
      items: items.map(i => ({
        taskId: i.taskId,
        projectId: i.projectId,
        title: i.title,
        taskType: i.taskType,
      }))
    });
  },
  
  pauseQueue: () => set({ isPaused: true }),
  resumeQueue: () => set({ isPaused: false }),
  stopQueue: () => set({ isRunning: false, isPaused: false }),
  
  onTaskStarted: (taskId) => {
    set(state => ({
      items: state.items.map(i => 
        i.taskId === taskId ? { ...i, status: 'running' as const } : i
      )
    }));
  },
  
  onTaskCompleted: (taskId, result) => {
    set(state => ({
      items: state.items.map(i => 
        i.taskId === taskId ? { ...i, status: 'completed' as const, result } : i
      ),
      currentIndex: state.currentIndex + 1
    }));
  },
  
  onTaskFailed: (taskId, error) => {
    set(state => ({
      items: state.items.map(i => 
        i.taskId === taskId ? { ...i, status: 'failed' as const } : i
      ),
      currentIndex: state.currentIndex + 1
    }));
  },
  
  onFollowUpCreated: (followUp, projectId) => {
    // Auto-append follow-up tasks to queue
    set(state => ({
      items: [...state.items, {
        taskId: followUp.id,
        projectId,
        projectName: 'Auto-created', // Should lookup actual name
        title: followUp.title,
        taskType: followUp.task_type,
        status: 'pending' as const,
      }]
    }));
  },
}));
```

### 4.2 UI Components

**TaskRunner Update:**

Modify existing `TaskRunner.tsx` to use the global queue store instead of single-task execution.

**Add "Add to Queue" Buttons:**

```typescript
// In TaskDetail.tsx and TaskBoard row actions
import { useQueueStore } from '@/stores/queueStore';

function AddToQueueButton({ task, projectName }: { task: Task; projectName: string }) {
  const addTask = useQueueStore(s => s.addTask);
  const isQueued = useQueueStore(s => s.items.some(i => i.taskId === task.id));
  
  if (isQueued) {
    return <Badge variant="outline">In Queue</Badge>;
  }
  
  return (
    <Button 
      variant="ghost" 
      size="sm"
      onClick={() => addTask(task, projectName)}
    >
      <Plus className="w-4 h-4 mr-1" />
      Add to Queue
    </Button>
  );
}
```

---

## 5. Migration Plan

### Step 1: Create TaskSpawner (1-2 hours)
- [ ] Create `engine/spawner.rs` with core `spawn()` method
- [ ] Add database migration for `task_idempotency_keys`
- [ ] Write unit tests for deduplication logic

### Step 2: Migrate Existing Spawners (2-3 hours)
- [ ] Move `create_content_review_apply_task()` → `TaskSpawner::spawn_follow_up()`
- [ ] Move `create_cluster_and_link_task()` → `TaskSpawner::spawn_follow_up()`
- [ ] Move GSC fix task creation → `TaskSpawner::spawn_from_gsc_collection()`
- [ ] Keep keyword→article creation in commands (user-initiated, not follow-up)
- [ ] Verify all migrations maintain existing behavior

### Step 3: Add execute_queue Command ✅
- [x] Create command accepting `Vec<QueueItem>`
- [x] Implement event emission for progress tracking
- [x] Add Tauri command registration

**Files Created:**
- `src-tauri/src/commands/executor.rs` - New `execute_queue` command with event streaming

**Files Modified:**
- `src-tauri/src/commands/mod.rs` - Added executor module
- `src-tauri/src/lib.rs` - Registered `execute_queue` command
- `src/lib/tauri.ts` - Added `executeQueue()` wrapper

**Event Types:**
| Event | Payload | Description |
|-------|---------|-------------|
| `queue:task-started` | `{ task_id, project_id, payload: { index, total, title, task_type } }` | Task begins execution |
| `queue:task-completed` | `{ task_id, project_id, payload: { message, steps, follow_up_count } }` | Task succeeded |
| `queue:task-failed` | `{ task_id, project_id, payload: { error, retryable } }` | Task failed |
| `queue:follow-up-created` | `{ task_id, project_id, title, task_type, execution_mode }` | Follow-up task ready for auto-queuing |
| `queue:finished` | `()` | All tasks completed |

### Step 4: Frontend Queue Store (2-3 hours)
- [ ] Create Zustand store with queue management
- [ ] Add event listeners for Rust progress events
- [ ] Modify TaskRunner to use global queue
- [ ] Add "Add to Queue" buttons to TaskBoard and TaskDetail

### Step 5: Integration & Testing (2 hours)
- [ ] Test cross-project queue execution
- [ ] Test follow-up auto-queuing
- [ ] Test deduplication (retry failed parent, verify no duplicate follow-ups)
- [ ] Test pause/resume functionality

---

## 6. Open Questions

1. **Cross-project dependencies:** Should tasks depend on tasks in other projects? (Current: No)
2. **Queue persistence:** Save queue to `localStorage` for session recovery? (Phase 2)
3. **Parallel execution:** Should independent tasks run in parallel? (Phase 2 - current: serial)
4. **Priority rebalancing:** Should high-priority tasks in Project B preempt low-priority in Project A?

---

## 7. Appendix: Idempotency Key Format

For consistent deduplication:

| Scenario | Key Format | Example |
|----------|-----------|---------|
| Follow-up from parent | `followup:{parent_id}:{task_type}:{title_hash}` | `followup:task-123:cluster_and_link:abc12` |
| GSC fix task | `gsc_fix:{project_id}:{url}:{issue_type}` | `gsc_fix:proj-456:/blog/post:not_indexed` |
| Scheduler created | `scheduler:{rule_id}:{date_ymd}` | `scheduler:rule-789:20260325` |
| Keyword→Article | `keyword_article:{project_id}:{keyword_hash}` | `keyword_article:proj-456:budget_planner` |

---

**Next Step:** Review this spec, then implement Step 1 (TaskSpawner module).
