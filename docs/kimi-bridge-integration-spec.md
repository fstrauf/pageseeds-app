# Kimi Bridge Verification Notes

Date: 2026-05-01

## Purpose

This document records bridge-side verification notes for the Kimi ACP OpenAI bridge after it implemented the PageSeeds consumer contract from `docs/kimi-bridge-provider-spec.md`.

The PageSeeds-side adoption plan lives separately in `docs/pageseeds-kimi-bridge-adoption-spec.md` so local app work can be planned and implemented independently from bridge verification.

## Bridge Repository

Bridge repository checked:

```text
/Users/fstrauf/01_code/kimi-acp-openai-bridge
```

Bridge commit previously checked:

```text
e2de17e feat: implement PageSeeds consumer spec (all phases)
```

## Verification Command

Command run:

```bash
cd /Users/fstrauf/01_code/kimi-acp-openai-bridge && pytest -q
```

Result:

```text
57 passed in 0.43s
```

## Verified Bridge Coverage

The implementation covered the main PageSeeds bridge requirements at the time of verification:

- `GET /health` returns richer capability data, including backends and prompt limits.
- `GET /v1/models` returns `kimi-k2.5`.
- `POST /v1/chat/completions` supports direct and ACP routing.
- `X-Kimi-Backend` is accepted.
- Array-form OpenAI message content is normalized to strings.
- `response_format` accepts `json_object` and `json_schema`.
- Direct mode rejects native tool calls with `422 tools_not_supported`.
- Prompt byte limits return `413 prompt_too_large`.
- Empty direct output maps to `503 backend_empty_response`.
- Direct step-limit text is detected in the client path.

## Earlier Bridge Feedback

Earlier review feedback was intentionally bridge-side and has been kept out of the PageSeeds adoption spec. If the bridge has since implemented those fixes, re-run the bridge test suite and any targeted bridge probes in the bridge repository, then update this note with the newer commit and result.

Historical areas that were reviewed:

- Missing direct binary should return structured `503` with `x-request-id`.
- Non-streaming ACP tool calls should preserve `function.name` and `function.arguments`.
- ACP timeout/error events should surface as precise structured timeout errors instead of generic empty-response errors.
- `acp_first_content_timeout` should be enforced separately from first-event and idle timeouts.
- Direct mode should use controlled environment handling and workdir validation.
- Health timeout and concurrency limit reporting should match actual behavior.

## PageSeeds Follow-Up

The PageSeeds-specific implementation plan is now tracked in `docs/pageseeds-kimi-bridge-adoption-spec.md`.