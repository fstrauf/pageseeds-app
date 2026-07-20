# Keyword Research: Remove Google Autocomplete, Validate Themes Directly

## Problem

The `research_keywords` / `research_landing_pages` pipeline fetched Google
Autocomplete per theme (step `research_autocomplete`) and then had an agentic
step (`research_seed_validation`) filter those suggestions into 1–3 seeds per
theme. This was designed as a cost-reduction measure in the Ahrefs/CapSolver
era, where every candidate keyword needed an individual paid KD check.

On the current DataForSEO path this rationale is stale and the step is
net-negative:

- **It no longer saves money.** DataForSEO bundles volume/KD/intent in the
  ideas call and filters server-side (volume > 50, KD ≤ 30, non-navigational).
  Pre-filtering candidates saves nothing. Worse, 1–3 seeds per theme × 2 paid
  calls per seed (`related_keywords` + `keyword_suggestions`) costs *more*
  than querying the raw themes directly (2 calls per theme).
- **It narrows discovery before data can vet it.** Autocomplete top-4 is
  popularity-biased toward head terms and mainstream phrasings. The entire
  paid discovery phase only explores neighborhoods around those few popular
  seeds — the opposite of the low-KD long-tail the final filter wants.
- **It is redundant.** DataForSEO `keyword_suggestions` is a superset of
  autocomplete (Google's own keyword DB, substring variations) with real
  metrics attached.
- **It is fragile.** Undocumented endpoint, forced `us/en`, silent degradation
  to empty suggestions (theme silently gets zero research).

## Decision

Remove the `research_autocomplete` step. Keep the agentic
`research_seed_validation` step — it is the only domain-relevance gate in the
pipeline — but repurpose it to work directly on the themes from
`research_seed_extraction`:

- **Input**: project brief + extracted themes (was: autocomplete suggestions).
- **Task**: validate each theme for domain relevance; for each on-topic theme,
  propose 1–3 sharpened seed phrasings (real-search-query phrasing, mix of
  head and long-tail/question angles) informed by the project brief.
- **Output contract**: unchanged —
  `{"validated_seeds": [{theme, seeds: [string]}]}` — so
  `SeedValidationOutput`, `parse_validated_seeds_artifact()`, and the
  downstream pipeline are untouched.

Expected effect: paid DataForSEO calls drop from up to 6 per theme to ~2–4 per
theme, one network dependency and one pipeline step disappear, and seed choice
is driven by domain knowledge instead of Google's popularity ranking.

## Changes

| File | Change |
|---|---|
| `src-tauri/src/engine/workflows/handlers.rs` | Remove `research_autocomplete` from the plan; update step comments (7 → 6 steps) |
| `src-tauri/src/engine/workflows/step_kind.rs` | Remove `ResearchAutocomplete` variant and its mappings |
| `src-tauri/src/engine/step_registry.rs` | Remove `ResearchAutocomplete` registration |
| `src-tauri/src/engine/exec/research/autocomplete.rs` | Delete `exec_research_autocomplete`; rename file to `final_selection.rs` (remaining contents are final selection + winnability) |
| `src-tauri/src/engine/exec/research/mod.rs` | Update module doc comment; module rename |
| `src-tauri/src/engine/exec/research/prompts.rs` | `research_seed_validation` reads the `research_seed_extraction` artifact (themes) instead of the autocomplete artifact |
| `src-tauri/src/prompts/seed_validation.md` | Rewrite contract: validate themes + propose seed phrasings |
| `src-tauri/src/models/research.rs` | Update `SeedValidationOutput` doc comment |
| `src-tauri/src/engine/exec/keywords/research_pipeline.rs` | Fix stale cost-estimate comment; log estimated cost from actual (theme, seed) pair count |
| `src-tauri/src/engine/exec/keywords/tests.rs` | Plan assertion: 6 steps |
| `src-tauri/src/engine/executor/tests.rs` | Remove autocomplete mock/env/assertions from full-flow test |
| `docs/WORKFLOW_ENGINE.md`, `docs/BUSINESS_PROCESSES.md` | Remove/update autocomplete references |

Out of scope: `seo/google_autocomplete.rs` stays — the legacy Ahrefs provider
path still uses it. The `custom_keyword_research` fallback (themes from task
description) is unchanged.

## Follow-up: candidate relevance check (implemented)

The first prod run after this change showed DataForSEO expansion can still
drift off-domain *after* seed validation (`assignment risk ao3` from an
options-trading seed — seeded vocabulary, wrong domain). Seed validation gates
seeds, not expanded keywords. A deterministic token-overlap filter was
considered and rejected: it cannot tell "ao3" (off-domain) from "61-day"
(on-domain new term), and it would kill legitimate semantic expansions
("iv crush" → "implied volatility calculator").

`research_final_selection` therefore: selects top 15 deterministically, runs
one batched LLM relevance check (`prompts/candidate_relevance.md`,
`CandidateRelevanceOutput`, non-fatal on failure), drops flagged candidates,
trims to 10, then enriches winnability.

## Verification

- `cargo check`, `cargo test` (esp. `execute_task_keyword_research_full_flow_with_mocked_http`, plan-shape tests, `all_task_types_have_non_fallback_handler`)
- `pnpm run check:task-store` (task-creation adjacent change)
