# PageSeeds Desktop — Release Pipeline Spec

Reference implementation: `/Users/fstrauf/01_code/tx/ExpenseDesktop`

Current state: no signing, no updater, no build scripts, versions mismatched (`tauri.conf.json` = `0.1.0`, `package.json` = `0.0.0`).

This spec covers everything needed to go from dev build → signed/notarized artifacts → GitHub Releases → download page → in-app auto-update.

---

## Scope of Changes

| Area | File(s) | Change |
|---|---|---|
| Tauri config | `src-tauri/tauri.conf.json` | Add updater, signing, Windows NSIS, fix targets |
| Rust deps | `src-tauri/Cargo.toml` | Add `tauri-plugin-updater`, `tauri-plugin-process` |
| JS deps | `package.json` | Add npm plugin packages, add build/release scripts |
| Rust lib | `src-tauri/src/lib.rs` | Register updater + process plugins |
| macOS entitlements | `src-tauri/Entitlements.plist` | New file required for notarization |
| macOS bundle info | `src-tauri/Info.plist` | New file (hardened runtime flags) |
| Build scripts | `build-release.sh`, `build-windows.sh` | New files |
| Release script | `publish-release.sh` | New file |
| Pre-release checks | `scripts/pre-release-checks.sh` | New file |
| Env template | `.env.example` | New file (document secrets needed) |
| Public releases repo | GitHub | Create `fstrauf/pageseeds-releases` |
| Signing keypair | `~/.tauri/pageseeds.key` | Generate once, store pubkey in config |
| Download page | `pageseeds` website | Create `/download` route (out of scope here — separate spec) |

---

## Step 1 — Align Versions

All three files must carry the same version string at all times. Starting version: `0.1.0`.

**`src-tauri/tauri.conf.json`** — already `0.1.0`, keep.

**`src-tauri/Cargo.toml`** — already `0.1.0`, keep.

**`package.json`** — change `"version": "0.0.0"` → `"version": "0.1.0"`.

The `publish-release.sh` script will keep all three in sync from this point forward.

---

## Step 2 — Generate the Signing Keypair (one-time)

Tauri uses minisign to sign update artifacts. The public key goes in `tauri.conf.json`; the private key stays on the developer's machine only.

```bash
pnpm tauri signer generate -w ~/.tauri/pageseeds.key
```

This outputs:
- `~/.tauri/pageseeds.key` — private key (never committed)
- `~/.tauri/pageseeds.key.pub` — public key (copy the content into `tauri.conf.json`)
- A password is prompted — store it as `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in `.env`

---

## Step 3 — Update `tauri.conf.json`

Replace the current minimal config with:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "PageSeeds",
  "version": "0.1.0",
  "identifier": "com.pageseeds.app",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:5173",
    "beforeDevCommand": "pnpm dev",
    "beforeBuildCommand": "pnpm build"
  },
  "app": {
    "withGlobalTauri": true,
    "windows": [
      {
        "title": "PageSeeds",
        "width": 1280,
        "height": 800,
        "minWidth": 900,
        "minHeight": 600,
        "resizable": true,
        "fullscreen": false,
        "center": true
      }
    ],
    "security": {
      "csp": null
    }
  },
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://github.com/fstrauf/pageseeds-releases/releases/latest/download/latest.json"
      ],
      "pubkey": "<PASTE_PUBLIC_KEY_HERE>"
    }
  },
  "bundle": {
    "active": true,
    "createUpdaterArtifacts": true,
    "targets": ["app", "nsis"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "macOS": {
      "minimumSystemVersion": "10.13",
      "signingIdentity": "Developer ID Application: Florian Strauf (KMJ36PKPW8)",
      "entitlements": "./Entitlements.plist"
    },
    "windows": {
      "webviewInstallMode": {
        "type": "downloadBootstrapper"
      },
      "nsis": {
        "installMode": "currentUser",
        "displayLanguageSelector": false
      }
    }
  }
}
```

Key changes vs current:
- `withGlobalTauri: true` — exposes Tauri APIs on `window.__TAURI__`
- `plugins.updater` block — points to `fstrauf/pageseeds-releases` (the new public repo)
- `createUpdaterArtifacts: true` — produces `.app.tar.gz` alongside the DMG
- `targets: ["app", "nsis"]` — explicit; "all" includes formats we don't need
- `macOS.signingIdentity` — same Apple Developer ID used for Expense Sorted
- `macOS.entitlements` — required for notarization
- Windows NSIS block with `currentUser` install mode

---

## Step 4 — Add `Entitlements.plist`

Create `src-tauri/Entitlements.plist` (required for Apple notarization):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.cs.allow-jit</key>
  <true/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
  <true/>
  <key>com.apple.security.cs.disable-library-validation</key>
  <true/>
  <key>com.apple.security.network.client</key>
  <true/>
  <key>com.apple.security.files.user-selected.read-write</key>
  <true/>
</dict>
</plist>
```

Notes:
- `allow-jit` + `allow-unsigned-executable-memory` — required by WebKit for the Tauri webview
- `disable-library-validation` — allows loading Tauri's bundled libraries
- `network.client` — needed for the updater to check for new versions and for any outbound HTTP calls (LLM APIs, GSC, etc.)
- `files.user-selected.read-write` — allows reading/writing files the user selects via file picker dialogs

If pageseeds-app needs to access arbitrary file paths (e.g. the user's repo directory), also add:
```xml
  <key>com.apple.security.files.all</key>
  <true/>
```

---

## Step 5 — Update `Cargo.toml`

Add two new plugin deps:

```toml
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```

These are needed for the in-app update check flow (`tauri-plugin-updater` does the version check + download; `tauri-plugin-process` provides `process::restart()` after update install).

---

## Step 6 — Update `package.json`

Add npm packages:

```json
"@tauri-apps/plugin-updater": "^2",
"@tauri-apps/plugin-process": "^2"
```

Add scripts:

```json
"build:mac": "./build-release.sh",
"build:windows": "./build-windows.sh",
"release": "./publish-release.sh",
"check:pre-release": "./scripts/pre-release-checks.sh"
```

---

## Step 7 — Register Plugins in `lib.rs`

In `src-tauri/src/lib.rs`, add the updater and process plugins to the Tauri builder:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    // ... existing plugins
```

The updater plugin exposes `check_update` and `install_update` to the frontend via `@tauri-apps/plugin-updater`.

---

## Step 8 — Create `build-release.sh`

```bash
#!/bin/bash
# PageSeeds macOS Release Build
# Produces: signed+notarized DMG + .app.tar.gz update artifact + latest.json

set -e

if [ -f .env ]; then
    set -a; source .env; set +a
    echo "✅ Loaded .env"
fi

# Cleanup lingering mounted volumes
for vol in /Volumes/PageSeeds*; do
    [ -d "$vol" ] && hdiutil detach "$vol" 2>/dev/null || true
done

echo "🚀 PageSeeds Release Build"
echo ""

VERSION=$(grep -o '"version": "[^"]*"' src-tauri/tauri.conf.json | head -1 | cut -d'"' -f4)
CARGO_VERSION=$(grep -o '^version = "[^"]*"' src-tauri/Cargo.toml | cut -d'"' -f2)
PACKAGE_VERSION=$(grep -o '"version": "[^"]*"' package.json | head -1 | cut -d'"' -f4)

echo "📦 Version: $VERSION"

if [ "$VERSION" != "$CARGO_VERSION" ] || [ "$VERSION" != "$PACKAGE_VERSION" ]; then
    echo "❌ Version mismatch!"
    echo "   tauri.conf.json: $VERSION"
    echo "   Cargo.toml:      $CARGO_VERSION"
    echo "   package.json:    $PACKAGE_VERSION"
    exit 1
fi
echo "✅ Versions consistent"

# Load signing key
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

# Clean old artifacts
MACOS_BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle/macos"
DMG_BUNDLE_DIR="src-tauri/target/universal-apple-darwin/release/bundle/dmg"
rm -rf "$MACOS_BUNDLE_DIR"/*.tar.gz "$MACOS_BUNDLE_DIR"/*.sig 2>/dev/null || true
rm -rf "$DMG_BUNDLE_DIR"/*.dmg 2>/dev/null || true
rm -rf "$MACOS_BUNDLE_DIR/PageSeeds.app" 2>/dev/null || true
echo "✅ Old artifacts cleaned"

# Build
echo "🔨 Building universal binary..."
pnpm tauri build --target universal-apple-darwin

# Verify .app.tar.gz produced
APP_TAR_GZ=$(find "$MACOS_BUNDLE_DIR" -name "*.app.tar.gz" 2>/dev/null | head -1)
if [ -z "$APP_TAR_GZ" ] || [ ! -f "$APP_TAR_GZ" ]; then
    echo "❌ .app.tar.gz not found — is createUpdaterArtifacts: true in tauri.conf.json?"
    exit 1
fi

SIG_FILE="${APP_TAR_GZ}.sig"
if [ ! -f "$SIG_FILE" ]; then
    echo "❌ Signature file not found: $SIG_FILE"
    exit 1
fi

# Locate DMG
DMG_PATH=$(find "$DMG_BUNDLE_DIR" -name "*.dmg" 2>/dev/null | head -1)
if [ -z "$DMG_PATH" ] || [ ! -f "$DMG_PATH" ]; then
    echo "❌ DMG not found in $DMG_BUNDLE_DIR"
    exit 1
fi

# Rename to safe filename (no spaces)
DMG_DIR=$(dirname "$DMG_PATH")
DMG_SAFE="$DMG_DIR/PageSeeds_${VERSION}_universal.dmg"
mv "$DMG_PATH" "$DMG_SAFE"
DMG_PATH="$DMG_SAFE"

# Rename tar.gz too
TAR_GZ_SAFE="$MACOS_BUNDLE_DIR/PageSeeds_${VERSION}_universal.app.tar.gz"
cp "$APP_TAR_GZ" "$TAR_GZ_SAFE"

# Read signature
SIGNATURE=$(cat "$SIG_FILE")

# Generate latest.json
RELEASES_REPO="fstrauf/pageseeds-releases"
TAR_URL="https://github.com/$RELEASES_REPO/releases/download/v${VERSION}/PageSeeds_${VERSION}_universal.app.tar.gz"

cat > latest.json << EOF
{
  "version": "$VERSION",
  "notes": "Release v$VERSION",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
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

# Write paths for publish script to pick up
echo "$DMG_PATH" > .build_dmg_path
echo "$TAR_GZ_SAFE" > .build_tar_gz_path

echo ""
echo "✅ Build complete"
echo "   DMG:      $DMG_PATH"
echo "   tar.gz:   $TAR_GZ_SAFE"
echo "   Manifest: latest.json"
```

---

## Step 9 — Create `build-windows.sh`

Build the Windows NSIS installer by cross-compiling from macOS using `cargo-xwin`.

**Prerequisites (install once):**
```bash
brew install nsis llvm
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin
```

```bash
#!/bin/bash
# PageSeeds Windows Release Build (cross-compile from macOS)

set -e

if [ -f .env ]; then
    set -a; source .env; set +a
fi

VERSION=$(grep -o '"version": "[^"]*"' src-tauri/tauri.conf.json | head -1 | cut -d'"' -f4)
echo "🪟 Building Windows installer for v$VERSION"

KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY_PATH:-~/.tauri/pageseeds.key}"
KEY_PATH="${KEY_PATH/#\~/$HOME}"
export TAURI_SIGNING_PRIVATE_KEY=$(cat "$KEY_PATH")
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD

pnpm tauri build --target x86_64-pc-windows-msvc

WIN_DIR="src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis"
EXE_PATH=$(find "$WIN_DIR" -name "*-setup.exe" 2>/dev/null | head -1)

if [ -z "$EXE_PATH" ] || [ ! -f "$EXE_PATH" ]; then
    echo "❌ Windows installer not found in $WIN_DIR"
    exit 1
fi

# Rename to safe filename
EXE_SAFE="$WIN_DIR/PageSeeds_${VERSION}_x64-setup.exe"
mv "$EXE_PATH" "$EXE_SAFE"

SIG_FILE="${EXE_SAFE}.sig"
SIGNATURE=""
if [ -f "$SIG_FILE" ]; then
    SIGNATURE=$(cat "$SIG_FILE")
fi

echo "$EXE_SAFE" > .build_windows_exe_path
echo "$SIGNATURE" > .build_windows_signature

echo "✅ Windows build complete: $EXE_SAFE"
```

---

## Step 10 — Create `publish-release.sh`

Interactive script: bump version → build mac → optionally build windows → create GitHub release.

```bash
#!/bin/bash
# PageSeeds Interactive Release Publisher

set -e

echo ""
echo "🚀 PageSeeds Release Publisher"
echo "================================"
echo ""

if ! ./scripts/pre-release-checks.sh; then
    echo "❌ Pre-release checks failed."
    exit 1
fi

if [ -f .env ]; then
    set -a; source .env; set +a
fi

CURRENT_VERSION=$(grep -o '"version": "[^"]*"' src-tauri/tauri.conf.json | head -1 | cut -d'"' -f4)
echo "📦 Current version: v$CURRENT_VERSION"
echo ""

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"
NEXT_PATCH="$MAJOR.$MINOR.$((PATCH + 1))"
NEXT_MINOR="$MAJOR.$((MINOR + 1)).0"
NEXT_MAJOR="$((MAJOR + 1)).0.0"

echo "Select version bump:"
echo "  1) Patch  ($CURRENT_VERSION → $NEXT_PATCH)"
echo "  2) Minor  ($CURRENT_VERSION → $NEXT_MINOR)"
echo "  3) Major  ($CURRENT_VERSION → $NEXT_MAJOR)"
echo "  4) Custom"
echo "  5) Keep   (re-release $CURRENT_VERSION)"
echo ""
read -p "Choice [1-5]: " VERSION_CHOICE

case $VERSION_CHOICE in
    1) NEW_VERSION="$NEXT_PATCH" ;;
    2) NEW_VERSION="$NEXT_MINOR" ;;
    3) NEW_VERSION="$NEXT_MAJOR" ;;
    4) read -p "Version (e.g. 1.2.3): " NEW_VERSION ;;
    5) NEW_VERSION="$CURRENT_VERSION" ;;
    *) echo "Invalid"; exit 1 ;;
esac

read -p "Release notes (Enter for default): " RELEASE_NOTES
[ -z "$RELEASE_NOTES" ] && RELEASE_NOTES="Release v$NEW_VERSION"

echo ""
echo "Version: v$NEW_VERSION  |  Notes: $RELEASE_NOTES"
read -p "Continue? [y/N]: " CONFIRM
[[ ! "$CONFIRM" =~ ^[Yy]$ ]] && exit 0

# Bump versions
if [ "$NEW_VERSION" != "$CURRENT_VERSION" ]; then
    sed -i '' "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" src-tauri/tauri.conf.json
    sed -i '' "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" src-tauri/Cargo.toml
    sed -i '' "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" package.json
    echo "✅ Version bumped to $NEW_VERSION"
fi

# Clean + build macOS
cargo clean --manifest-path src-tauri/Cargo.toml
./build-release.sh

DMG_PATH=$(cat .build_dmg_path)
TAR_GZ_PATH=$(cat .build_tar_gz_path)

[ ! -f "$DMG_PATH" ] && echo "❌ DMG missing" && exit 1
[ ! -f "$TAR_GZ_PATH" ] && echo "❌ tar.gz missing" && exit 1
[ ! -f "latest.json" ] && echo "❌ latest.json missing" && exit 1

# Optional Windows build
WINDOWS_EXE_PATH=""
WINDOWS_SIGNATURE=""
read -p "🪟 Build Windows too? [y/N]: " BUILD_WINDOWS
if [[ "$BUILD_WINDOWS" =~ ^[Yy]$ ]]; then
    if ./build-windows.sh; then
        WINDOWS_EXE_PATH=$(cat .build_windows_exe_path)
        WINDOWS_SIGNATURE=$(cat .build_windows_signature 2>/dev/null || echo "")
    else
        echo "⚠️  Windows build failed — continuing with macOS only"
    fi
fi

# Inject Windows into latest.json if built
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
EOF
    echo "✅ Manifest updated with Windows platform"
fi

# GitHub release
if ! command -v gh &> /dev/null; then
    echo "⚠️  gh CLI not installed. Manual upload required."
    echo "   https://github.com/fstrauf/pageseeds-releases/releases/new"
    exit 0
fi

RELEASES_REPO="fstrauf/pageseeds-releases"

if gh release view "v$NEW_VERSION" --repo "$RELEASES_REPO" &>/dev/null; then
    read -p "Release v$NEW_VERSION exists. Delete and recreate? [y/N]: " DEL
    [[ "$DEL" =~ ^[Yy]$ ]] && gh release delete "v$NEW_VERSION" --repo "$RELEASES_REPO" --yes || exit 1
fi

# Version-agnostic copies for /latest/download/ URLs
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

gh release create "v$NEW_VERSION" \
    --repo "$RELEASES_REPO" \
    --title "PageSeeds v$NEW_VERSION" \
    --notes "$RELEASE_NOTES" \
    "${UPLOAD_FILES[@]}"

echo ""
echo "✅ Released: https://github.com/$RELEASES_REPO/releases/tag/v$NEW_VERSION"
```

---

## Step 11 — Create `scripts/pre-release-checks.sh`

```bash
#!/bin/bash
# Pre-release validation

set -e
ERRORS=0

echo "🔍 Pre-release checks..."

# Version consistency
V1=$(grep -o '"version": "[^"]*"' src-tauri/tauri.conf.json | head -1 | cut -d'"' -f4)
V2=$(grep -o '^version = "[^"]*"' src-tauri/Cargo.toml | cut -d'"' -f2)
V3=$(grep -o '"version": "[^"]*"' package.json | head -1 | cut -d'"' -f4)

if [ "$V1" != "$V2" ] || [ "$V1" != "$V3" ]; then
    echo "❌ Version mismatch: tauri.conf=$V1  Cargo=$V2  package.json=$V3"
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Versions consistent: $V1"
fi

# Signing key
KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY_PATH:-~/.tauri/pageseeds.key}"
KEY_PATH="${KEY_PATH/#\~/$HOME}"
if [ ! -f "$KEY_PATH" ]; then
    echo "❌ Signing key not found: $KEY_PATH"
    ERRORS=$((ERRORS + 1))
else
    echo "✅ Signing key found"
fi

# Signing key password
if [ -z "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" ]; then
    if [ -f .env ] && grep -q "TAURI_SIGNING_PRIVATE_KEY_PASSWORD" .env; then
        echo "✅ Signing password in .env"
    else
        echo "❌ TAURI_SIGNING_PRIVATE_KEY_PASSWORD not set"
        ERRORS=$((ERRORS + 1))
    fi
else
    echo "✅ Signing password in env"
fi

# Apple signing identity
if ! security find-identity -v -p codesigning 2>/dev/null | grep -q "KMJ36PKPW8"; then
    echo "⚠️  Apple Developer ID certificate not found in keychain (required for notarization)"
fi

# cargo check
echo "🔨 Running cargo check..."
if cargo check --manifest-path src-tauri/Cargo.toml --quiet 2>/dev/null; then
    echo "✅ cargo check passed"
else
    echo "❌ cargo check failed"
    ERRORS=$((ERRORS + 1))
fi

# TypeScript check
echo "🔨 Running tsc..."
if pnpm tsc --noEmit --quiet 2>/dev/null; then
    echo "✅ TypeScript check passed"
else
    echo "⚠️  TypeScript errors found (non-blocking)"
fi

if [ $ERRORS -gt 0 ]; then
    echo ""
    echo "❌ $ERRORS check(s) failed"
    exit 1
fi

echo ""
echo "✅ All checks passed"
```

---

## Step 12 — Create `.env.example`

```bash
# PageSeeds Desktop — Build Environment
# Copy to .env and fill in values. Never commit .env.

# Tauri update signing
TAURI_SIGNING_PRIVATE_KEY_PATH=~/.tauri/pageseeds.key
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=your_key_password_here

# Apple notarization (set up via `xcrun notarytool store-credentials`)
# These are only needed if Tauri is configured to auto-notarize.
# The signing identity in tauri.conf.json uses the keychain certificate directly.
APPLE_ID=your@apple.id
APPLE_PASSWORD=app-specific-password
APPLE_TEAM_ID=KMJ36PKPW8
```

---

## Step 13 — Create the Public GitHub Releases Repo

Create a **new public repository** named `pageseeds-releases` under the `fstrauf` GitHub account.

- **Purpose**: hosts release artifacts only — no source code ever goes here
- **Visibility**: public (required so unauthenticated users can download)
- **Initialize**: with a minimal `README.md`

The `gh release create` command in `publish-release.sh` targets `fstrauf/pageseeds-releases`.

The Tauri updater endpoint in `tauri.conf.json` must point to this repo's `latest.json`:
```
https://github.com/fstrauf/pageseeds-releases/releases/latest/download/latest.json
```

The download page download button URLs will use:
```
https://github.com/fstrauf/pageseeds-releases/releases/latest/download/PageSeeds_universal.dmg
https://github.com/fstrauf/pageseeds-releases/releases/latest/download/PageSeeds_x64-setup.exe
```

---

## Step 14 — Add an Update Check UI (Optional but Recommended)

Add an update check to the app's Settings or Help UI. The pattern from Expense Sorted:

**`src/lib/tauri.ts`** — add wrapper:
```typescript
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

export async function checkForUpdates() {
  const update = await check();
  return update; // null if up to date, or { version, body, downloadAndInstall() }
}

export async function installUpdateAndRelaunch(update: any) {
  await update.downloadAndInstall();
  await relaunch();
}
```

**`src/lib/types.ts`** — no new shared types needed; updater returns a typed object from the plugin.

---

## Step 15 — Download Page (Website)

This is a separate frontend task for the `pageseeds` website repo (not `pageseeds-app`). At minimum it needs:

- A `/download` route
- macOS + Windows download buttons pointing to the GitHub `/latest/download/` URLs
- The same constants pattern as Expense Sorted:
  ```typescript
  export const MAC_APP_DOWNLOAD_URL =
    "https://github.com/fstrauf/pageseeds-releases/releases/latest/download/PageSeeds_universal.dmg";
  export const WINDOWS_APP_DOWNLOAD_URL =
    "https://github.com/fstrauf/pageseeds-releases/releases/latest/download/PageSeeds_x64-setup.exe";
  ```

Write a separate spec for this page once the artifact URLs are confirmed working.

---

## Implementation Order

Execute in this sequence to avoid blocked steps:

1. **Version alignment** — fix `package.json` version to `0.1.0`
2. **Generate keypair** — `pnpm tauri signer generate -w ~/.tauri/pageseeds.key`
3. **`tauri.conf.json`** — add updater/signing/NSIS blocks (paste pubkey from step 2)
4. **`Entitlements.plist`** — create new file
5. **`Cargo.toml`** — add two plugin deps
6. **`package.json`** — add npm plugin packages + build scripts
7. **`lib.rs`** — register updater + process plugins
8. **`.env.example`** and **`.env`** — set up secrets
9. **`scripts/pre-release-checks.sh`** — create + `chmod +x`
10. **`build-release.sh`** — create + `chmod +x`
11. **`build-windows.sh`** — create + `chmod +x`
12. **`publish-release.sh`** — create + `chmod +x`
13. **Create `fstrauf/pageseeds-releases`** on GitHub
14. **Test build** — run `./build-release.sh` to verify end-to-end
15. **First release** — run `./publish-release.sh` → tag `v0.1.0`
16. **Download page** — separate task

---

## Checklist Before First Release

- [ ] `cargo check` passes
- [ ] `pnpm build` succeeds
- [ ] Signing key at `~/.tauri/pageseeds.key`
- [ ] `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in `.env`
- [ ] Apple Developer cert in keychain (`Developer ID Application: Florian Strauf`)
- [ ] `fstrauf/pageseeds-releases` repo exists and is public
- [ ] `gh auth status` shows authenticated
- [ ] Pubkey pasted into `tauri.conf.json` updater block
- [ ] All three files carry same version
- [ ] `./scripts/pre-release-checks.sh` exits 0
