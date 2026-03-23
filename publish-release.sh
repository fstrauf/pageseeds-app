#!/bin/bash
# PageSeeds Interactive Release Publisher
# Bumps version, builds macOS (+ optionally Windows), and publishes to GitHub Releases.

set -e

echo ""
echo "🚀 PageSeeds Release Publisher"
echo "================================"
echo ""

# ── Pre-release checks ────────────────────────────────────────────────────────
if ! ./scripts/pre-release-checks.sh; then
    echo ""
    echo "❌ Pre-release checks failed. Fix the issues above and try again."
    exit 1
fi

echo ""
echo "✅ Pre-release checks passed — proceeding..."
echo ""

if [ -f .env ]; then
    set -a; source .env; set +a
fi

# ── Version selection ─────────────────────────────────────────────────────────
CURRENT_VERSION=$(grep -o '"version": "[^"]*"' src-tauri/tauri.conf.json | head -1 | cut -d'"' -f4)
echo "📦 Current version: v$CURRENT_VERSION"
echo ""

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"
NEXT_PATCH="$MAJOR.$MINOR.$((PATCH + 1))"
NEXT_MINOR="$MAJOR.$((MINOR + 1)).0"
NEXT_MAJOR="$((MAJOR + 1)).0.0"

echo "Select version bump type:"
echo "  1) Patch  ($CURRENT_VERSION → $NEXT_PATCH) — Bug fixes"
echo "  2) Minor  ($CURRENT_VERSION → $NEXT_MINOR) — New features"
echo "  3) Major  ($CURRENT_VERSION → $NEXT_MAJOR) — Breaking changes"
echo "  4) Custom — Enter version manually"
echo "  5) Keep   — Re-release current version ($CURRENT_VERSION)"
echo ""
read -p "Choice [1-5]: " VERSION_CHOICE

case $VERSION_CHOICE in
    1) NEW_VERSION="$NEXT_PATCH" ;;
    2) NEW_VERSION="$NEXT_MINOR" ;;
    3) NEW_VERSION="$NEXT_MAJOR" ;;
    4) read -p "Enter version (e.g. 1.2.3): " NEW_VERSION ;;
    5) NEW_VERSION="$CURRENT_VERSION" ;;
    *) echo "Invalid choice"; exit 1 ;;
esac

read -p "Release notes (Enter for default): " RELEASE_NOTES
[ -z "$RELEASE_NOTES" ] && RELEASE_NOTES="Release v$NEW_VERSION"

echo ""
echo "📋 Release summary:"
echo "   Version : v$NEW_VERSION"
echo "   Notes   : $RELEASE_NOTES"
echo ""
read -p "Continue? [y/N]: " CONFIRM
[[ ! "$CONFIRM" =~ ^[Yy]$ ]] && echo "Aborted." && exit 0

# ── Bump versions ─────────────────────────────────────────────────────────────
if [ "$NEW_VERSION" != "$CURRENT_VERSION" ]; then
    echo ""
    echo "📝 Updating version numbers..."
    sed -i '' "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" src-tauri/tauri.conf.json
    sed -i '' "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" src-tauri/Cargo.toml
    sed -i '' "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" package.json
    echo "✅ Version bumped to $NEW_VERSION in all 3 files"
fi

# ── Clean + build macOS ───────────────────────────────────────────────────────
echo ""
echo "🧹 Cleaning Cargo artifacts..."
cargo clean --manifest-path src-tauri/Cargo.toml

echo ""
echo "🔨 Building macOS release..."
./build-release.sh

DMG_PATH=$(cat .build_dmg_path)
TAR_GZ_PATH=$(cat .build_tar_gz_path)

[ ! -f "$DMG_PATH" ]     && echo "❌ DMG missing after build"     && exit 1
[ ! -f "$TAR_GZ_PATH" ]  && echo "❌ tar.gz missing after build"  && exit 1
[ ! -f "latest.json" ]   && echo "❌ latest.json missing"         && exit 1

echo ""
echo "✅ macOS build complete"
echo "   DMG:    $DMG_PATH"
echo "   tar.gz: $TAR_GZ_PATH"

# ── Optional Windows build ────────────────────────────────────────────────────
WINDOWS_EXE_PATH=""
WINDOWS_SIGNATURE=""

echo ""
read -p "🪟 Build Windows installer too? [y/N]: " BUILD_WINDOWS
if [[ "$BUILD_WINDOWS" =~ ^[Yy]$ ]]; then
    if ! command -v makensis &>/dev/null; then
        echo ""
        echo "⚠️  Windows build prerequisites missing:"
        echo "   brew install nsis llvm"
        echo "   rustup target add x86_64-pc-windows-msvc"
        echo "   cargo install --locked cargo-xwin"
        read -p "Skip Windows and continue with macOS only? [Y/n]: " SKIP_WIN
        [[ "$SKIP_WIN" =~ ^[Nn]$ ]] && exit 1
    else
        echo ""
        echo "🔨 Building Windows release..."
        if ./build-windows.sh; then
            WINDOWS_EXE_PATH=$(cat .build_windows_exe_path)
            WINDOWS_SIGNATURE=$(cat .build_windows_signature 2>/dev/null || echo "")
            echo "✅ Windows installer: $WINDOWS_EXE_PATH"
        else
            echo "⚠️  Windows build failed — continuing with macOS only"
            read -p "Continue? [Y/n]: " CONT
            [[ "$CONT" =~ ^[Nn]$ ]] && exit 1
        fi
    fi
fi

# ── Inject Windows into latest.json ──────────────────────────────────────────
if [ -n "$WINDOWS_EXE_PATH" ] && [ -f "$WINDOWS_EXE_PATH" ]; then
    WINDOWS_EXE_NAME=$(basename "$WINDOWS_EXE_PATH")
    python3 << EOF
import json
with open('latest.json') as f:
    data = json.load(f)
data['platforms']['windows-x86_64'] = {
    'signature': '$WINDOWS_SIGNATURE',
    'url': 'https://github.com/fstrauf/pageseeds-releases/releases/download/v$NEW_VERSION/$WINDOWS_EXE_NAME'
}
with open('latest.json', 'w') as f:
    json.dump(data, f, indent=2)
print('✅ latest.json updated with Windows platform')
EOF
fi

# ── GitHub release ────────────────────────────────────────────────────────────
if ! command -v gh &>/dev/null; then
    echo ""
    echo "⚠️  GitHub CLI (gh) not installed — manual upload required."
    echo "   Install with: brew install gh"
    echo "   Then go to: https://github.com/fstrauf/pageseeds-releases/releases/new"
    echo "   Upload: $DMG_PATH, $TAR_GZ_PATH, latest.json"
    exit 0
fi

if ! gh auth status &>/dev/null; then
    echo "⚠️  Not authenticated with gh. Run: gh auth login"
    exit 1
fi

RELEASES_REPO="fstrauf/pageseeds-releases"

if gh release view "v$NEW_VERSION" --repo "$RELEASES_REPO" &>/dev/null; then
    echo "⚠️  Release v$NEW_VERSION already exists."
    read -p "Delete and recreate? [y/N]: " DEL_CONFIRM
    if [[ "$DEL_CONFIRM" =~ ^[Yy]$ ]]; then
        gh release delete "v$NEW_VERSION" --repo "$RELEASES_REPO" --yes
    else
        echo "Aborted."
        exit 1
    fi
fi

# Version-agnostic copies so /latest/download/ URLs always work
DMG_DIR=$(dirname "$DMG_PATH")
DMG_LATEST="$DMG_DIR/PageSeeds_universal.dmg"
cp "$DMG_PATH" "$DMG_LATEST"

UPLOAD_FILES=("$DMG_PATH" "$DMG_LATEST" "$TAR_GZ_PATH" "latest.json")

if [ -n "$WINDOWS_EXE_PATH" ] && [ -f "$WINDOWS_EXE_PATH" ]; then
    WIN_DIR=$(dirname "$WINDOWS_EXE_PATH")
    WIN_LATEST="$WIN_DIR/PageSeeds_x64-setup.exe"
    cp "$WINDOWS_EXE_PATH" "$WIN_LATEST"
    UPLOAD_FILES+=("$WINDOWS_EXE_PATH" "$WIN_LATEST")
fi

echo ""
echo "📤 Creating GitHub release v$NEW_VERSION..."
gh release create "v$NEW_VERSION" \
    --repo "$RELEASES_REPO" \
    --title "PageSeeds v$NEW_VERSION" \
    --notes "$RELEASE_NOTES" \
    "${UPLOAD_FILES[@]}"

echo ""
echo "✅ Released: https://github.com/$RELEASES_REPO/releases/tag/v$NEW_VERSION"
echo ""
echo "Download URLs (for the website):"
echo "  macOS:   https://github.com/$RELEASES_REPO/releases/latest/download/PageSeeds_universal.dmg"
echo "  Windows: https://github.com/$RELEASES_REPO/releases/latest/download/PageSeeds_x64-setup.exe"
