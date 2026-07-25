#!/usr/bin/env bash
# Install pageseeds-cli for operator use outside the app repo.
#
# Customer (default — no checkout, no cargo):
#   curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
#
# Dev / contributor (from pageseeds-app checkout):
#   ./scripts/install-cli.sh              # download first (Darwin/arm64), cargo fallback
#   FROM_SOURCE=1 ./scripts/install-cli.sh  # force cargo build
#
# Installs to ${PREFIX:-$HOME/.local}/bin/pageseeds-cli
# Optional: VERSION=0.1.0 to pin a release (without cli-v prefix).
set -euo pipefail

REPO="fstrauf/pageseeds-app"
BIN_NAME="pageseeds-cli"
PREFIX="${PREFIX:-${HOME}/.local}"
BIN_DIR="${PREFIX}/bin"
TARGET_BIN="${BIN_DIR}/${BIN_NAME}"
FROM_SOURCE="${FROM_SOURCE:-0}"
ARCH_TRIPLE="aarch64-apple-darwin"

# ── helpers ──────────────────────────────────────────────────────────────

die() { echo "error: $*" >&2; exit 1; }
info() { echo "$*"; }

# Detect whether this script is running from a real file under a pageseeds-app
# checkout that has a workspace Cargo.toml (or legacy src-tauri/Cargo.toml).
# When piped via curl | bash, $0 is "bash" (or similar) and there is no monorepo root.
detect_checkout_root() {
  local script_path="${BASH_SOURCE[0]:-$0}"
  # Piped execution: $0 is often "bash" / "-bash" / "sh"
  if [[ "${script_path}" == "bash" || "${script_path}" == "-bash" \
     || "${script_path}" == "sh" || "${script_path}" == "-sh" \
     || "${script_path}" == "/dev/stdin" || "${script_path}" == "-" ]]; then
    return 1
  fi
  # Relative path without a real file (e.g. process substitution)
  if [[ ! -f "${script_path}" ]]; then
    return 1
  fi
  local root
  root="$(cd "$(dirname "${script_path}")/.." && pwd 2>/dev/null)" || return 1
  if [[ -f "${root}/Cargo.toml" ]] && grep -q 'pageseeds-cli' "${root}/Cargo.toml" 2>/dev/null; then
    echo "${root}"
    return 0
  fi
  # Legacy monorepo layout (pre-#183)
  if [[ -f "${root}/src-tauri/Cargo.toml" ]]; then
    echo "${root}"
    return 0
  fi
  return 1
}

is_darwin_arm64() {
  [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]
}

has_cargo_fallback() {
  [[ -n "${CHECKOUT_ROOT}" ]] && command -v cargo >/dev/null 2>&1
}

die_platform_unsupported() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  cat >&2 <<EOF
error: prebuilt pageseeds-cli is currently only available for macOS Apple Silicon (Darwin/arm64).
  detected: ${os}/${arch}
  options:
    - build from source on a pageseeds-app checkout:
        FROM_SOURCE=1 ./scripts/install-cli.sh
      (requires Rust/cargo)
    - other platforms are not yet supported
EOF
  exit 1
}

resolve_version() {
  # Prefer explicit VERSION env (strip optional cli-v / v prefix).
  if [[ -n "${VERSION:-}" ]]; then
    local v="${VERSION}"
    v="${v#cli-v}"
    v="${v#v}"
    echo "${v}"
    return 0
  fi

  # Latest GitHub release whose tag starts with cli-v
  local api_json
  if ! api_json="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases" 2>/dev/null)"; then
    return 1
  fi
  # Prefer python3 for robust JSON; fall back to grep/sed.
  local tag=""
  if command -v python3 >/dev/null 2>&1; then
    tag="$(printf '%s' "${api_json}" | python3 -c '
import json, sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)
for rel in data:
    t = rel.get("tag_name") or ""
    if t.startswith("cli-v") and not rel.get("draft") and not rel.get("prerelease"):
        print(t)
        sys.exit(0)
sys.exit(1)
' 2>/dev/null)" || true
  fi
  if [[ -z "${tag}" ]]; then
    tag="$(printf '%s' "${api_json}" \
      | grep -oE '"tag_name"[[:space:]]*:[[:space:]]*"cli-v[^"]+"' \
      | head -n1 \
      | sed -E 's/.*"cli-v([^"]+)".*/cli-v\1/' || true)"
  fi
  if [[ -z "${tag}" || "${tag}" != cli-v* ]]; then
    return 1
  fi
  echo "${tag#cli-v}"
}

install_from_download() {
  local version="$1"
  local asset="pageseeds-cli-${version}-${ARCH_TRIPLE}.tar.gz"
  local url="https://github.com/${REPO}/releases/download/cli-v${version}/${asset}"
  local sha_url="${url}.sha256"
  local tmp
  tmp="$(mktemp -d)"

  info "Downloading ${url} ..."
  if ! curl -fsSL -o "${tmp}/${asset}" "${url}"; then
    rm -rf "${tmp}"
    return 1
  fi

  # Checksum is part of the release contract — fail closed if missing or wrong.
  info "Downloading checksum ${sha_url} ..."
  if ! curl -fsSL -o "${tmp}/${asset}.sha256" "${sha_url}"; then
    rm -rf "${tmp}"
    die "failed to download checksum ${asset}.sha256 (refusing to install unverified binary)"
  fi

  info "Verifying checksum ..."
  if ! (cd "${tmp}" && shasum -a 256 -c "${asset}.sha256"); then
    rm -rf "${tmp}"
    die "checksum verification failed for ${asset} (refusing to install)"
  fi

  if ! tar -xzf "${tmp}/${asset}" -C "${tmp}"; then
    rm -rf "${tmp}"
    return 1
  fi
  if [[ ! -f "${tmp}/${BIN_NAME}" ]]; then
    rm -rf "${tmp}"
    die "tarball did not contain ${BIN_NAME}"
  fi

  mkdir -p "${BIN_DIR}"
  cp -f "${tmp}/${BIN_NAME}" "${TARGET_BIN}"
  chmod +x "${TARGET_BIN}"
  rm -rf "${tmp}"
  return 0
}

install_from_source() {
  local root="$1"
  local manifest=""
  local src=""

  if [[ -f "${root}/Cargo.toml" ]] && grep -q 'pageseeds-cli' "${root}/Cargo.toml" 2>/dev/null; then
    # Workspace layout (#183+): crates/pageseeds-cli
    manifest="${root}/Cargo.toml"
    src="${root}/target/release/${BIN_NAME}"
  elif [[ -f "${root}/src-tauri/Cargo.toml" ]]; then
    # Legacy single-crate layout
    manifest="${root}/src-tauri/Cargo.toml"
    src="${root}/src-tauri/target/release/${BIN_NAME}"
  else
    die "Cargo.toml not found (expected workspace root or src-tauri/Cargo.toml under ${root})"
  fi

  if ! command -v cargo >/dev/null 2>&1; then
    die "cargo not found on PATH (required for FROM_SOURCE / source install)"
  fi

  info "Building release ${BIN_NAME} from source..."
  if [[ "${manifest}" == "${root}/Cargo.toml" ]]; then
    cargo build --release --manifest-path "${manifest}" -p pageseeds-cli
  else
    cargo build --release --manifest-path "${manifest}" --bin "${BIN_NAME}"
  fi

  [[ -x "${src}" ]] || die "expected binary not found at ${src}"

  mkdir -p "${BIN_DIR}"
  cp -f "${src}" "${TARGET_BIN}"
  chmod +x "${TARGET_BIN}"
}

verify_install() {
  [[ -x "${TARGET_BIN}" ]] || die "installed binary not executable: ${TARGET_BIN}"
  local ver
  ver="$("${TARGET_BIN}" --version 2>/dev/null || true)"
  if [[ -z "${ver}" ]]; then
    die "${BIN_NAME} --version failed or produced empty output"
  fi
  # Help is stdout after #159; accept either stream for older binaries.
  if ! "${TARGET_BIN}" --help >/dev/null 2>&1; then
    die "${BIN_NAME} --help failed"
  fi
  info "Verified: ${TARGET_BIN} --version → ${ver}"

  # Machine-contract smoke (exit codes, help inventory) when installing from a
  # monorepo checkout that ships scripts/check-cli-contract.sh. Skipped for
  # curl | bash customer installs where that script is not on disk.
  if [[ -n "${CHECKOUT_ROOT:-}" && -f "${CHECKOUT_ROOT}/scripts/check-cli-contract.sh" ]]; then
    info "Running CLI machine-contract smoke..."
    PAGESEEDS_CLI="${TARGET_BIN}" bash "${CHECKOUT_ROOT}/scripts/check-cli-contract.sh" \
      || die "CLI machine-contract smoke failed"
  fi
}

print_path_warning() {
  if ! command -v "${BIN_NAME}" >/dev/null 2>&1; then
    echo "warning: ${BIN_DIR} is not on PATH -- add it, e.g.:" >&2
    echo "  export PATH=\"${BIN_DIR}:\$PATH\"" >&2
  else
    info "On PATH: $(command -v "${BIN_NAME}")"
  fi
}

print_gatekeeper_note() {
  cat <<EOF

Note (macOS Gatekeeper): unsigned downloads may be quarantined. If launch is
blocked, clear quarantine once:
  xattr -d com.apple.quarantine ${TARGET_BIN}
EOF
}

# Shared success epilogue. Pass "gatekeeper" to print the macOS quarantine note.
complete_install() {
  local mode="${1:-}"
  info "Installed: ${TARGET_BIN}"
  print_path_warning
  if [[ "${mode}" == "gatekeeper" ]]; then
    print_gatekeeper_note
  fi
  verify_install
  info "OK -- ${BIN_NAME} is ready (use from any directory; do not open pageseeds-app for SEO ops)."
  echo ""
  echo "Next step: in your customer project repo, run:"
  echo "  ${BIN_NAME} setup --path . --yes"
  echo "Then desk tools work without -i/-p, e.g.:"
  echo "  ${BIN_NAME} site-overview"
  exit 0
}

# ── main ─────────────────────────────────────────────────────────────────

CHECKOUT_ROOT=""
if CHECKOUT_ROOT="$(detect_checkout_root)"; then
  :
else
  CHECKOUT_ROOT=""
fi

# Force source build (any platform with cargo; no prebuilt matrix check)
if [[ "${FROM_SOURCE}" == "1" ]]; then
  [[ -n "${CHECKOUT_ROOT}" ]] || die "FROM_SOURCE=1 requires running from a pageseeds-app checkout (workspace Cargo.toml or src-tauri/Cargo.toml)"
  install_from_source "${CHECKOUT_ROOT}"
  complete_install
fi

# Prebuilt download path (Darwin/arm64 only)
if is_darwin_arm64; then
  VERSION_RESOLVED=""
  if VERSION_RESOLVED="$(resolve_version)"; then
    info "Resolved version: ${VERSION_RESOLVED}"
    if install_from_download "${VERSION_RESOLVED}"; then
      complete_install gatekeeper
    fi
    # Tarball download/extract failed — cargo fallback when available
    if has_cargo_fallback; then
      info "warning: download failed; falling back to cargo build" >&2
      install_from_source "${CHECKOUT_ROOT}"
      complete_install
    fi
    die "download failed and no cargo fallback available. Retry later, set VERSION=..., or clone pageseeds-app and run FROM_SOURCE=1 ./scripts/install-cli.sh"
  fi

  # Could not resolve a cli-v* release
  if has_cargo_fallback; then
    info "warning: could not resolve a cli-v* GitHub release; falling back to cargo build" >&2
    install_from_source "${CHECKOUT_ROOT}"
    complete_install
  fi
  die "could not resolve version (set VERSION=... or publish a cli-v* release). From a checkout you can use FROM_SOURCE=1."
fi

# Non-Darwin/arm64 (or Darwin non-arm64): fall through to cargo when checkout exists
if has_cargo_fallback; then
  info "No prebuilt binary for $(uname -s)/$(uname -m); building from source..."
  install_from_source "${CHECKOUT_ROOT}"
  complete_install
fi

die_platform_unsupported

