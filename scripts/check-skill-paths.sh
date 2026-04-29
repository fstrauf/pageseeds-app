#!/usr/bin/env bash
# Fail if stale skill paths are referenced in docs or code.
# Skills live at .github/skills/{name}/SKILL.md, NOT .github/automation/skills/.

set -euo pipefail

cd "$(dirname "$0")/.."

violations=$(grep -rn "\.github/automation/skills" --include="*.md" --include="*.rs" --include="*.ts" --include="*.tsx" . | \
    grep -v "node_modules/" | \
    grep -v "src-tauri/target/" | \
    grep -v ".git/" || true)

if [ -n "$violations" ]; then
    echo "ERROR: Stale skill path references found."
    echo "Skills live at .github/skills/{name}/SKILL.md (project-level) or are embedded in src-tauri/src/skills/ (app defaults)."
    echo "They do NOT live at .github/automation/skills/."
    echo ""
    echo "Violations:"
    echo "$violations"
    exit 1
fi

echo "OK: No stale .github/automation/skills references."
