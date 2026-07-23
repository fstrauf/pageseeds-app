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

# Find all Rust files that contain "task_store::create_task".
# Exclude allowed locations, binary smoke tests, and calls inside #[cfg(test)] modules.
violations=$(grep -rn "task_store::create_task" src-tauri/src --include="*.rs" | while IFS=: read -r file line text; do
    if [[ "$file" == *engine/spawner.rs \
        || "$file" == *engine/task_store.rs \
        || "$file" == *commands/tasks.rs \
        || "$file" == *executor.rs \
        || "$file" == *executor/tests.rs \
        || "$file" == *tests.rs \
        || "$file" == *bin/* \
        || "$file" == *engine/exec/ctr_audit/* ]]; then
        continue
    fi

    # If a #[cfg(test)] marker appears before this line in the same file, treat it
    # as test fixture setup. This keeps the guard focused on production paths.
    if awk -v target_line="$line" 'NR > target_line { exit } /^[[:space:]]*#\[cfg\(test\)\]/ { found = 1 } END { exit found ? 0 : 1 }' "$file"; then
        continue
    fi

    printf '%s:%s:%s\n' "$file" "$line" "$text"
done || true)

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
