#!/bin/bash
# PageSeeds Windows Release Build (cross-compile from macOS via cargo-xwin)
#
# Prerequisites (install once):
#   brew install nsis llvm
#   rustup target add x86_64-pc-windows-msvc
#   cargo install --locked cargo-xwin
#
# Writes installer path to .build_windows_exe_path and signature to .build_windows_signature.

set -e

if [ -f .env ]; then
    set -a; source .env; set +a
fi

VERSION=$(grep -o '"version": "[^"]*"' src-tauri/tauri.conf.json | head -1 | cut -d'"' -f4)
echo "🪟 Building Windows installer for v$VERSION"

# ── Signing key ───────────────────────────────────────────────────────────────
KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY_PATH:-~/.tauri/pageseeds.key}"
KEY_PATH="${KEY_PATH/#\~/$HOME}"

if [ ! -f "$KEY_PATH" ]; then
    echo "❌ Signing key not found: $KEY_PATH"
    exit 1
fi
if [ -z "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" ]; then
    echo "❌ TAURI_SIGNING_PRIVATE_KEY_PASSWORD not set"
    exit 1
fi

export TAURI_SIGNING_PRIVATE_KEY=$(cat "$KEY_PATH")
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
echo "✅ Signing key loaded"

# ── Check prerequisites ───────────────────────────────────────────────────────
if ! command -v makensis &>/dev/null; then
    echo "❌ NSIS not found. Install with: brew install nsis"
    exit 1
fi

if ! rustup target list --installed | grep -q "x86_64-pc-windows-msvc"; then
    echo "❌ Rust Windows target not installed. Run: rustup target add x86_64-pc-windows-msvc"
    exit 1
fi

# ── Build ─────────────────────────────────────────────────────────────────────
echo "🔨 Cross-compiling for Windows x64..."
pnpm tauri build --target x86_64-pc-windows-msvc

# ── Locate and rename installer ───────────────────────────────────────────────
WIN_DIR="src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis"
EXE_PATH=$(find "$WIN_DIR" -name "*-setup.exe" 2>/dev/null | head -1)

if [ -z "$EXE_PATH" ] || [ ! -f "$EXE_PATH" ]; then
    echo "❌ Windows installer not found in $WIN_DIR"
    exit 1
fi

EXE_SAFE="$WIN_DIR/PageSeeds_${VERSION}_x64-setup.exe"
mv "$EXE_PATH" "$EXE_SAFE"

SIG_FILE="${EXE_SAFE}.sig"
SIGNATURE=""
if [ -f "$SIG_FILE" ]; then
    SIGNATURE=$(cat "$SIG_FILE")
fi

echo "$EXE_SAFE" > .build_windows_exe_path
echo "$SIGNATURE" > .build_windows_signature

echo ""
echo "✅ Windows build complete"
echo "   Installer: $EXE_SAFE"
