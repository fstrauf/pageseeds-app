#!/usr/bin/env bash
# Smoke-check pageseeds-cli machine contract (exit codes, streams, help inventory).
# See CONTRACTS.md §14 and issue #159.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="${ROOT}/src-tauri/Cargo.toml"

resolve_bin() {
  if [[ -n "${PAGESEEDS_CLI:-}" && -x "${PAGESEEDS_CLI}" ]]; then
    echo "${PAGESEEDS_CLI}"
    return
  fi
  local debug="${ROOT}/src-tauri/target/debug/pageseeds-cli"
  local release="${ROOT}/src-tauri/target/release/pageseeds-cli"
  if [[ -x "${debug}" ]]; then
    echo "${debug}"
    return
  fi
  if [[ -x "${release}" ]]; then
    echo "${release}"
    return
  fi
  if command -v pageseeds-cli >/dev/null 2>&1; then
    command -v pageseeds-cli
    return
  fi
  echo "Building debug pageseeds-cli..." >&2
  cargo build --manifest-path "${MANIFEST}" --bin pageseeds-cli >&2
  if [[ ! -x "${debug}" ]]; then
    echo "error: failed to build pageseeds-cli at ${debug}" >&2
    exit 1
  fi
  echo "${debug}"
}

BIN="$(resolve_bin)"
echo "Using BIN=${BIN}"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# 1) Top-level --help → exit 0
set +e
HELP_OUT="$("${BIN}" --help 2>/tmp/cli-contract-help-err)"
HELP_EC=$?
set -e
[[ "${HELP_EC}" -eq 0 ]] || fail "--help exit ${HELP_EC}, expected 0"
[[ -z "$(cat /tmp/cli-contract-help-err 2>/dev/null || true)" ]] || true
# Prefer stdout for help; allow stderr only if stdout empty (should not happen after #159)
if [[ -z "${HELP_OUT}" ]]; then
  HELP_OUT="$(cat /tmp/cli-contract-help-err 2>/dev/null || true)"
fi
[[ -n "${HELP_OUT}" ]] || fail "--help produced no help text"

# 2) unknown tool → exit 1, stdout empty, stderr contains ERROR:
set +e
UNK_OUT="$("${BIN}" not-a-tool 2>/tmp/cli-contract-unk-err)"
UNK_EC=$?
set -e
UNK_ERR="$(cat /tmp/cli-contract-unk-err 2>/dev/null || true)"
[[ "${UNK_EC}" -eq 1 ]] || fail "unknown tool exit ${UNK_EC}, expected 1"
[[ -z "${UNK_OUT}" ]] || fail "unknown tool wrote to stdout (len ${#UNK_OUT}); expected empty"
echo "${UNK_ERR}" | grep -q 'ERROR:' || fail "unknown tool stderr missing ERROR: (got: ${UNK_ERR})"

# 3) <tool> --help → exit 0 (not --project-id required)
set +e
TOOL_HELP_OUT="$("${BIN}" research-pull --help 2>/tmp/cli-contract-tool-help-err)"
TOOL_HELP_EC=$?
set -e
TOOL_HELP_ERR="$(cat /tmp/cli-contract-tool-help-err 2>/dev/null || true)"
[[ "${TOOL_HELP_EC}" -eq 0 ]] || fail "research-pull --help exit ${TOOL_HELP_EC}, expected 0"
if echo "${TOOL_HELP_OUT}${TOOL_HELP_ERR}" | grep -qi 'project-id required'; then
  fail "research-pull --help still requires --project-id"
fi
if [[ -z "${TOOL_HELP_OUT}" ]]; then
  TOOL_HELP_OUT="${TOOL_HELP_ERR}"
fi
[[ -n "${TOOL_HELP_OUT}" ]] || fail "research-pull --help produced no text"

# 4) help inventory must list key tools
for needle in research-context research-pull create-reddit-replies write-submit fix-context; do
  echo "${HELP_OUT}" | grep -q "${needle}" || fail "help text missing tool: ${needle}"
done

# 5) bare `help` → exit 0
set +e
"${BIN}" help >/dev/null 2>/tmp/cli-contract-bare-help-err
BARE_EC=$?
set -e
[[ "${BARE_EC}" -eq 0 ]] || fail "help exit ${BARE_EC}, expected 0"

# 6) machine contract blurb present
echo "${HELP_OUT}" | grep -qi 'Machine contract' || fail "help header missing Machine contract blurb"
echo "${HELP_OUT}" | grep -qi 'Semver' || fail "help footer missing Semver note"

echo "OK — pageseeds-cli machine contract smoke passed"
