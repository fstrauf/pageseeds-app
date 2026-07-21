#!/usr/bin/env bash
# Run the live LLM eval regression suites (rig::evals).
#
# Evals call real providers, so they are #[ignore]d in the normal test run.
#
# Env:
#   EVAL_PROVIDER        generation backend: kimi (default, CLI connector) | claude | openai | ollama
#   EVAL_JUDGE_PROVIDER  judge provider: claude | openai (auto-detected from API keys when unset)
#   ANTHROPIC_API_KEY / OPENAI_API_KEY — required for the LLM judge (deterministic
#                        contract checks still run and gate without them)
#
# Usage: ./scripts/run-evals.sh [cargo-test-filter]   (default filter: "evals")
set -euo pipefail

FILTER="${1:-evals}"

echo "== Live LLM eval suites (filter: ${FILTER}) =="
echo "   EVAL_PROVIDER=${EVAL_PROVIDER:-kimi (default)}"
echo "   EVAL_JUDGE_PROVIDER=${EVAL_JUDGE_PROVIDER:-auto (ANTHROPIC_API_KEY/OPENAI_API_KEY)}"
echo

cargo test --manifest-path src-tauri/Cargo.toml "${FILTER}" -- --ignored --nocapture
