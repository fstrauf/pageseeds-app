#!/usr/bin/env bash
# CI guard: list direct runtime reads of articles.json outside approved modules.
#
# Approved modules:
#   - src-tauri/src/db/export.rs          (canonical import/export/projection)
#   - src-tauri/src/commands/articles.rs  (thin wrappers around export.rs)
#   - src-tauri/src/engine/setup_check.rs (setup diagnostics)
#
# Any new hit must be explicitly added to the ALLOWLIST or migrated to use
# the article-index service / SQLite.

set -euo pipefail

cd "$(dirname "$0")/.."

ALLOWLIST="
src-tauri/src/db/export.rs
src-tauri/src/commands/articles.rs
src-tauri/src/engine/setup_check.rs
"

# Find all files containing "articles.json"
FILES=$(grep -rln "articles\.json" src-tauri/src/ || true)

if [ -z "$FILES" ]; then
  echo "OK: No direct articles.json access found anywhere."
  exit 0
fi

# Filter out allowed files
VIOLATIONS=""
for f in $FILES; do
  if ! echo "$ALLOWLIST" | grep -qx "$f"; then
    VIOLATIONS="$VIOLATIONS $f"
  fi
done

if [ -n "$VIOLATIONS" ]; then
  echo "ERROR: Direct articles.json access found outside approved modules:"
  echo ""
  for f in $VIOLATIONS; do
    # Show the matching lines for context
    grep -n "articles\.json" "$f" | head -n 5
  done
  echo ""
  echo "Approved modules are:"
  echo "$ALLOWLIST" | sed '/^$/d' | sed 's/^/  - /'
  echo ""
  echo "If your change intentionally touches articles.json, either:"
  echo "  1. Add the file to ALLOWLIST in scripts/check-articles-json-access.sh, or"
  echo "  2. Migrate the access to use the article-index service / SQLite."
  exit 1
fi

echo "OK: No unapproved direct articles.json access found."
