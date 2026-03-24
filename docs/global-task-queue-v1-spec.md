# Global Task Queue V1 (Simple, Cross-Project)

## Summary

Introduce a single app-wide task queue that can accept tasks from any project and execute them one-by-one.

This keeps the system predictable while allowing users to:
- Queue tasks in Project A
- Switch to Project B
- Queue more tasks
- Let one runner process all queued work continuously
- Have follow-up tasks auto-queue so workflows keep moving

## Goals

1. Single global queue across all projects in the app session.
2. One execution surface (existing bottom runner) for all queued items.
3. Keep complexity low: serial execution only (concurrency = 1).
4. Follow-up tasks auto-queue from the runner so multi-step workflows run without intervention.
5. Prevent lockups while queue is running (already improved via dedicated DB execution connections).
6. One execution path — extend the existing Rust batch system, don't build a second runner.

## Non-Goals (V1)

1. Parallel execution.
2. Priority scheduling policies beyond manual queue order.
3. Background execution after app exit.
4. Advanced retry/backoff automation.
5. Cross-device or cloud queue sync.

## Architecture: Single Execution Path

The app already has a Rust-side batch processor (`engine/batch.rs`) that does serial execution with priority ordering and error handling. Rather than building a second orchestration loop in the frontend, we extend the Rust batch system to accept an explicit ordered list of `(task_id, project_id)` pairs.

**Frontend responsibility:** Manage the ordered queue list and send it to Rust.
**Rust responsibility:** Execute tasks sequentially, emit events per step, return results.

This keeps one execution path, one place for error handling, and avoids the Rust batch system becoming dead weight alongside a frontend queue runner.

### New Rust Command

```
execute_queue(items: Vec<QueueItem>) -> stream of QueueEvent
```

Where `QueueItem` is `{ task_id, project_id }` and `QueueEvent` covers:
- `task:started { task_id }`
- `task:step-progress { task_id, step_name, status }`
- `task:completed { task_id, result, follow_up_tasks }`
- `task:failed { task_id, error }`
- `queue:finished`

The existing `run_batch` command remains available for auto-discovery of ready tasks within a single project.

## User Experience

### A. Global Queue Behavior

1. Queue is app-global, not tied to current project view.
2. Queue remains visible and active while switching views or projects.
3. Queue items display a project badge/name to avoid confusion.

### B. Add to Queue

Users can add tasks from:
1. Tasks list (selected rows).
2. Task detail panel.
3. Overview quick actions (already creates and runs tasks; should also support enqueue semantics).
4. Follow-up actions in runner (Run now / Queue next).

### C. Runner Controls

1. Start queue.
2. Pause after current task.
3. Resume queue.
4. Remove queued item (not running).
5. Clear completed items.

### D. Follow-Up Tasks

When a task completes and creates follow-up tasks:
1. Runner shows them inline under the completed task.
2. Auto-queue follow-ups by default (this is the highest-value behavior — multi-step workflows keep running).
3. Each follow-up also supports:
   - Run now (insert at front of queue)
   - Skip (remove from queue)
   - Open task (navigate to task detail)

## Execution Model

1. Global queue worker processes exactly one task at a time.
2. Queue order is deterministic:
   - FIFO by default.
   - Run now inserts at head.
   - Follow-up tasks append after the parent's position.
3. If a task fails:
   - Mark failed.
   - Continue with next queued task (default).

## Data Model (V1)

The queue is a **frontend-only ordered array** (session-scoped, Zustand store). No separate persistence — the task's DB record remains the source of truth for status.

A queue item is minimal:

```ts
interface QueueItem {
  taskId: string
  projectId: string
  title: string
  taskType: string
}
```

Queue state is derived from position + Rust events:
- Items before `currentIndex` → done/failed (check task DB status).
- Item at `currentIndex` → running.
- Items after `currentIndex` → pending.

No separate `queue_item_id`, no `enqueued_at`/`started_at`/`finished_at` timestamps (the task record in SQLite already tracks these), no `source` field. If we need analytics on queue usage later, we add it then.

## Project Switching Rules

1. Switching projects does not stop queue execution.
2. Adding tasks from another project appends to same queue.
3. Runner rows must always show project context.
4. Opening task from runner navigates to Tasks view and selects correct project and task.

## Error Handling Rules

1. If task no longer exists when worker starts it:
   - Mark queue item failed with reason "Task not found", advance to next.
2. If task is not runnable (manual-only and no manual action path):
   - Show "Open task" action instead of run, advance to next.
3. If DB/IPC call fails:
   - Mark item failed and continue.
4. If app crashes/exits:
   - V1 queue is not restored; tasks in DB remain in their saved status.

## Implementation Plan

### Phase 1: Queue Store + Follow-Up Queuing + Event Streaming

Follow-up auto-queuing is the highest-value feature — it turns isolated task runs into continuous workflows. Ship it in Phase 1, not as a later phase.

1. Add Tauri event emission from the executor (step-level progress events via `app.emit()`). This is the key enabler for live progress display now and concurrency later.
2. Add a global Zustand queue store (ordered array + `currentIndex` + `paused` flag).
3. Add a new Rust command that accepts an explicit `Vec<QueueItem>` and executes sequentially, emitting events.
4. Move TaskRunner to consume events from the queue store.
5. Add controls: Pause, Resume, Remove queued item.
6. Auto-queue follow-up tasks returned in `ExecutionResult.follow_up_tasks`.

### Phase 2: Cross-Project Add + Entry Points

1. Add "Add to Queue" from task list selections.
2. Add "Add to Queue" from task detail panel.
3. Add "Add to Queue" from overview quick actions.
4. Show project badge in runner rows.

## Multitasking Prep (Do Now, Pays Off Later)

Three things that are useful in V1 and make future parallelism straightforward:

1. **Tauri event streaming from the executor.** Currently `execute_task` blocks and returns one `ExecutionResult`. Emitting events per step (`task:step-started`, `task:step-done`, `task:failed`) lets the frontend show live progress for the current task — and gives you the event bus needed for tracking multiple concurrent tasks later.

2. **Decouple queue data from runner UI.** The Zustand queue store should be a pure data structure (ordered list + current index). The runner component subscribes to it. This separation makes it trivial to later run N workers against the same queue.

3. **Dedicated DB connections per execution.** Already done — each `execute_task` call opens its own `rusqlite::Connection`. Confirm this stays true so there's no shared-mutex bottleneck when concurrency is added.

## Acceptance Criteria

1. User can queue tasks in Project A, switch to Project B, queue more, and all run in one global queue.
2. Queue continues running while switching tabs/views/projects.
3. Runner always indicates which project each task belongs to.
4. Follow-up tasks auto-queue and continue running without user intervention.
5. Runner shows live step-level progress for the currently executing task.
6. Filter switching in Tasks during queue execution remains responsive.
7. No parallel execution occurs in V1.

## Suggested Future Extensions (V2+)

1. Optional persisted queue table in SQLite for restart recovery.
2. Limited parallelism for explicitly safe task types (N workers consuming from the same queue store).
3. Queue profiles (throughput mode vs. safe mode).
4. Retry policies and failure grouping.
