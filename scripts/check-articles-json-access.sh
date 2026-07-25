#!/usr/bin/env bash
# CI guard: list direct runtime reads of articles.json outside approved modules.
#
# Approved modules (domain SoT after #183 crate split):
#   - crates/pageseeds-core/src/db/export.rs              (canonical import/export/projection)
#   - crates/pageseeds-core/src/engine/setup_check/       (setup diagnostics)
#   - src-tauri/src/commands/articles.rs                  (residual desktop IPC wrappers; #184)
#
# Any new hit must be explicitly added to the ALLOWLIST or migrated to use
# the article-index service / SQLite.

set -euo pipefail

cd "$(dirname "$0")/.."

# Exact files and directory prefixes (trailing / = any file under that tree).
ALLOWLIST_EXACT="
crates/pageseeds-core/src/db/export.rs
src-tauri/src/commands/articles.rs
"
ALLOWLIST_PREFIXES="
crates/pageseeds-core/src/engine/setup_check/
"

is_allowed() {
  local f="$1"
  if echo "$ALLOWLIST_EXACT" | grep -qx "$f"; then
    return 0
  fi
  local p
  while IFS= read -r p; do
    [ -z "$p" ] && continue
    case "$f" in
      "$p"*) return 0 ;;
    esac
  done <<< "$ALLOWLIST_PREFIXES"
  return 1
}

# Scan domain SoT + residual desktop commands (domain under src-tauri was deleted).
FILES=$(
  {
    grep -rln "articles\.json" crates/pageseeds-core/src/ 2>/dev/null || true
    grep -rln "articles\.json" src-tauri/src/commands/ 2>/dev/null || true
  } | sort -u
)

if [ -z "$FILES" ]; then
  echo "OK: No direct articles.json access found anywhere."
  exit 0
fi

# Filter out allowed files
VIOLATIONS=""
for f in $FILES; do
  if ! is_allowed "$f"; then
    VIOLATIONS="$VIOLATIONS $f"
  fi
done

if [ -n "$VIOLATIONS" ]; then
  echo "ERROR: Direct articles.json access found outside approved modules:"
  echo ""
  for f in $VIOLATIONS; do
    # Show the matching lines for context
    grep -n "articles\.json" "$f" | head -n 5
    echo "  ($f)"
    echo ""
  done
  echo "Approved modules are:"
  echo "$ALLOWLIST_EXACT" | sed '/^$/d' | sed 's/^/  - /'
  echo "$ALLOWLIST_PREFIXES" | sed '/^$/d' | sed 's|^|  - |;s|/$|/…|'
  echo ""
  echo "If your change intentionally touches articles.json, either:"
  echo "  1. Add the file to ALLOWLIST in scripts/check-articles-json-access.sh, or"
  echo "  2. Migrate the access to use the article-index service / SQLite."
  exit 1
fi

echo "OK: No unapproved direct articles.json access found."
