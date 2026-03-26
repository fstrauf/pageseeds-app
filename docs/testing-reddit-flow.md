# Reddit Flow Testing Guide

This document describes how to test the Reddit opportunity search flow end-to-end.

## Overview

The Reddit flow consists of several steps:

1. **Config Parsing** (Agentic) - Kimi parses `reddit_config.md` to extract search parameters
2. **Reddit Search** (Deterministic) - Query Reddit API using parsed parameters
3. **Enrichment** (Agentic) - AI scores and generates reply drafts for found posts
4. **Results Storage** (Deterministic) - Save opportunities to SQLite

## Test Architecture

### Unit Tests (Fast, No External Dependencies)

Located in: `src-tauri/tests/reddit_e2e_test.rs`

- **JSON Extraction Test** - Verifies our JSON parsing handles various Kimi output formats

### Integration Tests (Requires Real APIs)

Located in: `src-tauri/tests/reddit_e2e_test.rs`

These tests are marked with `#[ignore]` and must be run manually:

- **Config Parsing Test** - Tests Kimi integration with real agent calls
- **Full Flow Test** - Tests the complete pipeline including Reddit API

## Running Tests

### Quick Test (No APIs Required)

```bash
cd src-tauri
cargo test --test reddit_e2e_test test_json_extraction_from_kimi_output -- --nocapture
```

### Using the Test Runner Script

```bash
# Run all tests
./scripts/test-reddit-flow.sh

# Run specific tests
./scripts/test-reddit-flow.sh json      # Unit test only
./scripts/test-reddit-flow.sh config    # Config parsing with Kimi
./scripts/test-reddit-flow.sh full      # Full end-to-end
```

### Manual Test Execution

```bash
cd src-tauri

# JSON extraction (fast)
cargo test --test reddit_e2e_test test_json_extraction_from_kimi_output -- --nocapture

# Config parsing with real Kimi (~30s)
cargo test --test reddit_e2e_test test_reddit_config_parsing_with_real_kimi -- --ignored --nocapture

# Full flow with real APIs (~60s)
cargo test --test reddit_e2e_test test_full_reddit_flow_with_real_apis -- --ignored --nocapture
```

## Prerequisites

1. **Kimi CLI Installed**
   ```bash
   pip install kimi-cli
   ```

2. **Kimi Authenticated**
   ```bash
   kimi auth
   ```

3. **Internet Connection** (for Reddit API tests)

## Sample Test Output

### Config Parsing Test

```
========================================
TEST 1: Reddit Config Parsing with Real Kimi
========================================

✅ Test project created at: /tmp/reddit_e2e_test_12345
✅ Project created in DB: test-proj-123
✅ Task created: test-reddit-1678901234567

🚀 Executing task (this will call real Kimi agent)...

⏱️  Execution completed in 25.3s

📊 Execution Result:
   Success: true
   Message: Task completed
   Steps executed: 4

   Step 1: reddit_config_parse
      Kind: reddit_config_parse
      Status: ok
      Message: Parsed config: 4 keywords, 5 topics, 5 subreddits
      
   Step 2: reddit_search
      ...
```

## Troubleshooting

### "Agent binary 'kimi' not found"

Install Kimi CLI:
```bash
pip install kimi-cli
```

### "Kimi authentication failed"

Run authentication:
```bash
kimi auth
```

### "No Reddit posts found"

This is not necessarily a failure. Reddit may:
- Rate limit requests
- Have no recent posts matching your keywords
- Block the user agent

Check the step output for details on why no posts were found.

### "JSON parse error"

If Kimi returns malformed JSON:
1. Check the raw output in the test logs
2. Verify the prompt in `engine/exec/reddit.rs`
3. Run the config parsing example to debug:
   ```bash
   cargo run --example test_reddit_config_parse -- /path/to/project
   ```

## Debugging Tips

### View Detailed Logs

```bash
RUST_LOG=info cargo test --test reddit_e2e_test ...
```

### Run Individual Examples

```bash
# Test config parsing only
cargo run --example test_reddit_config_parse -- /Users/fstrauf/01_code/your-project

# Test full flow manually
cargo run --example test_reddit_full_flow -- /Users/fstrauf/01_code/your-project
```

### Check Kimi Output Directly

```bash
cd /Users/fstrauf/01_code/your-project
kimi -p "Extract JSON from this config..." --print --no-thinking --final-message-only
```

## CI/CD Considerations

The integration tests requiring real APIs are marked with `#[ignore]` and won't run in CI.

For CI, only run the unit tests:
```bash
cargo test --test reddit_e2e_test  # Ignores #[ignored] tests by default
```

Or explicitly:
```bash
cargo test --test reddit_e2e_test test_json_extraction_from_kimi_output
```

## Related Files

- Test file: `src-tauri/tests/reddit_e2e_test.rs`
- Reddit execution: `src-tauri/src/engine/exec/reddit.rs`
- Agent invocation: `src-tauri/src/engine/agent.rs`
- Executor: `src-tauri/src/engine/executor.rs`
- Examples: `src-tauri/examples/test_reddit_*.rs`
