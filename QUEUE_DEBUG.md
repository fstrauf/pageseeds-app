# Queue System Diagnostics

## The Problem
Task is added to queue but execution doesn't appear to start.

## How to Diagnose (Using Logging System)

### Step 1: Rebuild the App
```bash
cd src-tauri && cargo build
```

### Step 2: Press the Button
In the app, press the "Run" button on a task.

### Step 3: Check Logs

#### Option A: SQLite Logs (Recommended)
```bash
./scripts/check_queue_logs.sh
```

This queries the SQLite database and shows:
- Recent queue-related logs
- All ERROR logs
- Log stats by component
- Full trace for specific tasks

#### Option B: File Logs
```bash
./scripts/check_file_logs.sh
```

This shows the file-based logs at:
`~/Library/Logs/com.pageseeds.app/PageSeeds.log`

#### Option C: In-App LogViewer
1. Open Settings
2. Go to "Logs" tab
3. Filter by component: `backend::executor` or `frontend::queue`
4. Look for `execute_queue`, `queue_internal`, `task-started`, `task-completed`

### Step 4: Interpret Results

#### Expected Flow (When Working)
```
[execute_queue] Called with 1 items
[execute_queue_internal] Starting execution of 1 tasks
[execute_queue_internal] Task 1/1: Task Title (task-xxx) in project xxx
[execute_queue_internal] Emitting queue:task-started for task task-xxx
[execute_queue_internal] Successfully emitted started event
... (task execution) ...
[execute_queue_internal] Task task-xxx succeeded: Task completed
[execute_queue_internal] Emitting queue:task-completed for task task-xxx
[execute_queue_internal] Successfully emitted completed event
[execute_queue_internal] All tasks complete
```

#### Common Failures

**Case 1: No backend logs at all**
- `execute_queue` was never called
- Check: Frontend `enqueue` → `start` → `executeQueue()` flow

**Case 2: Backend starts but no events emitted**
- Task execution started but events not sent
- Check: `tokio::spawn` working, `app_handle.emit` errors

**Case 3: Events emitted but not received**
- Backend emits events but frontend doesn't receive
- Check: Event listeners registered, channel name matches

**Case 4: Task execution fails silently**
- Error in executor but not logged
- Check: `spawn_blocking` result handling

## Logs Added

### Backend (Rust)
- `[execute_queue] Called with N items` - Entry point
- `[execute_queue_internal] Starting execution` - Background task started
- `[execute_queue_internal] Task X/Y: ...` - Each task starting
- `[execute_queue_internal] Emitting queue:task-started` - Event emission
- `[execute_queue_internal] Successfully emitted ...` - Event confirmed
- `[execute_queue_internal] Task X succeeded/failed` - Execution result
- `[execute_queue_internal] All tasks complete` - Batch finished

### Frontend (TypeScript)
Logs in queueStore.ts already use the logging system:
- `enqueue called with N items`
- `auto-starting queue`
- `start() called`
- `calling executeQueue()`
- `Received queue:task-started`
- `Received queue:task-completed`

## Debugging Checklist

- [ ] `./scripts/check_queue_logs.sh` shows `execute_queue` was called
- [ ] Shows `execute_queue_internal` started
- [ ] Shows `Emitting queue:task-started`
- [ ] Shows `Successfully emitted started event`
- [ ] Frontend received `queue:task-started` event
- [ ] Task execution completed
- [ ] Frontend received `queue:task-completed` event

## Next Steps

After running the scripts, check which step is missing and report:
1. Which logs DO appear
2. Which logs do NOT appear
3. Any ERROR logs
