#!/usr/bin/env bash
# Binding staleness check: fail if Rust ts-rs exports differ from committed bindings.
# This script can be run in CI or as a pre-commit hook.

set -euo pipefail

cd "$(dirname "$0")/.."

echo "[check-bindings] Generating TypeScript bindings from Rust..."
cd src-tauri
cargo test export_bindings --lib --quiet
cd ..

echo "[check-bindings] Comparing generated bindings against src/lib/bindings/..."
if ! diff -r src-tauri/bindings/ src/lib/bindings/ > /dev/null 2>&1; then
    echo "[check-bindings] FAIL: Bindings are stale. Run ./scripts/sync-bindings.sh and commit the changes."
    diff -r src-tauri/bindings/ src/lib/bindings/ || true
    exit 1
fi

echo "[check-bindings] OK: Bindings are up to date."
