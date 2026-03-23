#!/bin/bash
# PageSeeds macOS Release Build
# Produces:
#   - Signed + notarized DMG  (PageSeeds_<ver>_universal.dmg)
#   - Update artifact          (PageSeeds_<ver>_universal.app.tar.gz)
#   - Update manifest          (latest.json)
# Writes artifact paths to .build_dmg_path and .build_tar_gz_path for publish-release.sh.
#
# Usage:
#   ./build-release.sh                 # full build + notarization
#   ./build-release.sh --skip-notarize # skip notarization (signed only)
#   ./build-release.sh --bump          # bump patch before building (e.g. 0.1.0 → 0.1.1)
#   ./build-release.sh --bump=minor    # bump minor (e.g. 0.1.0 → 0.2.0)
#   ./build-release.sh --bump=major    # bump major (e.g. 0.1.0 → 1.0.0)

set -e

# ── Parse flags ───────────────────────────────────────────────────────────────
SKIP_NOTARIZE=false
BUMP_SEGMENT=""
for arg in "$@"; do
    case "$arg" in
        --skip-notarize)  SKIP_NOTARIZE=true ;;
        --bump)           BUMP_SEGMENT="patch" ;;
        --bump=patch)     BUMP_SEGMENT="patch" ;;
        --bump=minor)     BUMP_SEGMENT="minor" ;;
        --bump=major)     BUMP_SEGMENT="major" ;;
    esac
done

# ── Load environment ──────────────────────────────────────────────────────────
if [ -f .env ]; then
    set -a; source .env; set +a
    echo "✅ Loaded .env"
else
    echo "⚠️  No .env file — using shell environment"
fi

# Cleanup lingering mounted DMG volumes
for vol in /Volumes/PageSeeds*; do
    [ -d "$vol" ] && hdiutil detach "$vol" 2>/dev/null || true
done

echo ""
echo "🚀 PageSeeds macOS Release Build"
echo "================================="
echo ""

# ── Version consistency ───────────────────────────────────────────────────────
VERSION=$(grep -o '"version": "[^"]*"' src-tauri/tauri.conf.json | head -1 | cut -d'"' -f4)
CARGO_VERSION=$(grep -o '^version = "[^"]*"' src-tauri/Cargo.toml | cut -d'"' -f2)
PACKAGE_VERSION=$(grep -o '"version": "[^"]*"' package.json | head -1 | cut -d'"' -f4)

if [ "$VERSION" != "$CARGO_VERSION" ] || [ "$VERSION" != "$PACKAGE_VERSION" ]; then
    echo "❌ Version mismatch!"
    echo "   tauri.conf.json : $VERSION"
    echo "   Cargo.toml      : $CARGO_VERSION"
    echo "   package.json    : $PACKAGE_VERSION"
    exit 1
fi

# ── Optional version bump ─────────────────────────────────────────────────────
if [ -n "$BUMP_SEGMENT" ]; then
    MAJOR=$(echo "$VERSION" | cut -d. -f1)
    MINOR=$(echo "$VERSION" | cut -d. -f2)
    PATCH=$(echo "$VERSION" | cut -d. -f3)
    case "$BUMP_SEGMENT" in
        major) MAJOR=$((MAJOR+1)); MINOR=0; PATCH=0 ;;
        minor) MINOR=$((MINOR+1)); PATCH=0 ;;
        patch) PATCH=$((PATCH+1)) ;;
    esac
    NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
    echo "🔢 Bumping version: $VERSION → $NEW_VERSION"
    # Update all three files atomically
    sed -i '' "s/\"version\": \"${VERSION}\"/\"version\": \"${NEW_VERSION}\"/" src-tauri/tauri.conf.json
    sed -i '' "s/^version = \"${VERSION}\"/version = \"${NEW_VERSION}\"/" src-tauri/Cargo.toml
    sed -i '' "s/\"version\": \"${VERSION}\"/\"version\": \"${NEW_VERSION}\"/" package.json
    VERSION="$NEW_VERSION"
    echo "✅ Version bumped to $VERSION"
fi

echo "📦 Building version: $VERSION"
echo "✅ Versions consistent"
echo ""

# ── Signing key ───────────────────────────────────────────────────────────────
KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY_PATH:-~/.tauri/pageseeds.key}"
KEY_PATH="${KEY_PATH/#\~/$HOME}"

if [ ! -f "$KEY_PATH" ]; then
    echo "❌ Signing key not found: $KEY_PATH"
    echo "   Generate it with: pnpm tauri signer generate -w ~/.tauri/pageseeds.key"
    exit 1
fi
if [ -z "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" ]; then
    echo "❌ TAURI_SIGNING_PRIVATE_KEY_PASSWORD not set"
    exit 1
fi

export TAURI_SIGNING_PRIVATE_KEY=$(cat "$KEY_PATH")
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
echo "✅ Signing key loaded (${#TAURI_SIGNING_PRIVATE_KEY} chars)"
echo ""

# ── Clean old artifacts ───────────────────────────────────────────────────────
MACOS_BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle/macos"
DMG_BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle/dmg"
rm -rf "$MACOS_BUNDLE_DIR"/*.tar.gz "$MACOS_BUNDLE_DIR"/*.sig 2>/dev/null || true
rm -rf "$DMG_BUNDLE_DIR"/*.dmg 2>/dev/null || true
rm -rf "$MACOS_BUNDLE_DIR/PageSeeds.app" 2>/dev/null || true
echo "✅ Old artifacts cleaned"
echo ""

# ── Build (Tauri handles signing; we handle notarization manually below) ──────
# Unset Apple notarization vars so Tauri never attempts its own notarization.
# We call xcrun notarytool directly after the build (same as Expense Sorted).
echo "🔨 Building universal binary (Intel + Apple Silicon)..."
APPLE_ID_SAVED="$APPLE_ID"
APPLE_PASSWORD_SAVED="$APPLE_PASSWORD"
APPLE_TEAM_ID_SAVED="$APPLE_TEAM_ID"
unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
pnpm tauri build --target universal-apple-darwin
export APPLE_ID="$APPLE_ID_SAVED"
export APPLE_PASSWORD="$APPLE_PASSWORD_SAVED"
export APPLE_TEAM_ID="$APPLE_TEAM_ID_SAVED"

# ── Verify update artifact ────────────────────────────────────────────────────
APP_TAR_GZ=$(find "$MACOS_BUNDLE_DIR" -name "*.app.tar.gz" 2>/dev/null | head -1)
if [ -z "$APP_TAR_GZ" ] || [ ! -f "$APP_TAR_GZ" ]; then
    echo "❌ .app.tar.gz not found — check that createUpdaterArtifacts: true in tauri.conf.json"
    exit 1
fi

SIG_FILE="${APP_TAR_GZ}.sig"
if [ ! -f "$SIG_FILE" ]; then
    echo "❌ Signature file not found: $SIG_FILE"
    exit 1
fi

# ── Locate and rename DMG ─────────────────────────────────────────────────────
DMG_PATH=$(find "$DMG_BUNDLE_DIR" -name "*.dmg" 2>/dev/null | head -1)
if [ -z "$DMG_PATH" ] || [ ! -f "$DMG_PATH" ]; then
    echo "❌ DMG not found in $DMG_BUNDLE_DIR"
    exit 1
fi

DMG_DIR=$(dirname "$DMG_PATH")
DMG_SAFE="$DMG_DIR/PageSeeds_${VERSION}_universal.dmg"
[ "$DMG_PATH" != "$DMG_SAFE" ] && mv "$DMG_PATH" "$DMG_SAFE"
DMG_PATH="$DMG_SAFE"

# ── Notarize DMG with xcrun notarytool (manual, same as Expense Sorted) ───────
if [ "$SKIP_NOTARIZE" = true ]; then
    echo "⚠️  --skip-notarize: skipping notarization"
elif [ -z "$APPLE_ID" ] || [ -z "$APPLE_PASSWORD" ] || [ -z "$APPLE_TEAM_ID" ]; then
    echo "⚠️  APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID not set — skipping notarization"
else
    echo ""
    echo "🍎 Notarizing DMG with Apple (xcrun notarytool)..."
    xcrun notarytool submit "$DMG_PATH" \
        --apple-id "$APPLE_ID" \
        --password "$APPLE_PASSWORD" \
        --team-id "$APPLE_TEAM_ID" \
        --wait
    echo "📌 Stapling notarization ticket..."
    xcrun stapler staple "$DMG_PATH"
    echo "✅ Notarization + staple complete"
fi

# ── Copy and rename tar.gz ────────────────────────────────────────────────────
TAR_GZ_SAFE="$MACOS_BUNDLE_DIR/PageSeeds_${VERSION}_universal.app.tar.gz"
cp "$APP_TAR_GZ" "$TAR_GZ_SAFE"

# ── Generate latest.json ──────────────────────────────────────────────────────
SIGNATURE=$(cat "$SIG_FILE")
RELEASES_REPO="fstrauf/pageseeds-releases"
TAR_URL="https://github.com/$RELEASES_REPO/releases/download/v${VERSION}/PageSeeds_${VERSION}_universal.app.tar.gz"
PUB_DATE=$(date -u +%Y-%m-%dT%H:%M:%SZ)

cat > latest.json << EOF
{
  "version": "$VERSION",
  "notes": "Release v$VERSION",
  "pub_date": "$PUB_DATE",
  "platforms": {
    "darwin-aarch64": {
      "signature": "$SIGNATURE",
      "url": "$TAR_URL"
    },
    "darwin-x86_64": {
      "signature": "$SIGNATURE",
      "url": "$TAR_URL"
    }
  }
}
EOF

echo "✅ latest.json generated"

# Write paths for publish-release.sh
echo "$DMG_PATH" > .build_dmg_path
echo "$TAR_GZ_SAFE" > .build_tar_gz_path

echo ""
echo "✅ macOS build complete"
echo "   DMG:      $DMG_PATH"
echo "   tar.gz:   $TAR_GZ_SAFE"
echo "   Manifest: latest.json"
