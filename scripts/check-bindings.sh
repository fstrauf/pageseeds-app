#!/usr/bin/env bash
# Binding staleness check: fail if Rust ts-rs exports differ from committed bindings.
# This script can be run in CI or as a pre-commit hook.

set -euo pipefail

cd "$(dirname "$0")/.."

# Desktop shell is intentionally non-building until #184 (post #183 crate split).
is_desktop_non_building() {
  if [ "${DESKTOP_SKIP:-}" = "1" ]; then
    return 0
  fi
  local lib="src-tauri/src/lib.rs"
  if [ ! -f "$lib" ]; then
    return 0
  fi
  if grep -q 'compile_error!' "$lib" && grep -Eq '#184|non-building' "$lib"; then
    return 0
  fi
  return 1
}

if is_desktop_non_building; then
  echo "Skipping IPC/bindings check: src-tauri desktop shell is non-building pending #184."
  exit 0
fi

echo "[check-bindings] Generating TypeScript bindings from Rust..."
cd src-tauri
cargo test export_bindings --lib --quiet
cd ..

echo "[check-bindings] Comparing generated bindings against src/lib/bindings/..."
# Exclude index.ts — it's generated only in src/lib/bindings/ by sync-bindings.sh
if ! diff -r --exclude="index.ts" src-tauri/bindings/ src/lib/bindings/ > /dev/null 2>&1; then
    echo "[check-bindings] FAIL: Bindings are stale. Run ./scripts/sync-bindings.sh and commit the changes."
    diff -r --exclude="index.ts" src-tauri/bindings/ src/lib/bindings/ || true
    exit 1
fi

echo "[check-bindings] OK: Bindings are up to date."
