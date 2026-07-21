#!/usr/bin/env bash
# Run the live LLM eval regression suites (rig::evals).
#
# Evals call real providers, so they are #[ignore]d in the normal test run.
# This script is part of `pnpm test:all` — it SKIPS (exit 0) when no generation
# provider is available on the machine, so CI/dev machines without credentials
# stay green. Set EVALS_REQUIRED=1 to turn a skip into a failure.
#
# Env:
#   EVAL_PROVIDER        generation backend: kimi (default, CLI connector) | claude | openai | ollama
#   EVAL_JUDGE_PROVIDER  judge provider: claude | openai (auto-detected from API keys when unset)
#   ANTHROPIC_API_KEY / OPENAI_API_KEY — required for the LLM judge (deterministic
#                        contract checks still run and gate without them)
#   EVALS_REQUIRED=1     fail (instead of skip) when no provider is available
#
# Usage: ./scripts/run-evals.sh [cargo-test-filter]   (default filter: "evals")
set -euo pipefail

FILTER="${1:-evals}"
PROVIDER="${EVAL_PROVIDER:-kimi}"

skip() {
  local reason="$1"
  if [ "${EVALS_REQUIRED:-0}" = "1" ]; then
    echo "ERROR: live evals required but unavailable: ${reason}" >&2
    exit 1
  fi
  echo "== Live LLM eval suites: SKIPPED (${reason}) =="
  exit 0
}

# Provider availability check — keep in sync with rig::provider::resolve_backend.
case "${PROVIDER}" in
  kimi)
    command -v kimi >/dev/null 2>&1 || skip "EVAL_PROVIDER=kimi but 'kimi' CLI not on PATH"
    ;;
  claude)
    [ -n "${ANTHROPIC_API_KEY:-}" ] || skip "EVAL_PROVIDER=claude but ANTHROPIC_API_KEY unset"
    ;;
  openai)
    [ -n "${OPENAI_API_KEY:-}" ] || skip "EVAL_PROVIDER=openai but OPENAI_API_KEY unset"
    ;;
  ollama)
    : # local server assumed; the eval tests fail with a clear message if unreachable
    ;;
  *)
    skip "unknown EVAL_PROVIDER=${PROVIDER}"
    ;;
esac

echo "== Live LLM eval suites (filter: ${FILTER}) =="
echo "   EVAL_PROVIDER=${PROVIDER}"
echo "   EVAL_JUDGE_PROVIDER=${EVAL_JUDGE_PROVIDER:-auto (ANTHROPIC_API_KEY/OPENAI_API_KEY)}"
echo

cargo test --manifest-path src-tauri/Cargo.toml "${FILTER}" -- --ignored --nocapture
