# PageSeeds Kimi Bridge Adoption Spec

Date: 2026-05-01

## Purpose

This spec defines the PageSeeds-side work needed to take advantage of the improved Kimi ACP OpenAI bridge.

The bridge-side provider contract and verification notes live separately in `docs/kimi-bridge-provider-spec.md` and `docs/kimi-bridge-integration-spec.md`. This document is only about changes inside PageSeeds.

The short version: PageSeeds should continue to work with the new bridge without an emergency application change. The useful local work is cleanup and hardening: consume bridge capabilities, parse structured bridge errors, make backend routing explicit, and preflight prompt sizes before sending large agent tasks.

## Compatibility Assessment

PageSeeds can keep its current Kimi path temporarily:

- `rig/provider.rs` still resolves Kimi bridge through `KIMI_BRIDGE_URL` or `http://localhost:8080/v1`.
- `check_bridge_health()` only requires `kimi_available`, which the new bridge still returns.
- `rig/compat/kimi.rs` sends strict string-content requests, which the bridge still accepts.
- `X-Kimi-Backend` is still sent for generic agentic calls through the current custom adapter.
- Existing direct fallback through `agent-wrapper` still exists.

No immediate PageSeeds runtime change is required for basic compatibility.

However, PageSeeds is not yet taking advantage of the new bridge contract:

- Health capability data is ignored.
- Bridge error bodies are treated as opaque strings.
- Prompt limits are only enforced bridge-side, after the prompt has already been built and sent.
- The Kimi compatibility adapter still exists mainly for historical serializer issues and custom `X-Kimi-Backend` routing.
- The `kimi_backend_mode = auto` comment says fallback to direct CLI, but current behavior fails clearly if bridge health is down.

## Feature Scope

### Phase 1: Model Bridge Health As A First-Class Capability

Add typed Rust models for bridge health:

```rust
struct KimiBridgeHealth {
    status: String,
    kimi_available: bool,
    bridge_version: String,
    kimi_cli_version: Option<String>,
    models: Vec<String>,
    backends: HashMap<String, KimiBackendCapabilities>,
    limits: KimiBridgeLimits,
}

struct KimiBackendCapabilities {
    available: bool,
    tool_calls: bool,
    json_mode: bool,
    file_io: bool,
}

struct KimiBridgeLimits {
    max_prompt_bytes_direct: usize,
    max_prompt_bytes_acp: usize,
    max_concurrent_requests: usize,
}
```

Implementation notes:

- Add `get_kimi_bridge_health(base_url)` in `rig/provider.rs` or a small `rig/kimi_bridge.rs` module.
- Keep `check_bridge_health(base_url) -> bool` as a compatibility wrapper around the typed health check.
- Update logs to include bridge version, Kimi CLI version, selected backend availability, and prompt limits.
- Optionally surface this in Settings later; backend support is enough for the first pass.

### Phase 2: Parse Structured Bridge Errors

Add a typed bridge error parser in `rig/compat/kimi.rs`.

Desired behavior:

- Parse `error.code`, `error.message`, `error.retryable`, `error.backend`, `error.request_id`, `error.phase`, and `error.details` from bridge error bodies.
- Include the bridge request id in the PageSeeds step error message.
- Treat `413 prompt_too_large` and `422 tools_not_supported` as non-retryable.
- Retry only bridge errors that are explicitly retryable or status `429`, `503`, or `504` where retrying is useful.
- Preserve clear setup errors for `kimi_not_found`, `bridge_unhealthy`, and `backend_unavailable`.

This replaces opaque messages like:

```text
Kimi API error 503 Service Unavailable: {...}
```

With messages like:

```text
Kimi bridge request chatcmpl-... failed: prompt_too_large for direct backend (23,412 bytes, limit 20,000). Split or trim the prompt before retrying.
```

### Phase 3: Make Backend Routing Explicit

Keep `X-Kimi-Backend` for now. The custom PageSeeds Kimi adapter is still useful because Rig's OpenAI provider may not expose per-request custom headers in the places PageSeeds needs them.

Clarify and document routing rules in PageSeeds:

- `kimi_backend_mode = bridge`: always use the bridge, no health fallback.
- `kimi_backend_mode = direct`: always use local `agent-wrapper` direct CLI fallback.
- `kimi_backend_mode = auto`: use the bridge only when health says it is available; otherwise fail with a clear setup message unless a caller explicitly chooses direct fallback.

Audit workflow routing:

- Content-writing tasks should request `acp`.
- Short stateless tasks should request `direct`.
- Structured extraction should generally request bridge/ACP unless PageSeeds deliberately uses JSON-schema direct mode.
- File-modifying agentic tasks outside the current `is_content_task` list should be reviewed before continuing to force `direct`.

Potentially add a small helper:

```rust
fn kimi_backend_preference_for_step(task: &Task, step: &WorkflowStep) -> Option<&'static str>
```

This avoids burying important routing policy inside a local boolean in `exec_agentic`.

### Phase 4: Reduce Kimi-Specific Serialization Workarounds

The bridge now accepts array-form OpenAI message content, so PageSeeds no longer needs strict string content for that reason alone.

Do not remove `rig/compat/kimi.rs` immediately. It still owns useful behavior:

- `X-Kimi-Backend` routing.
- Kimi-specific structured extraction fallback.
- Retry policy.
- Bridge error handling once added.

After the bridge is verified with PageSeeds typed extraction, PageSeeds can evaluate two options:

1. Continue using the custom adapter, but simplify it and update comments so it is a bridge policy adapter rather than a wire-format workaround.
2. Move Kimi completions and extraction to standard Rig OpenAI provider paths if custom headers and backend preferences can be expressed cleanly.

Acceptance for removing the workaround:

- A Rig native extractor call against the bridge returns a valid typed result.
- A Rig native `.preamble()` call against the bridge succeeds with array-form system content.
- Backend preference can still be set per request, or PageSeeds accepts bridge `auto` for that path.
- Tests cover both direct and ACP bridge paths.

### Phase 5: Add Prompt Budget Preflight In PageSeeds

The bridge now rejects prompts over the hard limit. That is good, but PageSeeds should prevent known oversized workflows before sending them.

Add prompt byte budgeting at the PageSeeds side for high-risk agentic steps:

- CTR audit analysis and schema planning.
- Content review recommendation.
- Cannibalization strategy.
- Territory strategy.
- Any workflow that embeds task artifacts or article excerpts.

Recommended behavior:

- Estimate final prompt bytes before provider call.
- Compare against bridge health limits when provider is Kimi bridge.
- If prompt exceeds target budget, batch or trim deterministically.
- If prompt exceeds hard budget and cannot be batched, fail before provider call with a message naming the oversized artifact or context source.

Initial PageSeeds defaults should mirror the bridge until health data is wired:

| Backend | Target Budget | Hard Budget |
| --- | ---: | ---: |
| direct | 15 KB | 20 KB |
| acp | 15 KB | 20 KB |

Bridge limits should override these defaults when available.

### Phase 6: Optional Settings UI Improvements

Once typed health exists, Settings can show:

- Bridge reachable/unreachable.
- Bridge version.
- Kimi CLI version.
- Direct backend availability.
- ACP backend availability.
- Tool-call support.
- JSON mode support.
- Prompt byte limits.

This is useful, but not required for backend correctness.

## Test Plan

### Unit Tests

Add Rust tests with mocked HTTP responses for:

- Rich `/health` parsing.
- Legacy boolean `check_bridge_health()` compatibility.
- `prompt_too_large` bridge error parsing.
- `tools_not_supported` bridge error parsing.
- `backend_empty_response` bridge error parsing.
- Retryable versus non-retryable bridge errors.
- Backend preference header still being sent.

### Integration-Style Tests

Use wiremock to verify:

- Plain Kimi bridge prompt still works.
- Tool-call structured extraction parses valid `submit` arguments.
- JSON-mode fallback still parses fenced and bare JSON.
- Standard Rig OpenAI provider path can send array-form system content once/if PageSeeds tests that migration.

### Live Smoke Tests

Run only manually or as ignored tests:

- Bridge `/health` against local server.
- Short direct prompt through PageSeeds.
- Structured extraction through bridge/ACP.
- One content-writing task routed to ACP.
- One short non-content task routed to direct.

## Definition Of Done

- PageSeeds parses and logs rich Kimi bridge health.
- PageSeeds parses bridge error bodies and preserves request ids in task failures.
- Backend routing rules are explicit and covered by tests.
- Kimi bridge prompt-size failures are actionable and non-retried.
- High-risk workflows preflight prompt bytes or batch before hitting bridge hard limits.
- Existing `agent-wrapper` direct fallback remains available.
- Existing Kimi bridge behavior continues to work with current custom adapter.
- PageSeeds tests pass: `cargo check`, targeted Rust tests for provider/adapter behavior, and existing relevant workflow tests.

## Recommended Sequence

1. Add typed health and structured error parsing in PageSeeds.
2. Clarify `kimi_backend_mode` semantics and routing policy.
3. Add prompt budget preflight for oversized workflows.
4. Re-evaluate whether `rig/compat/kimi.rs` can be simplified or replaced by standard Rig OpenAI provider usage.

## Current Recommendation

Do not rush to remove the PageSeeds Kimi compatibility adapter. The new bridge makes the adapter less about wire-format compatibility and more about routing, retries, and user-friendly error handling. That is still useful.

The next PageSeeds change should be small and high-leverage: consume bridge health capabilities and parse structured bridge errors. That gives users clearer failures immediately and sets up prompt budgeting without destabilizing the working provider path.