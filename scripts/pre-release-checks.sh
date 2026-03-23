#!/bin/bash
# PageSeeds pre-release validation checks

set -e
ERRORS=0

echo "🔍 Running pre-release checks..."
echo ""

# ── Version consistency ────────────────────────────────────────────────────────
V_TAURI=$(grep -o '"version": "[^"]*"' src-tauri/tauri.conf.json | head -1 | cut -d'"' -f4)
V_CARGO=$(grep -o '^version = "[^"]*"' src-tauri/Cargo.toml | cut -d'"' -f2)
V_PKG=$(grep -o '"version": "[^"]*"' package.json | head -1 | cut -d'"' -f4)

if [ "$V_TAURI" != "$V_CARGO" ] || [ "$V_TAURI" != "$V_PKG" ]; then
    echo "❌ Version mismatch:"
    echo "   tauri.conf.json : $V_TAURI"
    echo "   Cargo.toml      : $V_CARGO"
    echo "   package.json    : $V_PKG"
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Versions consistent: $V_TAURI"
fi

# ── Signing key ─────────────────────────────────────────────────────────────
if [ -f .env ]; then
    set -a; source .env; set +a
fi

KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY_PATH:-~/.tauri/pageseeds.key}"
KEY_PATH="${KEY_PATH/#\~/$HOME}"

if [ ! -f "$KEY_PATH" ]; then
    echo "❌ Signing key not found: $KEY_PATH"
    echo "   Generate it with: pnpm tauri signer generate -w ~/.tauri/pageseeds.key"
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Signing key found: $KEY_PATH"
fi

# ── Signing key password ─────────────────────────────────────────────────────
if [ -z "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" ]; then
    echo "❌ TAURI_SIGNING_PRIVATE_KEY_PASSWORD not set (add it to .env)"
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Signing password present"
fi

# ── Pubkey placeholder check ─────────────────────────────────────────────────
if grep -q "REPLACE_WITH_PUBKEY" src-tauri/tauri.conf.json; then
    echo "❌ Pubkey placeholder still in tauri.conf.json — paste the real pubkey from ~/.tauri/pageseeds.key.pub"
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Pubkey configured"
fi

# ── Apple Developer certificate ──────────────────────────────────────────────
if command -v security &>/dev/null; then
    if security find-identity -v -p codesigning 2>/dev/null | grep -q "KMJ36PKPW8"; then
        echo "✅ Apple Developer ID certificate found in keychain"
    else
        echo "⚠️  Apple Developer ID (KMJ36PKPW8) not found in keychain — notarization will fail"
    fi
fi

# ── cargo check ─────────────────────────────────────────────────────────────
echo ""
echo "🔨 Running cargo check..."
if cargo check --manifest-path src-tauri/Cargo.toml --quiet 2>/dev/null; then
    echo "✅ cargo check passed"
else
    echo "❌ cargo check failed"
    ERRORS=$((ERRORS + 1))
fi

# ── TypeScript check ─────────────────────────────────────────────────────────
echo "🔨 Running tsc --noEmit..."
if pnpm tsc --noEmit 2>/dev/null; then
    echo "✅ TypeScript check passed"
else
    echo "⚠️  TypeScript errors found (non-blocking)"
fi

echo ""
if [ $ERRORS -gt 0 ]; then
    echo "❌ $ERRORS check(s) failed — fix them before releasing"
    exit 1
fi

echo "✅ All pre-release checks passed"
