#!/usr/bin/env bash
# Fail if invoke() is called directly outside tauri.ts.
# All IPC calls must go through typed wrappers in src/lib/tauri.ts.

set -euo pipefail

cd "$(dirname "$0")/.."

# Find direct invoke() calls outside tauri.ts
# Allowlist: tauri.ts (canonical wrappers), tauri.ts test files, and dynamic wrapper patterns
violations=$(grep -rn "invoke(" src/ --include="*.ts" --include="*.tsx" | \
    grep -v "src/lib/tauri.ts" | \
    grep -v "src/lib/tauri.test.ts" | \
    grep -v "from ['\"]@tauri-apps/api/tauri['\"]" | \
    grep -v "import.*invoke.*from" | \
    grep -v "// allowlisted" || true)

if [ -n "$violations" ]; then
    echo "ERROR: Direct invoke() calls found outside src/lib/tauri.ts."
    echo "All IPC calls must use typed wrappers from tauri.ts. Do not call invoke() inline in components."
    echo ""
    echo "Violations:"
    echo "$violations"
    echo ""
    echo "Fix: Add a wrapper in src/lib/tauri.ts and import it in your component."
    exit 1
fi

echo "OK: No direct invoke() calls outside src/lib/tauri.ts."
