# Task Queue System

The task queue is the **single execution path** for all task processing. All task execution goes through the queue — never call `executeTask` directly from components.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         TASK QUEUE SYSTEM                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│   FRONTEND (Zustand + Tauri Events)                                     │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │  QueueStore                                                  │     │
│   │  ├─ items: QueueItem[]           // Ordered queue            │     │
│   │  ├─ currentIndex: number         // Running item             │     │
│   │  ├─ isRunning: boolean                                      │     │
│   │  └─ isPaused: boolean                                       │     │
│   └──────────────────────────────────────────────────────────────┘     │
│        │                                                                │
│        │ invoke                                                         │
│        ▼                                                                │
│   RUST BACKEND                                                          │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │  execute_queue(items: Vec<QueueItem>)                       │     │
│   │     spawn_blocking                                             │     │
│   │        for item in items:                                      │     │
│   │           emit "task-started"                                  │     │
│   │           execute_task(item.task_id)                           │     │
│   │           emit "task-completed"                                │     │
│   │        emit "queue:finished"                                   │     │
│   └──────────────────────────────────────────────────────────────┘     │
│        │                                                                │
│        │ events                                                         │
│        ▼                                                                │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │  TaskRunner (UI)                                             │     │
│   │  ├─ Shows queue items with project badges                    │     │
│   │  ├─ Live step progress                                       │     │
│   │  ├─ Pause / Resume / Clear controls                          │     │
│   │  └─ Follow-up task actions (Run now / Skip / Open)          │     │
│   └──────────────────────────────────────────────────────────────┘     │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Queue Semantics

### Global Queue

- **One queue per app session**, not per project
- Tasks from any project can be added
- Queue continues running when switching projects/views
- Queue state is **managed by the backend** (`enqueue_tasks`, `get_queue_snapshot`, `pause_queue`, `resume_queue` in `tauri.ts:468`). The frontend subscribes to events and renders progress, but does not own execution.

### Execution Order

```
FIFO by default
     ↓
"Run now" inserts at head
     ↓
Follow-up tasks append after parent's position
```

### Concurrency

**V1:** Serial execution only (one task at a time). No parallel execution.

**Future:** May add limited parallelism for explicitly safe task types.

---

## Data Model

### QueueItem (Frontend)

```typescript
interface QueueItem {
  taskId: string;
  projectId: string;
  projectName: string;  // For display
  title: string;
  taskType: string;
  status: 'pending' | 'running' | 'completed' | 'failed';
  result?: ExecutionResult;
  liveSteps?: StepProgress[];
}
```

### QueueItem (Rust)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub task_id: String,
    pub project_id: String,
    pub title: String,
    pub task_type: String,
}
```

### ExecutionResult

```rust
pub struct ExecutionResult {
    pub success: bool,
    pub message: String,
    pub steps: Vec<StepResult>,
    pub follow_up_tasks: Vec<FollowUpTask>,
}
```

---

## Events

| Event | Direction | Payload | Purpose |
|-------|-----------|---------|---------|
| `queue:task-started` | Rust → TS | `{ task_id, project_id, index, total, title, task_type }` | Task begins |
| `queue:task-step-progress` | Rust → TS | `{ task_id, step_name, status }` | Step update |
| `queue:task-completed` | Rust → TS | `{ task_id, project_id, success, message, follow_up_count }` | Task done |
| `queue:task-failed` | Rust → TS | `{ task_id, project_id, error, retryable }` | Task failed |
| `queue:follow-up-created` | Rust → TS | `{ task_id, project_id, title, task_type, execution_mode }` | Follow-up ready |
| `queue:finished` | Rust → TS | `()` | All done |

---

## Frontend API

### From Components

```typescript
import { useQueue } from '@/stores/queueStore';

function MyComponent() {
  const { addTask, startQueue, isRunning } = useQueue();
  
  const handleRun = (task: Task) => {
    addTask(task, projectName);
    if (!isRunning) startQueue();
  };
}
```

### QueueStore Actions

| Action | Purpose |
|--------|---------|
| `addTask(task, projectName)` | Add single task to queue |
| `addTasks(tasks, projectName)` | Add multiple tasks |
| `removeTask(taskId)` | Remove pending task |
| `reorderTasks(newOrder)` | Manual reorder |
| `clearCompleted()` | Remove done/failed items |
| `startQueue()` | Begin execution |
| `pauseQueue()` | Pause after current task |
| `resumeQueue()` | Resume execution |
| `stopQueue()` | Stop immediately |

---

## Adding Tasks to Queue

### From Task List

Bulk selection → "Add to Queue" button.

### From Task Detail

"Add to Queue" button beside Run button.

### From Quick Actions

Overview cards can enqueue tasks.

### From Follow-Up Tasks

When a task completes with follow-ups:
```
┌─────────────────────────────────────────┐
│ Task completed: content_review          │
│                                         │
│ Follow-up tasks created:                │
│ ┌─────────────────────────────────────┐ │
│ │ content_review_apply        [Run]   │ │
│ └─────────────────────────────────────┘ │
└─────────────────────────────────────────┘
```

---

## Follow-Up Task Handling

When `ExecutionResult.follow_up_tasks` is returned:

1. **Auto-queue** if `execution_mode` is `"automatic"` or `"batchable"`
2. **Show inline** in TaskRunner with actions:
   - **Run now** — inserts at head of queue
   - **Skip** — removes from follow-up list
   - **Open task** — navigates to task detail

**Idempotency:** Follow-up creation uses `TaskSpawner::spawn_follow_up()` which prevents duplicates via deterministic keys.

---

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Task not found | Mark failed, continue to next |
| Task not runnable | Show "Open task" action, continue |
| DB/IPC failure | Mark failed, continue |
| App crash | Queue not restored (V1 limitation); task statuses preserved in DB |

---

## Task Spawner

All programmatic task creation goes through `TaskSpawner`:

```rust
// src-tauri/src/engine/spawner.rs

pub struct TaskSpawner;

impl TaskSpawner {
    /// Primary creation method
    pub fn spawn(conn: &Connection, spec: TaskSpec) -> Result<Task>;
    
    /// Idempotent follow-up creation
    pub fn spawn_follow_up(
        conn: &Connection,
        parent: &Task,
        task_type: &str,
        title: &str,
    ) -> Result<Option<Task>>;  // None if duplicate exists
}
```

**Never call `task_store::create_task` directly** — the spawner enforces idempotency and dependency validation.

### Idempotency Key Format

| Scenario | Key Format |
|----------|------------|
| Follow-up from parent | `followup:{parent_id}:{task_type}:{title_hash}` |
| GSC fix task | `gsc_fix:{project_id}:{url}:{issue_type}` |
| Scheduler created | `scheduler:{rule_id}:{date_ymd}` |
| Keyword→Article | `keyword_article:{project_id}:{keyword_hash}` |

---

## Project Switching

- Switching projects does **not** stop queue execution
- Runner rows always show **project badge**
- Opening a task from runner navigates to correct project + task

---

## Debugging

### Check Queue Logs

```bash
./scripts/check_queue_logs.sh
```

Shows:
- Recent queue-related logs
- All ERROR logs
- Log stats by component
- Full trace for specific tasks

### Expected Flow

```
[execute_queue] Called with N items
[execute_queue_internal] Starting execution of N tasks
[execute_queue_internal] Task 1/N: Title (task-xxx) in project xxx
[execute_queue_internal] Emitting queue:task-started for task task-xxx
[execute_queue_internal] Successfully emitted started event
... (execution) ...
[execute_queue_internal] Task task-xxx succeeded: Task completed
[execute_queue_internal] Emitting queue:task-completed for task task-xxx
[execute_queue_internal] All tasks complete
```

### Common Failures

| Symptom | Likely Cause |
|---------|--------------|
| No backend logs | Frontend `enqueue` flow broken |
| Backend starts, no events | `tokio::spawn` or `app_handle.emit` error |
| Events emitted, not received | Event listener not registered |
| Task fails silently | `spawn_blocking` result handling error |

---

## See Also

- [Workflow Engine](./WORKFLOW_ENGINE.md) — How tasks are planned and executed
- [Business Processes](./BUSINESS_PROCESSES.md) — What tasks accomplish
- Check the task detail panel → Run History for step-by-step failure diagnostics
