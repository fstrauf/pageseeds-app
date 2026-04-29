#!/usr/bin/env bash
# Fail if task_store::create_task is called outside the allowed allowlist.
# The spawner.rs module is the canonical programmatic creation path.
# Direct task_store::create_task is only allowed in:
#   - engine/spawner.rs (the central spawner itself)
#   - engine/task_store.rs (the low-level CRUD module)
#   - commands/tasks.rs (user-facing create_task command)
#   - test files
#   - engine/executor.rs (for test helpers)

set -euo pipefail

cd "$(dirname "$0")/.."

# Find all Rust files that contain "task_store::create_task"
# Exclude allowed locations, test modules, binary smoke tests, and test helper files
violations=$(grep -rn "task_store::create_task" src-tauri/src --include="*.rs" | \
    grep -v "engine/spawner.rs" | \
    grep -v "engine/task_store.rs" | \
    grep -v "commands/tasks.rs" | \
    grep -v "executor.rs" | \
    grep -v "bin/" | \
    grep -v "#\[cfg(test)\]" | \
    grep -v "mod tests" | \
    grep -v "engine/exec/ctr_audit/" || true)

if [ -n "$violations" ]; then
    echo "ERROR: Direct task_store::create_task calls found outside allowlist."
    echo "All programmatic task creation must go through TaskSpawner::spawn() or TaskSpawner::spawn_follow_up()."
    echo ""
    echo "Violations:"
    echo "$violations"
    echo ""
    echo "Allowed locations: engine/spawner.rs, engine/task_store.rs, commands/tasks.rs, executor.rs (tests only)"
    exit 1
fi

echo "OK: No direct task_store::create_task calls outside allowlist."
