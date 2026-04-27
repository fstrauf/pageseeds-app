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

# Build a grep invert pattern from the allowlist
INVERT=""
for f in $ALLOWLIST; do
  INVERT="$INVERT -e $f"
done

HITS=$(grep -rn "articles\.json" src-tauri/src/ $INVERT || true)

if [ -n "$HITS" ]; then
  echo "ERROR: Direct articles.json access found outside approved modules:"
  echo ""
  echo "$HITS"
  echo ""
  echo "Approved modules are:"
  for f in $ALLOWLIST; do
    echo "  - $f"
  done
  echo ""
  echo "If your change intentionally touches articles.json, either:"
  echo "  1. Add the file to ALLOWLIST in scripts/check-articles-json-access.sh, or"
  echo "  2. Migrate the access to use the article-index service / SQLite."
  exit 1
fi

echo "OK: No unapproved direct articles.json access found."
