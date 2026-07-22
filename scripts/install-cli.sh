#!/usr/bin/env bash
# Build and install pageseeds-cli for operator use outside the app repo.
#
# Installs a release binary to ~/.local/bin/pageseeds-cli (override with
# PREFIX). After install, weekly-seo and other agents should invoke
# pageseeds-cli from any cwd -- never cargo run from pageseeds-app.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="${ROOT}/src-tauri/Cargo.toml"
PREFIX="${PREFIX:-${HOME}/.local}"
BIN_DIR="${PREFIX}/bin"
BIN_NAME="pageseeds-cli"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found on PATH" >&2
  exit 1
fi

echo "Building release ${BIN_NAME}..."
cargo build --release --manifest-path "${MANIFEST}" --bin "${BIN_NAME}"

SRC="${ROOT}/src-tauri/target/release/${BIN_NAME}"
if [[ ! -x "${SRC}" ]]; then
  echo "error: expected binary not found at ${SRC}" >&2
  exit 1
fi

mkdir -p "${BIN_DIR}"
cp -f "${SRC}" "${BIN_DIR}/${BIN_NAME}"
chmod +x "${BIN_DIR}/${BIN_NAME}"

echo "Installed: ${BIN_DIR}/${BIN_NAME}"
if ! command -v "${BIN_NAME}" >/dev/null 2>&1; then
  echo "warning: ${BIN_DIR} is not on PATH -- add it, e.g.:" >&2
  echo "  export PATH=\"${BIN_DIR}:\$PATH\"" >&2
else
  echo "On PATH: $(command -v "${BIN_NAME}")"
fi

"${BIN_DIR}/${BIN_NAME}" --help >/dev/null
echo "OK -- ${BIN_NAME} is ready (use from any directory; do not open pageseeds-app for SEO ops)."
