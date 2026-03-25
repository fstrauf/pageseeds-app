# Async Architecture Documentation

## Overview

This document describes the async architecture of the PageSeeds task execution system.

## Current Architecture (Phase 2 - Implemented)

### Pattern

```
Tauri Command (async)
    ↓
spawn_blocking (dedicated OS thread per task)
    ↓
SQLite Connection::open() (per-thread connection)
    ↓
tokio::runtime::Runtime::new() (local per-task runtime)
    ↓
async { executor::function(&db, ...).await }
```

### Why This Pattern?

1. **SQLite is !Send**: SQLite connections cannot be sent between threads
2. **Tauri uses multi-threaded async runtime**: Can't hold Connection across .await
3. **Solution**: Each task gets its own OS thread with a fresh connection and local runtime

### Code Example

```rust
#[tauri::command]
pub async fn execute_task(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<ExecutionResult, String> {
    let db_path = state.db_path.clone();
    
    tokio::task::spawn_blocking(move || {
        // Open dedicated connection
        let db = rusqlite::Connection::open(&db_path)?;
        db.busy_timeout(Duration::from_secs(10))?;
        
        // Create local runtime for async execution
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            executor::execute_task(&db, &task_id).await
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
```

## Performance Characteristics

### Current (Phase 2)

| Metric | Value |
|--------|-------|
| Threads per task | 1 OS thread |
| SQLite connections | 1 per task (created/destroyed) |
| Tokio runtimes | 1 per task (created/destroyed) |
| Memory overhead | ~2-4MB per task (thread + runtime) |
| Startup latency | ~1-5ms (connection + runtime creation) |

### Trade-offs

**Pros:**
- Simple and robust
- No connection sharing issues
- Isolated failures (one task can't corrupt another)
- Works reliably with SQLite's threading model

**Cons:**
- Creates new OS thread per task (heavyweight)
- Creates new SQLite connection per task (no caching)
- Creates new Tokio runtime per task (overhead)
- Memory usage scales with concurrent tasks

## Future Optimization (Phase 3)

### deadpool-sqlite Approach

```
Tauri Command (async)
    ↓
Pool::get().await (non-blocking, reuses connection)
    ↓
SQLite Connection (from pool, already open)
    ↓
.async_execute() or similar async API
    ↓
Return connection to pool
```

### Benefits

| Metric | Phase 2 (Current) | Phase 3 (Pool) |
|--------|-------------------|----------------|
| Threads per task | 1 OS thread | 0 (uses async runtime) |
| SQLite connections | Created/destroyed | Reused from pool |
| Memory overhead | ~2-4MB per task | ~50KB per connection |
| Startup latency | ~1-5ms | ~10-100µs |
| Concurrent tasks | Limited by RAM | Limited by pool size |

### Implementation Considerations

1. **Rusqlite async support**: deadpool-sqlite provides async connection pool
2. **Thread safety**: Pool manages connections across threads safely
3. **Error handling**: Need to handle pool exhaustion gracefully
4. **Migration effort**: Medium - requires changing all DB access patterns

### When to Implement Phase 3

Consider Phase 3 when:
- You need >50 concurrent tasks
- Memory usage becomes a concern
- Task startup latency matters (sub-millisecond)
- You're running on resource-constrained environments

**Current recommendation**: Phase 2 is sufficient for typical desktop use.

## File Locations

| Component | Path |
|-----------|------|
| Executor | `src-tauri/src/engine/executor.rs` |
| Batch runner | `src-tauri/src/engine/batch.rs` |
| Scheduler | `src-tauri/src/engine/scheduler.rs` |
| Command handlers | `src-tauri/src/commands/engine.rs` |
| Runtime helpers | `src-tauri/src/engine/runtime.rs` |

## Key Principles

1. **Never use `Handle::current().block_on()` in async context** - This causes panics
2. **Each task gets its own connection** - Avoids SQLite threading issues
3. **Use spawn_blocking for SQLite operations** - Keeps async runtime responsive
4. **Create local runtime for async step handlers** - Allows async/await in executor

## Testing

All 125 unit tests pass with the current architecture:

```bash
cd src-tauri && cargo test --lib
```

## References

- [Rusqlite threading model](https://github.com/rusqlite/rusqlite/blob/master/README.md)
- [Tokio spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)
- [deadpool-sqlite](https://docs.rs/deadpool-sqlite/latest/deadpool_sqlite/)
