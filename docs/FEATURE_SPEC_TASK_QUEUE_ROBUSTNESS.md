# Feature Specification: Task Queue Robustness

Generated: 2026-05-22
Status: Draft — Ready for Implementation

---

## Executive Summary

The task queue system works end-to-end but has accumulated fragility in three areas:

1. **Queue stalls on failure.** The runner can stop processing when a task fails or panics, leaving the user with a stuck bottom panel and no clear way to resume.
2. **Sheets open unexpectedly.** A chain of `useEffect` auto-open logic in `TaskRunner → App → TaskBoard → TaskDetail` causes the review drawer to reopen after the user closes it, or to open when no user action requested it.
3. **State drift between queue and tasks.** The queue item status and task status are updated in separate DB operations. On crashes, tasks can be orphaned in `in_progress` with no running queue item.

This spec outlines minimal, targeted fixes that reuse the existing architecture. No new abstractions. No new state layers. Just harden what we have.

---

## P0 — Queue Resilience (Backend)

### 1. Harden `pause_on_error` Semantics

**Problem:** The queue run has a `pause_on_error` boolean that is currently hard-coded to `false` in `create_run`, yet the runner loop checks it in three places. Recent changes may have accidentally toggled it, or a panic in the nested runtime may be stopping the runner regardless of the flag.

**Fix:** Make `pause_on_error` an explicit, traceable default.

**File:** `src-tauri/src/engine/queue.rs`

```rust
// In create_run — keep it false, but make it a named constant
const DEFAULT_PAUSE_ON_ERROR: bool = false;

let run = QueueRun {
    // ...
    pause_on_error: DEFAULT_PAUSE_ON_ERROR,
    // ...
};
```

**Additional fix:** In the three failure arms of the runner loop, log the exact reason before deciding to pause:

```rust
if run.pause_on_error {
    log::warn!("Queue paused because pause_on_error=true. Task {} failed with: {}", task_id, error);
    pause_run(&mut conn, run.id)?;
    emit_event(app_handle, "queue:finished", ());
    break;
} else {
    log::warn!("Task {} failed but queue continues (pause_on_error=false). Error: {}", task_id, error);
    // continue loop — do NOT break
}
```

**Behavior:** Failures are logged. The queue only stops if explicitly configured to. Users see failed items in the runner panel but the run continues.

---

### 2. Ensure Runner Loop Never Breaks on Non-Panicking Errors

**Problem:** The runner loop currently has a `break` in some error paths that should `continue`. If `execute_task_with_token` returns `Err` (e.g. DB lock), the loop may exit, leaving pending items unprocessed.

**Fix:** Audit all `break` statements in `run_queue`. Only break when:
- The run is explicitly paused by user action
- The run is explicitly dismissed
- There are no more pending items (normal finish)

**File:** `src-tauri/src/engine/queue.rs`

Search the `run_queue` function for all `break` statements and annotate each with a comment explaining why it is a terminal condition. Convert any `break` on task failure to `continue` (unless `pause_on_error` is true).

---

### 3. Recover Orphaned `in_progress` Tasks on Startup

**Problem:** `recover_queue_on_startup` resets `running` queue items to `pending`, but it does NOT reset tasks with status `in_progress`. After a crash, a task can be stuck `in_progress` forever — it will never be enqueued again because the queue thinks it's already running.

**Fix:** Reset orphaned `in_progress` tasks to `todo` during startup recovery.

**File:** `src-tauri/src/engine/queue.rs`

```rust
// Inside recover_queue_on_startup, after resetting queue items:
conn.execute(
    "UPDATE tasks SET status = 'todo', updated_at = ?1 WHERE status = 'in_progress'",
    params![now],
)?;
```

**Behavior:** On app restart, any task that was `in_progress` with no corresponding `running` queue item becomes `todo` and is eligible for enqueue again.

---

### 4. Atomic Queue Item + Task Status Update

**Problem:** The runner updates `queue_items.status` and `tasks.status` in separate DB operations. A crash between them causes drift.

**Fix:** Wrap the paired updates in a single SQLite transaction.

**File:** `src-tauri/src/engine/queue.rs`

```rust
// Before executing a task:
let tx = conn.transaction()?;
tx.execute("UPDATE queue_items SET status = 'running' WHERE id = ?1", params![item_id])?;
tx.execute("UPDATE tasks SET status = 'in_progress', last_error = NULL WHERE id = ?1", params![task_id])?;
tx.commit()?;

// After execution (success or failure):
let tx = conn.transaction()?;
tx.execute("UPDATE queue_items SET status = ?1 WHERE id = ?2", params![item_status, item_id])?;
tx.execute("UPDATE tasks SET status = ?1 WHERE id = ?2", params![task_status, task_id])?;
tx.commit()?;
```

**Behavior:** Queue item and task status are always consistent. A crash leaves both in the pre-execution state, which startup recovery handles.

---

## P0 — Stop Unexpected Sheet Opens (Frontend)

### 5. Deduplicate Auto-Open via `openedTaskIds` in `queueStore`

**Problem:** `TaskRunner` uses a `useRef<Set<string>>` (`autoOpenedRef`) to track which tasks it has already auto-opened. This resets when `TaskRunner` remounts (e.g. user dismisses and reopens the queue panel), causing the same review task to auto-open again.

**Fix:** Move the deduplication set into `queueStore` so it survives component lifecycles.

**File:** `src/stores/queueStore.ts`

```ts
interface QueueState {
  snapshot: QueueSnapshot | null;
  isVisible: boolean;
  isStarting: boolean;
  expandedTaskIds: Set<string>;
  autoOpenedTaskIds: Set<string>;  // NEW — survives remounts
  // ... actions
}

// In the store:
markAutoOpened: (taskId: string) => set(state => ({
  autoOpenedTaskIds: new Set([...state.autoOpenedTaskIds, taskId])
})),
hasAutoOpened: (taskId: string) => get().autoOpenedTaskIds.has(taskId),
```

**File:** `src/components/tasks/TaskRunner.tsx`

```tsx
// Replace autoOpenedRef usage:
const autoOpenedRef = useRef<Set<string>>(new Set())
// With:
const markAutoOpened = useQueueStore(s => s.markAutoOpened)
const hasAutoOpened = useQueueStore(s => s.hasAutoOpened)

// In the effect:
if (isReviewTask && onOpenTask && !hasAutoOpened(item.task.id)) {
  markAutoOpened(item.task.id)
  onOpenTask(item.task.id)
}
```

**Behavior:** Once a task auto-opens, it never auto-opens again in the same session — even if the queue panel is dismissed and reopened.

---

### 6. Clear `pendingTaskId` When TaskBoard Opens the Sheet

**Problem:** `App.tsx` holds `pendingTaskId` state. `TaskBoard` uses it to open the sheet via `initialTaskId`. But `pendingTaskId` is never cleared after the sheet opens. Every subsequent `tasks` array update (e.g. from `runCompletedTick` refetch) re-triggers the open effect.

**Fix:** Add an `onTaskOpened` callback that clears `pendingTaskId` in `App.tsx`.

**File:** `src/App.tsx`

```tsx
const handleOpenTask = useCallback((taskId: string) => {
  setActiveView('tasks')
  setPendingTaskId(taskId)
}, [])

const handleTaskOpened = useCallback(() => {
  setPendingTaskId(null)  // ← clear once consumed
}, [])
```

**File:** `src/components/tasks/TaskBoard.tsx`

```tsx
// Ensure onTaskOpened is called in BOTH branches of the effect:
useEffect(() => {
  if (!initialTaskId) return
  const target = tasks.find(t => t.id === initialTaskId)
  if (target) {
    setSelectedTask(target)
    onTaskOpened?.()        // ← call here
    return
  }
  // ... fetch fallback ...
  getTask(initialTaskId).then(task => {
    setSelectedTask(task)
    onTaskOpened?.()        // ← and here
  })
}, [initialTaskId, tasks, statusFilter, onTaskOpened])
```

**Behavior:** `initialTaskId` is consumed exactly once. The sheet opens when requested, then stays closed unless the user explicitly opens another task.

---

### 7. Guard `initialTaskId` Effect with `useRef`

**Problem:** The `TaskBoard` effect that watches `initialTaskId` also depends on `tasks`. Every `tasks` array update causes the effect to re-evaluate. Even after `onTaskOpened` clears `pendingTaskId`, React may batch updates and the effect can see the old `initialTaskId` value on the next render.

**Fix:** Add a `processedInitialTaskIdRef` to ensure the effect body only runs once per `initialTaskId` value.

**File:** `src/components/tasks/TaskBoard.tsx`

```tsx
const processedInitialTaskIdRef = useRef<string | null>(null)

useEffect(() => {
  if (!initialTaskId) return
  if (processedInitialTaskIdRef.current === initialTaskId) return
  processedInitialTaskIdRef.current = initialTaskId
  // ... existing logic ...
}, [initialTaskId, tasks, statusFilter, onTaskOpened])
```

**Behavior:** The auto-open logic fires at most once per `initialTaskId` value, regardless of how many times `tasks` updates.

---

## P1 — Frontend Queue Stability

### 8. Coalesce Rapid `sync()` Calls

**Problem:** The backend emits `queue:task-started`, `queue:task-completed`, `queue:task-failed`, `queue:follow-up-created`, and `queue:finished` in rapid succession. Each triggers `sync()`, which calls `getQueueSnapshot()`. During a batch run, this can mean 5+ full snapshot fetches within a few hundred milliseconds.

**Fix:** Debounce or coalesce `sync()` calls in `queueStore`.

**File:** `src/stores/queueStore.ts`

```ts
let syncTimeout: ReturnType<typeof setTimeout> | null = null

sync: () => {
  if (syncTimeout) clearTimeout(syncTimeout)
  syncTimeout = setTimeout(async () => {
    syncTimeout = null
    const snapshot = await tauri.getQueueSnapshot()
    set({ snapshot, isStarting: false, isVisible: snapshotToVisible(snapshot) })
  }, 150)  // 150ms coalescing window
},
```

**Behavior:** Rapid event bursts result in a single snapshot fetch. The UI still updates within 150ms, but backend load drops significantly.

---

### 9. Prevent `runCompletedTick` from Reopening Closed Sheets

**Problem:** `useQueueRunner` calls `onCompleted` when the queue finishes. `App.tsx` increments `runCompletedTick`, which triggers a `TaskBoard` refetch. If the user had a task detail sheet open and closed it, the refetch does not directly reopen it — but if any other state is still holding an `initialTaskId`, it can cascade.

**Fix:** Ensure `runCompletedTick` only triggers data refresh, not UI navigation.

**File:** `src/components/tasks/TaskBoard.tsx`

```tsx
useEffect(() => {
  if (!projectId || runCompletedTick === 0) return
  refetch()  // safe — only refreshes data, does not touch selectedTask
}, [projectId, runCompletedTick, refetch])
```

This is already the current code, but verify that `refetch()` does NOT mutate `selectedTask` or `initialTaskId`. If it does, decouple them.

---

## P1 — Backend Reliability

### 10. Log Panics in `spawn_blocking` Threads

**Problem:** When a task panics inside the `spawn_blocking` wrapper, the runner catches the `JoinError` but logs at `warn!` level with limited context. Panics often indicate real bugs (e.g. unwrap on None) that are silently swallowed.

**Fix:** Elevate panic logging to `error!` and include the task ID and type.

**File:** `src-tauri/src/engine/queue.rs`

```rust
Err(e) => {
    log::error!("TASK PANIC: task_id={} type={} error={:?}", task_id, task_type, e);
    // ... existing failure handling ...
}
```

**Behavior:** Panics are visible in logs immediately, making them actionable.

---

## P2 — Cleanup

### 11. Remove Unused `task_step_progress` Event or Wire It Up

**Problem:** `executor.rs` emits `task_step_progress` events after every step, but nothing in the frontend listens to them. The `RunnerItem.liveSteps` field exists in types but is always empty. This is dead code that creates confusion.

**Decision:** Either:
- **Option A (minimal):** Remove the emit from `executor.rs` and the `liveSteps` field from types.
- **Option B (useful):** Wire it up in `queueStore` to populate a `liveSteps` map, and display step progress in `TaskRunner`.

**Recommendation:** Option A for now. Step-level progress is not a user-reported pain point. We can add it later if needed.

**Files:**
- `src-tauri/src/engine/executor.rs` — remove `emit("task_step_progress", ...)` block
- `src-tauri/src/models/queue.rs` — remove `live_steps` from `QueueItem` if present
- `src/lib/types.ts` — remove `liveSteps` from `RunnerItem` if present

---

## Issue Matrix

| # | Issue | Priority | Type | Status |
|---|-------|----------|------|--------|
| 1 | Harden `pause_on_error` semantics | P0 | Backend | Not started |
| 2 | Ensure runner loop never breaks on non-panic errors | P0 | Backend | Not started |
| 3 | Recover orphaned `in_progress` tasks on startup | P0 | Backend | Not started |
| 4 | Atomic queue item + task status update | P0 | Backend | Not started |
| 5 | Deduplicate auto-open via `queueStore` | P0 | Frontend | Not started |
| 6 | Clear `pendingTaskId` when sheet opens | P0 | Frontend | Not started |
| 7 | Guard `initialTaskId` with `useRef` | P0 | Frontend | Not started |
| 8 | Coalesce rapid `sync()` calls | P1 | Frontend | Not started |
| 9 | Prevent `runCompletedTick` from reopening sheets | P1 | Frontend | Not started |
| 10 | Log panics in `spawn_blocking` threads | P1 | Backend | Not started |
| 11 | Remove unused `task_step_progress` event | P2 | Cleanup | Not started |

---

## Implementation Order

**Phase 1 (Immediate — Queue Stops):**
1. Items 1 + 2: Audit `pause_on_error` and runner loop `break` statements. Add logging.
2. Item 3: Reset orphaned `in_progress` tasks on startup.
3. Item 4: Wrap paired status updates in transactions.

**Phase 2 (This Week — Drawers):**
4. Item 5: Move auto-open dedup to `queueStore`.
5. Item 6: Clear `pendingTaskId` on open.
6. Item 7: Add `processedInitialTaskIdRef` guard.

**Phase 3 (Next Week — Polish):**
7. Item 8: Coalesce `sync()` calls.
8. Item 9: Verify `runCompletedTick` does not navigate.
9. Item 10: Elevate panic logs.

**Phase 4 (Backlog):**
10. Item 11: Remove dead `task_step_progress` code.

---

## Testing Checklist

- [ ] Run a task that fails. Queue continues to next item.
- [ ] Run a task that panics (simulate with a debug panic). Queue logs error and continues.
- [ ] Restart app while a task is `in_progress`. Task resets to `todo`.
- [ ] Run a review-surface task (e.g. `research_keywords`). Sheet auto-opens once.
- [ ] Close the sheet. It does not reopen on its own.
- [ ] Dismiss queue panel. Reopen it. Sheet does not reopen.
- [ ] Run 5 tasks rapidly. Frontend snapshot fetches are coalesced (check network / Tauri logs).
