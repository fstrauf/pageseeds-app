# Queue System Testing Guide

This document describes how to test the task queue and logging system end-to-end.

## Test Architecture

We have three levels of tests:

### 1. Unit Tests (Fast, < 1 second)
- No external dependencies
- Pure logic testing
- Run in CI

### 2. Integration Tests (Fast, ~2 seconds)
- Uses SQLite in-memory database
- Tests database operations
- Tests log storage and querying
- No external APIs
- Run in CI

### 3. E2E Tests (Slow, ~60 seconds)
- Uses real Kimi agent
- Uses real Reddit API
- Full task execution flow
- Run manually

## Running Tests

### Using the Test Script

```bash
# Run all tests
./scripts/test-queue-system.sh

# Run specific levels
./scripts/test-queue-system.sh unit      # Unit tests only
./scripts/test-queue-system.sh int       # Integration tests
./scripts/test-queue-system.sh e2e       # E2E tests (requires APIs)
./scripts/test-queue-system.sh log       # Log system tests
```

### Running Individual Tests

```bash
cd src-tauri

# Unit tests
cargo test --test reddit_e2e_test test_json_extraction_from_kimi_output -- --nocapture

# Integration tests
cargo test --test queue_integration_test test_queue_enqueue_and_state_management -- --nocapture
cargo test --test queue_integration_test test_batch_log_submission -- --nocapture
cargo test --test queue_integration_test test_log_querying -- --nocapture

# Log persistence test
cargo test --test queue_e2e_test test_log_persistence -- --nocapture

# Full E2E (requires real Kimi)
cargo test --test queue_e2e_test test_full_queue_flow_with_real_execution -- --ignored --nocapture
```

## What Each Test Does

### `test_json_extraction_from_kimi_output`
Tests JSON parsing from various Kimi output formats:
- Clean JSON
- JSON with markdown wrapper
- JSON with surrounding text
- Realistic Kimi output

### `test_queue_enqueue_and_state_management`
Simulates the exact frontend flow:
1. Creates project and tasks
2. Logs enqueue action
3. Updates task status (Todo → InProgress → Done)
4. Logs each state change
5. Verifies final state

### `test_batch_log_submission`
Tests frontend log batching:
1. Creates 10 log entries
2. Stores them all
3. Verifies session ID grouping
4. Confirms all logs retrievable

### `test_log_querying`
Tests log query filters:
- Level filtering (Debug, Info, Warn, Error)
- Component filtering
- Text search
- Combined filters

### `test_log_persistence`
Tests the logging system end-to-end:
1. Stores logs of all levels
2. Queries by level
3. Queries by source (Frontend, Backend, Agent)
4. Searches by text
5. Gets statistics
6. Verifies counts

### `test_full_queue_flow_with_real_execution`
**The full E2E test:**
1. Creates test project with `reddit_config.md`
2. Creates `reddit_opportunity_search` task
3. Logs enqueue action
4. Executes task through real executor
5. Calls real Kimi agent to parse config
6. Searches real Reddit API
7. Stores results
8. Verifies:
   - Task completed
   - Logs stored
   - Statistics correct
   - State transitions correct

## Test Output

### Successful Run

```
========================================
E2E Queue Flow Test
========================================

[Step 1] Setting up test project...
✅ Project created at: /tmp/queue_e2e_test_...

[Step 2] Initializing database...
✅ Database initialized, project ID: test-proj-e2e

[Step 3] Creating task...
✅ Task created: test-task-...

[Step 4] Storing initial log...
✅ Log stored

[Step 5] Executing task through executor...
   This will call the real Kimi agent and Reddit API...
   (This may take 30-60 seconds)

⏱️  Execution completed in 45.2s

[Step 6] Verifying execution result...
✅ Task executed successfully
   Success: true
   Message: Task completed
   Steps executed: 4

[Step 7] Verifying task state...
   Final status: Review
   Attempts: 1

[Step 8] Verifying logs...
   Total logs stored: 25
   Backend logs: 20
   Frontend logs: 5

[Step 9] Log statistics...
   Total logs: 25
   Errors: 0
   Warnings: 1
   Info: 20
   Debug: 4

========================================
✅ E2E QUEUE FLOW TEST PASSED
========================================
```

## Debugging Failed Tests

### Test Hangs
If a test hangs during execution:
1. Check if Kimi CLI is responding: `kimi --version`
2. Check if Reddit API is rate-limiting
3. Check logs: `tail -f ~/Library/Logs/com.pageseeds.app/PageSeeds.log`

### JSON Parse Errors
If you see "Agent produced invalid JSON":
1. Run the test with `--nocapture` to see raw output
2. Check the prompt in `engine/exec/reddit.rs`
3. Test Kimi directly with the prompt

### Database Errors
If you see SQLite errors:
1. Check if the database is locked
2. Verify migrations ran: `db::init_with_conn`
3. Check table exists: `.schema app_logs`

## CI/CD Integration

For CI, run only fast tests:

```yaml
test:
  script:
    - cd src-tauri
    - cargo test --test reddit_e2e_test test_json_extraction_from_kimi_output
    - cargo test --test queue_integration_test
    - cargo test --test queue_e2e_test test_log_persistence
```

## Manual Testing in App

To test the actual app:

1. **Build and run:**
   ```bash
   pnpm tauri dev
   ```

2. **Open browser console** (Cmd+Option+I)

3. **Navigate to Settings → Application Logs**

4. **Create and run a task:**
   - Go to Tasks
   - Create a "Reddit Search" task
   - Click "Run"

5. **Watch logs appear in real-time**

6. **Export logs** and verify JSON structure

## Logs Location

After running tests, logs are in:
- **Test DB**: In-memory (destroyed after test)
- **App DB**: `~/Library/Application Support/com.pageseeds.app/pageseeds.db`

Query logs directly:
```bash
sqlite3 ~/Library/Application\ Support/com.pageseeds.app/pageseeds.db \
  "SELECT * FROM app_logs ORDER BY timestamp DESC LIMIT 10;"
```

## Adding New Tests

When adding features:

1. **Add unit test** in `src/engine/*` modules
2. **Add integration test** in `tests/queue_integration_test.rs`
3. **Add E2E test** in `tests/queue_e2e_test.rs` if it uses real APIs
4. **Update this documentation**

Example:
```rust
#[test]
fn test_my_feature() {
    let conn = create_test_db();
    // Test your feature
    assert!(result.is_ok());
}
```
