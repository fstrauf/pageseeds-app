# Cannibalization Audit Prompt Budgeting Spec

**Status:** Implemented  
**Date:** 2026-05-01  
**Scope:** Make `cannibalization_audit` reliable under Kimi bridge prompt limits by replacing one site-wide agent prompt with deterministic candidate preparation and byte-budgeted agent analysis.

## Problem

The current cannibalization audit can fail with:

```text
prompt_too_large ... Prompt too large (321686 bytes). Limit: 20000 bytes.
```

The workflow builds a large cannibalization context and passes it directly into one agent prompt. For a 142-article site, deterministic clustering produced 1,986 similarity pairs and one giant cluster, causing nearly the entire site context to be sent to Kimi in a single request.

Relevant implementation:

- `src-tauri/src/engine/workflows/handlers.rs`
- `src-tauri/src/engine/exec/cannibalization_audit.rs`
- `src-tauri/src/engine/executor.rs`
- `.github/skills/cannibalization-strategy/SKILL.md`

## Goals

- Keep every Kimi bridge prompt under the configured hard limit.
- Preserve enough evidence for high-quality keeper, redirect, hub, and territory recommendations.
- Move mechanical work into deterministic Rust steps.
- Ask the LLM to judge small, prepared cases instead of the full site at once.
- Produce the same final review artifact shape expected by the UI and downstream review flow.

## Non-Goals

- Do not auto-apply redirects or destructive merge actions.
- Do not rewrite the entire cannibalization system.
- Do not replace the Kimi bridge or provider layer.
- Do not rely on a larger model context window as the fix.

## Current Behavior

The current workflow is:

```text
can_gsc_sync
can_coverage_load
can_build_context
can_analyze
```

`can_build_context` writes full artifacts to disk, then also returns a large `agent_context` as step output.

Because the step uses `LatestRawPolicy::ReplaceWithOutput`, the executor stores that full JSON as `latest_raw`. `can_analyze` receives `latest_raw`, concatenates it with the cannibalization strategy skill, and calls `agent::run_agent`.

This bypasses the generic agentic prompt-budget preflight used elsewhere.

## Proposed Architecture

Split the audit into deterministic evidence, deterministic candidate selection, byte-budgeted agent analysis, and deterministic reduction.

```text
can_gsc_sync                  deterministic
can_coverage_load             deterministic
can_build_context             deterministic, writes full artifacts
can_select_candidates         deterministic, creates compact candidate work units
can_analyze_candidates        agentic, byte-budgeted batches
can_reduce_strategy           deterministic, validates and merges model outputs
```

## Data Flow

`can_build_context` should continue writing full reference artifacts:

- `cannibalization_audit_context.json`
- `cannibalization_clusters.json`
- `hub_gaps.json`
- `territory_analysis.json`

But it should no longer return the full context as `latest_raw`.

Instead, it should return a compact summary:

```json
{
  "artifact_paths": {
    "context": ".github/automation/cannibalization_audit_context.json",
    "clusters": ".github/automation/cannibalization_clusters.json",
    "hub_gaps": ".github/automation/hub_gaps.json",
    "territory_analysis": ".github/automation/territory_analysis.json"
  },
  "summary": {
    "total_articles": 142,
    "similarity_pairs": 1986,
    "candidate_clusters": 12,
    "hub_gaps": 1,
    "territories": 44
  }
}
```

## Candidate Selection

Add a deterministic candidate selection step that reads the full artifacts and emits `cannibalization_candidates.json`.

Candidate types:

- `merge_candidate`: possible keeper/redirect decision
- `hub_candidate`: possible hub or pillar recommendation
- `territory_candidate`: possible new or saturated territory recommendation

Merge candidates should be gated by stronger evidence than broad TF-IDF similarity alone:

- exact or near-exact target keyword overlap
- shared GSC query overlap
- high similarity score above a stricter threshold
- same intent phrase family
- meaningful impressions on at least one page

The selector should avoid connected-component chain explosions. A giant similarity component should be split by target keyword, shared query sets, or strongest local edges.

## Prompt Budgeting

`can_analyze_candidates` must build prompts by byte budget.

Recommended defaults:

```text
target_prompt_bytes: 15000
hard_prompt_bytes: 20000
max_candidates_per_batch: 1 merge cluster, or 3 small non-merge candidates
max_pages_per_merge_candidate: 8
max_excerpt_words_per_page: 60
```

If a candidate still exceeds the hard budget:

1. Remove page excerpts.
2. Keep title, URL, H1, target keyword, GSC metrics, links, word count, and top shared queries.
3. If still too large, fail that candidate with a clear validation error instead of calling the provider.

## Agent Tasks

Use separate prompt shapes for separate judgment types.

Merge prompt:

- Input: one candidate cluster
- Output: true cannibalization decision, keeper URL, redirect URLs, confidence, reason
- The model should be allowed to return `no_action` when overlap is topical but intent differs.

Hub prompt:

- Input: one hub gap candidate
- Output: hub needed or not, suggested URL/title, spoke IDs, outline

Territory prompt:

- Input: compact territory evidence
- Output: priority, demand evidence, suggested tasks

Do not ask one prompt to solve merges, hubs, territories, and calculator strategy at the same time.

## Reduction And Validation

Add a deterministic reducer that merges batch outputs into the final `cannibalization_strategy.json`.

Validation rules:

- Every keeper URL must exist in the inventory.
- Every redirect URL must exist and differ from the keeper.
- Every merge recommendation must include `confidence`.
- Low-confidence recommendations remain review-only.
- Hub URLs must not collide with existing non-hub pages.
- Invalid batch outputs fail the audit unless explicitly marked as recoverable.

## Final Output

The final strategy artifact should preserve the existing review-facing shape:

```json
{
  "generated_at": "2026-05-01T00:00:00Z",
  "merge_recommendations": [],
  "hub_recommendations": [],
  "territory_recommendations": [],
  "risks": []
}
```

## Implementation Plan

1. Add prompt-size preflight to `exec_can_analyze` as an immediate guardrail.
2. Change `can_build_context` to return a compact artifact summary instead of the full agent context.
3. Add deterministic candidate selection from existing artifacts.
4. Add byte-budgeted candidate analysis with per-candidate prompts.
5. Add deterministic strategy reduction and validation.
6. Update tests to cover giant-cluster splitting and prompt budget enforcement.
7. Run a live audit and verify every Kimi request is under the hard prompt limit.

## Acceptance Criteria

- A 142-article audit no longer sends a single site-wide 300 KB prompt.
- No Kimi bridge request exceeds 20 KB.
- Giant similarity components are split before agent analysis.
- The audit can complete with multiple small agent calls.
- The final `cannibalization_strategy.json` remains reviewable in the existing UI.
- Provider errors identify the failed candidate or batch.
- Full raw evidence remains available on disk for debugging and review.

## Open Questions

- Should shared-query overlap be mandatory for merge candidates, or only a strong ranking signal?
- What should the default maximum pages per merge candidate be?
- Should hub and territory recommendations run in the same task or separate follow-up audit tasks?
- Should prompt budgets come from bridge health metadata instead of static constants?