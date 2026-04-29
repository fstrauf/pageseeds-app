#!/usr/bin/env bash
# Flag new local JSON extraction functions in engine/exec/ when shared helpers exist.
# Shared helpers: engine::text::extract_json, engine::text::extract_json_as, engine::text::extract_json_string
#
# This is advisory — some domain-specific extraction may be legitimate.
# If flagged code has a genuine reason, add a comment explaining why the shared helper cannot be used.

set -euo pipefail

cd "$(dirname "$0")/.."

advisory=0

# 1. New functions named extract_json* or parse_json* in engine/exec/
new_extractors=$(grep -rn "^\s*pub\s\+fn\s\+\(extract_json\|parse_json\)" src-tauri/src/engine/exec/ --include="*.rs" | grep -v "// ok:" || true)

if [ -n "$new_extractors" ]; then
    echo "ADVISORY: New JSON extraction functions found in engine/exec/."
    echo "Consider using the shared helpers in engine::text instead:"
    echo "  - engine::text::extract_json(text) -> Option<Value>"
    echo "  - engine::text::extract_json_as::<T>(text) -> Option<T>"
    echo "  - engine::text::extract_json_string(text) -> Option<String>"
    echo ""
    echo "Flagged functions:"
    echo "$new_extractors"
    echo ""
    advisory=1
fi

# 2. Regex-based JSON extraction in engine/exec/ (pattern: find('{') + find('}') or similar)
regex_extractors=$(grep -rn "find('{')\|find('}')\|rfind('{')\|rfind('}')" src-tauri/src/engine/exec/ --include="*.rs" | grep -v "// ok:" || true)

if [ -n "$regex_extractors" ]; then
    echo "ADVISORY: Manual JSON boundary detection found in engine/exec/."
    echo "This is often a sign of re-implementing extract_json. Use engine::text::extract_json instead."
    echo ""
    echo "Flagged lines:"
    echo "$regex_extractors"
    echo ""
    advisory=1
fi

if [ "$advisory" -eq 1 ]; then
    echo "If the flagged code genuinely needs domain-specific extraction, add '// ok: <reason>' on the same line."
    exit 0  # Advisory only — does not fail CI
fi

echo "OK: No new local JSON extraction functions flagged in engine/exec/."
