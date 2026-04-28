# CTR Audit Slowness Investigation — Problem Statement

## Symptoms

The `ctr_audit` workflow in PageSeeds has become significantly slower between runs:

| Run | Context Build | Agent Step | Total |
|---|---|---|---|
| First run (04:06 UTC) | ~34s | ~11m | ~12m |
| Second run (06:10 UTC) | ~40s | ~39m | ~40m |
| Third run (incomplete) | ~43s | still running | — |

The deterministic `ctr_build_context` step is stable (~40s). The slowness is entirely in the agentic `ctr_analyze` step.

## Architecture

PageSeeds → `rig/provider.rs` → OpenAI-compatible HTTP call → **Kimi ACP Bridge** (`localhost:8080/v1`) → spawns `kimi acp` subprocess → Kimi API (`api.kimi.com/coding/v1`)

The bridge does **not** proxy to a remote HTTP API. It:
1. Accepts an OpenAI-compatible `/v1/chat/completions` request
2. Spawns a **fresh** `kimi acp` subprocess via `asyncio.create_subprocess_exec`
3. Translates OpenAI messages → ACP `session/prompt` protocol
4. Reads JSON-RPC line-by-line from subprocess stdout
5. Returns completion when subprocess finishes

## Key Finding: Model String Is a Fiction

- **PageSeeds advertises**: `kimi-k2.5` (hardcoded in `rig/provider.rs`)
- **Bridge validates**: `kimi-k2.5` (hardcoded in `server.py AVAILABLE_MODELS`)
- **Actual model used**: `kimi-for-coding` (K2.6) — from `~/.kimi/config.toml`

The bridge **never passes the model name to the `kimi` CLI**. It validates the request's `model` field against its hardcoded list, then delegates entirely to the subprocess. The subprocess uses its own config.

## Bridge Configuration

```toml
# ~/.kimi/config.toml
default_model = "kimi-code/kimi-for-coding"

[models."kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
model = "kimi-for-coding"
display_name = "Kimi-k2.6"

[providers."managed:kimi-code"]
type = "kimi"
base_url = "https://api.kimi.com/coding/v1"
```

Bridge defaults:
- `session_timeout = 300` seconds (`acp_client.py:217-220`)
- `session_mode = "ephemeral"` (fresh subprocess per request)
- No retry/backoff logic

## Uncommitted Changes in Bridge

The bridge repo (`/Users/fstrauf/01_code/kimi-acp-openai-bridge`) has **6 unstaged files**:

| File | Change |
|---|---|
| `acp_client.py` | Added `_build_prompt_text()` that folds full message history into single text block; added `enable_native_tools` flag; JSON buffering for multi-line ACP output; auto-approve permission requests |
| `models.py` | Added `ResponseFormat` model (`text` / `json_object` / `json_schema`) |
| `server.py` | Passes `response_format` to translator; handles `tool_choice: "none"`; wraps non-streaming path in `try/finally` |
| `translator.py` | Injects JSON schema constraints into system preamble (`inject_response_format_to_prompt()`) |
| `tests/` | Added tests for new response format and tool stripping behavior |

## Timing Discrepancy

From the second run:

```
App log:     05:31:46  executor ctr_build_context output
App log:     06:10:32  agent step completed
             → 38m 46s total gap

Bridge log:  17:05:17  acp_initialized (subprocess spawned)
Bridge log:  17:13:33  chat_completion_complete, duration_ms=496934
             → ~8m 17s actual API time
```

**Unaccounted time: ~30 minutes.**

Possible explanations:
1. Subprocess spawn / model loading overhead before `acp_initialized`
2. Multiple subprocesses / retries happening silently
3. Prompt construction (history folding + response format injection) is expensive
4. ACP session setup (preamble, tools, permissions) takes significant time
5. The subprocess stderr pipe is missing (`log_acp_messages = false`), so errors may be silently swallowed

## What Changed Recently

In PageSeeds:
- `task_spawner.rs`: removed `target_keyword` rejection (doesn't affect agent step)
- `models/ctr.rs`: added `#[serde(default)]` to `target_keyword` (doesn't affect agent step)
- `territory_research.rs`, `rig/compat/kimi.rs`, workflow handlers (unrelated to CTR audit)

In Bridge:
- The 6 unstaged files add structured output support and history folding
- These run **before** the API call, during request translation and ACP session setup

## Open Questions

1. **Is `_build_prompt_text()` constructing a massive prompt?** The CTR audit prompt is ~11,500 tokens. If history folding concatenates system + user + tool messages into a single block, does it duplicate content?

2. **Does `inject_response_format_to_prompt()` inject a large JSON schema into the system preamble?** The CTR skill expects structured JSON output. The translator may be injecting the full `CtrAgentOutput` schema into every request.

3. **Is there a subprocess leak?** With `session_mode = "ephemeral"`, every request spawns a new process. Are old processes being cleaned up properly? Is `client.close()` always called?

4. **Why did the first run take 11 minutes but the second took 39?** The bridge changes may not have been active during the first run, or the `kimi` CLI cached something.

5. **What does the `kimi` CLI do during the 30-minute gap before emitting tokens?** The bridge only sees what the CLI writes to stdout. If the CLI is doing internal retries, model warm-up, or OAuth refresh, the bridge has no visibility.

## Recommended Next Steps

1. **Add timing instrumentation to the bridge**:
   - Log timestamp when HTTP request arrives
   - Log timestamp when subprocess is spawned
   - Log timestamp when `acp_initialized` arrives
   - Log timestamp when first token / chunk arrives
   - Log timestamp when subprocess exits

2. **Profile the prompt size**:
   - Log the byte size of `preamble` and `acp_messages` before sending to ACP
   - Compare with the raw OpenAI request body size

3. **Check subprocess health**:
   - Ensure `client.close()` always kills the subprocess
   - Check if zombie `kimi acp` processes accumulate during a long run

4. **Verify the `kimi` CLI version**:
   - Current: `kimi-cli 1.39.0`
   - Check if a newer version has ACP performance fixes
   - Check if `kimi-for-coding` (K2.6) has known latency issues vs older models

5. **Consider making the bridge accept `kimi-k2.6`**:
   - Update `AVAILABLE_MODELS` in `server.py`
   - Update PageSeeds `rig/provider.rs` to emit `kimi-k2.6`
   - This is cosmetic but reduces confusion

## Follow-up Investigation: 2026-04-28

### What was checked

- PageSeeds CTR path:
   - `ctr_analyze` builds one plain prompt from the CTR skill plus context JSON.
   - It calls `engine::agent::run_agent()`, which resolves the Kimi bridge backend and sends one non-streaming OpenAI-compatible HTTP request.
   - This path does **not** use Rig `Extractor<T>`, `response_format`, or structured tool extraction.
   - The PageSeeds Kimi HTTP adapter uses `reqwest::Client::new()` with no request timeout and no retry loop for this plain prompt path.
- Bridge path:
   - Existing bridge tests initially had one failure: streaming startup errors leaked as HTTP 500 instead of service-unavailable style failure.
   - After wrapping subprocess startup/write failures as `RuntimeError`, all bridge tests pass.
   - Added phase timing and size logs to the bridge source so future runs can split latency by request translation, Kimi process spawn/init, ACP session creation, prompt send, first ACP event, first content chunk, and prompt completion.

### Local timing observations

Using the currently installed Kimi CLI (`kimi, version 1.39.0`):

| Probe | Result |
|---|---:|
| Existing bridge on `localhost:8080`, non-streaming tiny prompt | ~10.57s total |
| Existing bridge on `localhost:8080`, streaming tiny prompt | ~3.03s first byte, ~7.49s total |
| Instrumented source bridge on `localhost:9090`, non-streaming tiny prompt | ~6.70s total |

Instrumented phase logs for the tiny prompt on the source bridge:

| Phase | Time |
|---|---:|
| OpenAI -> ACP translation | 0.01ms |
| Spawn `kimi acp` subprocess | 1.75ms |
| ACP initialize complete | ~1.10s |
| ACP `session/new` | ~0.74s |
| Prompt send | 0.05ms |
| Prompt completion | ~4.85s |
| Total HTTP request | ~6.69s |

This strongly suggests bridge-side Python translation/prompt construction is not the dominant cost. The fixed overhead is mostly Kimi ACP initialization/session setup plus the model/API response time behind the CLI.

### Updated assessment

- The recent bridge `response_format` injection is probably **not** responsible for the CTR audit slowdown, because the PageSeeds `ctr_analyze` path does not send `response_format` or tools.
- The bridge `_build_prompt_text()` history folding is also unlikely to explain the slowdown for CTR audit, because PageSeeds sends a single user message for this path.
- PageSeeds does not appear to add a 30-minute retry loop around the plain Kimi prompt. It does, however, have no request timeout, so it will wait indefinitely if the bridge/Kimi CLI/API stalls.
- If a bridge request log shows only ~8 minutes while the PageSeeds step shows ~39 minutes, the most likely explanations are:
   1. the compared app and bridge log entries are not the same request/run;
   2. clock/timezone alignment is misleading;
   3. PageSeeds waited before/after the actual HTTP request in a path not visible in the current logs;
   4. a different backend/path was used for part of the run.

### New evidence to collect on the next slow run

With the bridge instrumentation in place, compare one `request_id` across these events:

- `chat_completion_request`
- `chat_completion_translated`
- `kimi_process_spawned`
- `acp_initialized`
- `session_created`
- `acp_prompt_prepared`
- `acp_first_event_received`
- `acp_first_content_chunk`
- `acp_prompt_finished`
- `chat_completion_complete`

If `acp_prompt_finished` accounts for nearly all runtime, the slow part is Kimi CLI/API/model behavior. If there is a large gap before `chat_completion_request`, the issue is in PageSeeds before the HTTP request. If `chat_completion_complete` is fast but PageSeeds remains slow, instrument PageSeeds around `send_request()` and `exec_ctr_analyze()` to catch post-response parsing or workflow persistence delays.

## Working Conclusion: ACP Prompt Size Is the Failure Boundary

The latest bridge/API-side run narrows the issue enough to choose a product fix:

- The bridge received the CTR request and launched Kimi with `--no-thinking acp` correctly.
- The Kimi ACP child accepted the prompt and emitted ACP session events, but never emitted `acp_first_content_chunk`.
- The child then sat mostly idle until killed. After that, the bridge completed with empty content, which PageSeeds surfaced downstream as `No ctr_recommendations artifact found`.
- The bridge now treats this empty-response case as a 503, which is the right behavior. It prevents PageSeeds from interpreting a dead ACP generation as a successful empty assistant message.
- A tiny prompt succeeds through the same bridge in a few seconds, so the bridge is healthy enough for small requests.
- `--no-thinking` does not fix the CTR-size stall. Streaming also would not fix this specific failure, because the model never reaches first content.

That points to the Kimi ACP code path itself, not Rig as an abstraction and not the Python bridge translation layer. PageSeeds is sending one large CTR prompt through ACP: about 46 KB / 11.5K estimated tokens for the failing run. The current code caps by article count (`top_20_by_clicks_lost`) but not by byte or token budget, so a "top 20" audit can still become one oversized ACP request once rendered audit data, excerpts, GSC metrics, and query data are included.

The exact black-box failure is inside `kimi acp` / the Kimi backend after `session/prompt` is accepted. The actionable PageSeeds root cause is simpler: `ctr_analyze` packages too much audit context into one ACP turn.

## Hypotheses Ruled Out

| Hypothesis | Current read |
|---|---|
| Rig itself is causing the stall | Unlikely. The CTR path uses the local strict Kimi compatibility adapter and sends one plain OpenAI-compatible chat request. Rig is only the provider boundary here. |
| Bridge `response_format` schema injection bloated the prompt | Unlikely for CTR. `ctr_analyze` does not currently use Rig `Extractor<T>`, tools, or `response_format`. |
| Bridge history folding duplicates CTR context | Unlikely for CTR. PageSeeds sends a single user message, so `_build_prompt_text()` wraps it as one `User:` block rather than replaying a large conversation. |
| `--no-thinking` solves it | Ruled out by the retry: ACP still stalled before first content. |
| Streaming solves it | No. Streaming helps visibility and time-to-first-token only after generation starts; this failure never emits a first content chunk. |
| The bridge should return success with empty content | Ruled out and fixed. Empty Kimi ACP output should be a 503/error, not a successful assistant response. |

## Best Path Forward

Keep the Rig/bridge architecture, but change CTR analysis from one large request into deterministic micro-batches.

The low-risk implementation is inside `exec_ctr_analyze`, without changing the workflow graph:

1. Parse the existing `context_json`.
2. Read `top_20_by_clicks_lost` in priority order.
3. Split articles into batches by serialized prompt size, not only by count.
4. Send each batch through the same provider path.
5. Parse each batch response as JSON.
6. Merge all batch `recommendations` into one final `CtrAgentOutput` artifact.
7. Fail the step if any batch returns invalid JSON or a provider error, rather than silently producing a partial audit.

Recommended first thresholds:

| Setting | Value |
|---|---:|
| Target full prompt size per batch | 10-15 KB |
| Hard max full prompt size per batch | 18-20 KB |
| Default max articles per batch | 4 |
| Fallback if still slow | 3 articles per batch |
| Do not exceed without fresh evidence | 5 articles per batch |

For the failing run, this turns one 46 KB request into roughly five requests of four articles each. That keeps the output contract the same for downstream task spawning while avoiding the observed ACP stall zone.

## Suggested Code Shape

Minimal PageSeeds patch:

- Add a small `CtrAnalyzeBatch` helper in `src-tauri/src/engine/exec/ctr_audit/analyze.rs` or a sibling module.
- Add constants such as:
   - `CTR_ANALYZE_TARGET_PROMPT_BYTES: usize = 15_000`
   - `CTR_ANALYZE_HARD_PROMPT_BYTES: usize = 20_000`
   - `CTR_ANALYZE_MAX_ARTICLES_PER_BATCH: usize = 4`
- Build batch context documents with the same top-level metadata plus a sliced `top_20_by_clicks_lost` array.
- Include batch metadata in the prompt: `batch_index`, `batch_count`, and `article_count`.
- Log each batch's prompt bytes and article IDs before the provider call.
- Parse each response through `extract_json`, then deserialize into `CtrAgentOutput`.
- Merge by preserving input priority order; optionally dedupe by `(article_id, fix_type)` if a future overlapping batch strategy is added.

Important detail: do not switch CTR to schema/tool extraction as the first fix. That may be a good later cleanup, but it would add tool/schema overhead to the same ACP path we are trying to stabilize. First make the plain prompt smaller and reliable, then consider typed extraction/validation once the transport is boring again.

## Secondary Guardrail

PageSeeds should also add a request timeout for the Kimi bridge HTTP client. The current `reqwest::Client::new()` in `rig/compat/kimi.rs` has no timeout for plain prompt calls, so PageSeeds can wait indefinitely if ACP stalls.

A conservative guardrail is:

- `connect_timeout`: 10 seconds
- total request timeout: slightly above bridge `KIMI_BRIDGE_SESSION_TIMEOUT`, or a configurable default such as 330 seconds

This timeout should be treated as a failure signal, not a recovery mechanism. The real reliability fix is still batching. The timeout just prevents another unbounded hung task.

## Role of Direct `kimi --print`

The old `kimi --print --no-thinking --final-message-only` path remains useful as an A/B test and emergency escape hatch, because it is a different Kimi CLI code path and has already worked for PageSeeds tasks.

It should not be the primary fix if the goal is to keep a unified Rig/provider architecture. The clean approach is:

1. Default CTR to batched Rig/bridge calls.
2. Keep `kimi_backend_mode = "direct"` available for manual fallback.
3. Consider automatic direct fallback only for explicit bridge failures such as 503/timeout, and only if the user accepts that token usage/structured provider behavior will differ from the Rig path.

## Validation Plan

After implementing batching:

1. Unit-test the chunking helper with synthetic article records, including one oversized article.
2. Unit-test merge behavior to ensure the final artifact still matches `CtrAgentOutput`.
3. Run `cargo test test_exec_ctr_build_context_preserves_resolved_issues_on_healthy_rerun` to protect the existing healthy/unchanged skip behavior.
4. Run `cargo test --manifest-path src-tauri/Cargo.toml` if time allows.
5. Live-test CTR audit through the bridge and compare these log fields per batch:
    - PageSeeds prompt bytes
    - bridge `acp_prompt_prepared.prompt_bytes`
    - `acp_first_content_chunk.duration_ms`
    - `chat_completion_complete.duration_ms`
6. If any 4-article batch still has no first content within 60-90 seconds, reduce to 3 articles or lower the byte budget before changing bridge internals.

## Decision

Proceed with CTR byte-budget batching plus a Kimi bridge HTTP timeout in PageSeeds. Keep the bridge 503 empty-response patch and instrumentation. Do not spend more time on streaming or `--no-thinking` for this issue unless new evidence shows Kimi ACP reaches first content and then stalls mid-generation.
