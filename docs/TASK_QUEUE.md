# Task Queue System

The task queue is the **single execution path** for all task processing. All task execution goes through the queue — never call `executeTask` directly from components.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         TASK QUEUE SYSTEM                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│   FRONTEND (Projection Cache + Tauri Events)                            │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │  QueueStore                                                  │     │
│   │  ├─ snapshot: QueueSnapshot      // Backend state copy       │     │
│   │  ├─ isVisible / expanded rows    // UI-only preferences      │     │
│   │  └─ enqueue / pause / resume     // Command wrappers         │     │
│   └──────────────────────────────────────────────────────────────┘     │
│        │                                                                │
│        │ invoke enqueue_tasks / get_queue_snapshot / pause_queue        │
│        ▼                                                                │
│   RUST BACKEND (Source of Truth)                                        │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │  engine::queue                                                │     │
│   │  ├─ queue_runs + queue_items in SQLite                        │     │
│   │  ├─ ensure_runner_started()                                   │     │
│   │  ├─ lease next pending item                                   │     │
│   │  ├─ execute_task_with_token(task_id)                          │     │
│   │  └─ persist result + auto-enqueue eligible follow-ups         │     │
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
- Queue state is **managed by the backend** (`enqueue_tasks`, `get_queue_snapshot`, `pause_queue`, `resume_queue`). The frontend subscribes to events and renders progress, but does not own execution.
- Queue membership and item status are persisted in SQLite (`queue_runs`, `queue_items`) and can be rehydrated with `get_queue_snapshot`.

### Execution Order

```
FIFO by default
     ↓
"Run now" inserts at head
     ↓
Follow-up tasks append after parent's position
```

`EnqueueMode::Append` adds pending items to the end. `EnqueueMode::Next` inserts them immediately after the currently running item.

### Concurrency

**V1:** Serial execution only (one task at a time). No parallel execution.

**Future:** May add limited parallelism for explicitly safe task types.

---

## Data Model

### QueueSnapshot (Frontend/Rust)

```rust
pub struct QueueSnapshot {
    pub run: Option<QueueRun>,
    pub items: Vec<QueueItem>,
}
```

### EnqueueItem

```rust
pub struct EnqueueItem {
    pub task_id: String,
    pub project_id: String,
    pub title: Option<String>,
    pub task_type: Option<String>,
    pub project_name: Option<String>,
}
```

`QueueItem` rows include durable queue state (`pending`, `running`, `completed`, `failed`, `skipped`), error/result JSON, and display fields joined from the task/project tables.

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
| `queue:task-started` | Rust → TS | `{ task_id, project_id, title, task_type }` | Task begins |
| `queue:task-step-progress` | Rust → TS | `{ task_id, step_name, status }` | Step update |
| `queue:task-completed` | Rust → TS | `{ task_id, project_id, success, message, follow_up_tasks }` | Task done |
| `queue:task-failed` | Rust → TS | `{ task_id, project_id, error }` | Task failed |
| `queue:follow-up-created` | Rust → TS | `{ taskId, projectId, title, taskType, runPolicy }` | Auto-enqueued follow-up ready |
| `queue:finished` | Rust → TS | `()` | All done |

---

## Frontend API

### From Components

```typescript
import { useTaskQueueActions } from '@/lib/taskQueueActions';

function MyComponent() {
  const { enqueueTasks } = useTaskQueueActions();
  
  const handleRun = (task: Task) => {
    enqueueTasks([task], projectName);
  };
}
```

### QueueStore Actions

| Action | Purpose |
|--------|---------|
| `sync()` | Refresh projection from `get_queue_snapshot` |
| `enqueue(items, 'append')` | Add tasks to the end of the backend queue and start runner if needed |
| `enqueueNext(items)` | Insert tasks next in the pending section |
| `removeTask(taskId)` | Remove a pending backend queue item |
| `pauseQueue()` | Pause after current task |
| `resumeQueue()` | Resume execution |
| `clearCompleted()` | Remove completed/failed/skipped queue rows |
| `dismiss()` | Hide/archive a finished queue run |

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
│ │ fix_content_article         [Run]   │ │
│ └─────────────────────────────────────┘ │
└─────────────────────────────────────────┘
```

---

## Follow-Up Task Handling

When `ExecutionResult.follow_up_tasks` is returned:

1. The backend queue auto-enqueues only follow-ups whose `run_policy` is `"auto_enqueue"`.
2. The task completion payload includes all returned follow-ups so TaskRunner can show review/open/run actions.
3. User-enqueue follow-ups are not automatically inserted into the queue.

TaskRunner can show inline actions:
- **Run now** — inserts at head of queue
- **Skip** — removes from follow-up list
- **Open task** — navigates to task detail

**Idempotency:** Follow-up creation uses `TaskSpawner::spawn_follow_up()` which prevents duplicates via deterministic keys.

---

## User-Input Gates

Some workflows deliberately stop before creating downstream tasks. This is not a queue concern; it is a task lifecycle contract.

| Contract | Meaning |
|----------|---------|
| `review_surface != none` | Successful execution ends with task status `review` |
| `follow_up_policy = user_selection` | Downstream tasks are created only after the user chooses items in the review UI |
| Selection command | Validates selected IDs against the parent artifact, creates downstream tasks, and marks the parent done |

Examples:
- Keyword research stores selectable keywords, goes to review, then `create_article_tasks_from_keywords` creates article tasks after selection.
- Reddit opportunity search stores selectable opportunities, goes to review, then `create_reply_tasks_from_opportunities` creates reply tasks after selection.
- Cannibalization audit stores selectable recommendations, goes to review, then `create_cannibalization_tasks_from_selection` creates implementation tasks.

Do not spawn user-selection follow-ups in `post_actions.rs`; doing so bypasses the user gate.

---

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Task not found | Mark failed, continue to next |
| Task not runnable | Show "Open task" action, continue |
| DB/IPC failure | Mark failed, continue |
| App crash | Queue rows persist; startup recovery resets stale running work and `get_queue_snapshot` shows recoverable state |

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
| Follow-up from parent | `followup:{parent_id}:{task_type}:{title}` |
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
[enqueue_tasks] Called with N items
[queue] ensure_runner_started
[queue_runner] TASK: task-xxx (Title)
[queue:task-started] task-xxx
... (execution) ...
[queue:task-completed] task-xxx
[queue_runner] auto-enqueue follow-up with run_policy=auto_enqueue, if any
[queue:finished]
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
