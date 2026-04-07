# Keyword Research Pipeline Improvement Plan

## Problem Statement

The current native keyword research pipeline (`engine/exec/keywords.rs`) often returns only 2-4 suggestions instead of the target 10. The funnel is too narrow at the top and too brittle in the middle:

1. **Theme extraction is regex-based** and often produces junk (e.g. `### Cluster 7`) or overly narrow seeds
2. **`get_keyword_ideas` only runs on 3-4 themes** — not enough candidates enter the pipeline
3. **The `MIN_VOLUME = 100` floor is too aggressive** for the free Ahrefs API, which returns coarse labels like `LessThanOneHundred`
4. **KD checking stops early** once it finds 10 "with data" results, but Ahrefs free tools often return `null` KD for 60-70% of keywords, so the pipeline burns its 30-call budget on long-tail terms that have no KD data

## Root Cause: Endpoint Economics

| Endpoint | What it gives us | Cost |
|----------|-----------------|------|
| `stGetFreeKeywordIdeas` | ~20-50 keyword ideas per seed, with rough volume/difficulty labels | 1 CapSolver solve (~15s) **per seed keyword** |
| `stGetFreeSerpOverviewForKeywordDifficultyChecker` | Exact KD (0-100), SERP data, top-ranking metrics | 1 CapSolver solve (~15s) **per single keyword** |

**The scarce resource is the KD check.** We cannot brute-force 150 keywords. We need more seeds entering Phase 1, and smarter sampling in Phase 2.

---

## Proposed Redesign: Hybrid Agentic-Deterministic Pipeline

### Step 1 — Agentic: `context_to_seeds`
**Replaces regex-based theme extraction entirely.**

- **Input:** Full project context (`seo_content_brief.md`, `project_summary.md`, `articles.json`, gap analysis, task description)
- **Output:** **8-12 seed keywords** + **2-3 competitor domains**
- **Why agentic:** An LLM can understand context, avoid junk headings, and deliberately choose seeds broad enough to actually return Ahrefs data. It can also identify competitor domains for cheap traffic cross-reference later.
- **Prompt update needed:** `seed_extraction.md` currently asks for 3-4 themes of 1-3 words. We need a richer prompt that also asks for competitor domains and justifies why each seed was chosen.

### Step 2 — Deterministic: `expand_seeds`
- Call `get_keyword_ideas` for each of the 8-12 seeds
- For each seed, collect both `ideas` (regular) and `question_ideas`
- Deduplicate against existing `articles.json` keywords
- **Expected output:** 100-200 candidate keywords with rough volume labels

### Step 3 — Deterministic: `smart_sample_for_kd`
**The critical design decision.** Instead of checking every candidate, we sample ~40-50 keywords to maximize the chance of finding winners.

Sampling rules:
1. **Stratified by seed:** Pick top candidates from *each* seed's idea cluster (don't let one broad seed dominate the sample)
2. **Question quota:** Reserve ~10 slots specifically for question-based keywords (often lower competition)
3. **Difficulty-label diversity:** Include some keywords labeled `Easy` and `Medium` from the ideas response
4. **Lower/remove volume floor for sampling:** Drop `MIN_VOLUME` from 100 to 50 (or keep all candidates in the sample pool)
5. **Random sprinkle:** Add a few unpredictable long-tail candidates — Ahrefs data is sparse and non-obvious terms sometimes have the only available KD scores

**Output:** 40-50 keywords prioritized for the expensive KD check.

### Step 4 — Deterministic: `check_difficulty`
- Call `get_keyword_difficulty` for all 40-50 sampled keywords
- Do **NOT** stop early. Run the full sample.
- Record: exact KD, SERP metrics, `has_data` flag, top result URL
- **Expected hit rate:** ~30-40% will have KD data = 12-20 keywords with full data

### Step 5 — Deterministic: `competitor_context` (optional, cheap)
- Call `check_traffic` on the 2-3 competitor domains from Step 1
- Extract their top keywords and traffic
- Cross-reference with our candidate pool
- **Cost:** 2-3 CapSolver solves (~30-45s total)

### Step 6 — Agentic: `final_selection`
- **Input:** Full candidate dataset (with and without KD data) + competitor context
- **Output:** 10 selected keywords with titles and selection reasons
- **Why agentic:** The LLM can make judgment calls — e.g. "this keyword has no KD data but the SERP is weak and it's a perfect pillar fit" — rather than hard-filtering on `has_data == true`.

---

## Quick Wins (Can Ship First)

These can be done in the existing native pipeline without a full redesign:

- [x] **Lower `MIN_VOLUME`** from `100` to `50` (or make it configurable per project)
- [x] **Increase `max_api_calls`** from `30` to `50` (or `60`) — it's just time, not incremental cost
- [x] **Remove early exit** — don't stop at `target_with_data = 10`. Always run the full budget of KD calls and let the final selection agent sort it out
- [ ] **Add competitor domain extraction** to the seed step (or task description parsing)

---

## Open Question: Long-Tail Keywords

**Current state:** The prompts do **not** explicitly ask for long-tail keywords.

- `seed_extraction.md` asks for 1-3 word broad seeds
- `keyword_discovery.md` (agentic tool path) asks for variations like `how to [theme]`, `[theme] guide`, etc. — which often produce long-tail results indirectly
- `final_selection_keywords.md` filters for informational intent but does not mention "long-tail"
- The **only** mention of "long-tail" in the entire codebase is in `src/components/overview/Overview.tsx`:
  > "Find new long-tail keyword opportunities for your site, then select which to write about"

**Decision needed:**

Do we want to:
1. **Embrace long-tail explicitly** — add it to `seed_extraction.md` (e.g. "include 2-3 longer, specific question-based seeds") and to `final_selection.md` (e.g. "prefer keywords with 4+ words and clear informational intent")
2. **Stay broad and let the LLM decide** — keep seeds at 1-3 words, but let the final selection agent balance head terms vs. long-tail based on the KD data it sees
3. **Split into two task types** — one for "broad pillar keywords" and one for "long-tail opportunities"

**Recommendation:** Option 2 for now, with a soft preference for question-keywords in the sampling step. The free Ahrefs API is too sparse for systematic long-tail KD checking. If we force long-tail, we'll get even more `null` KD results. Instead, let the agent use the available data intelligently.

---

## Implementation Checklist

### Phase 1: Quick Wins
- [x] Lower `MIN_VOLUME` to `50` in `engine/exec/keywords.rs`
- [x] Increase `max_api_calls` to `50` in `engine/exec/keywords.rs`
- [x] Remove early exit on `target_with_data` — always run full budget
- [x] Add competitor domain extraction to the seed step (or task description parsing)
- [x] ~~Add fallback to KD checks on seed themes~~ — **Replaced with Google Autocomplete**

### Phase 2: Agentic Seed Step
- [x] Rewrite `seed_extraction.md` to accept full project context and return 8-12 seeds + 2-3 competitor domains
- [x] Update `models/research.rs` seed output type to include `competitors: Vec<String>`
- [x] Wire the new seed step into `engine/workflows/handlers.rs` for research tasks (already wired, artifact parsing updated)
- [ ] Remove or deprecate the regex-based `derive_themes_from_project` fallback

### Phase 3: Smart Sampling
- [x] Implement `smart_sample_for_kd` in `engine/exec/keywords.rs`
  - [x] Stratified sampling by seed source
  - [x] Question-keywords quota (at least 1 per theme if available)
  - [ ] Difficulty-label diversity heuristic (deferred — ideas API only gives Easy/Medium/Hard labels)
  - [x] Lower volume threshold for sampling (`MIN_VOLUME` = 50)
- [x] Log sampling decisions for debugging

### Phase 4: Full Budget KD + Competitor Context
- [x] Run full 40-50 sample through `get_keyword_difficulty`
- [x] Add optional `check_traffic` call for competitor domains (fetches top 5 keywords per competitor)
- [x] Merge competitor insights into `KeywordPipelineOutput` for final selection context

### Phase 5: Final Selection Prompt Update
- [x] Update `final_selection_keywords.md` to explicitly handle missing KD data
- [x] Update `final_selection_landing_pages.md` with same `has_data: false` guidance + competitor context
- [x] Give the agent permission to select keywords with `has_data: false` if the SERP/positioning story is strong
- [x] Soft long-tail guidance: balance head terms and question keywords (4+ words)
- [x] Lowered volume floor in prompts from 100 → 50

### Phase 6: Frontend / Tooling
- [x] Regenerated TypeScript bindings to include `CompetitorInsight` and `CompetitorTopKeyword`
- [ ] Update `KeywordResearch.tsx` to expose batch/bulk flows if standalone research is needed (deferred)
- [ ] Consider adding a "Research Budget" setting so users can tune `max_api_calls` and sample size (deferred)

---

## Changelog

### 2026-04-02
- **Phase 1 shipped:** Lowered `MIN_VOLUME` to 50, increased `max_api_calls` to 50, removed early exit on `target_with_data`
- **Phase 2 shipped:** Updated `seed_extraction.md` to request 8-12 seeds + 2-3 competitors; added `competitors` field to `SeedExtractionOutput` and `KeywordPipelineOutput`; updated artifact parser to extract competitors
- **Phase 3 shipped:** Implemented `smart_sample_candidates` with stratified theme sampling and question-keyword quotas; added unit tests for sampling logic
- **Phase 4 shipped:** Added competitor traffic lookups via `check_traffic`; competitor insights now flow into `KeywordPipelineOutput`
- **Phase 5 shipped:** Updated both final selection prompts to allow `has_data: false` selections and to leverage competitor context
- **Phase 6 shipped:** Regenerated TS bindings for new research types
- **Bugfix:** Fixed broken test `workflow_uses_four_step_hybrid_workflow` that had stale `agentic` kind assertion for `research_final_selection`
- **Google Autocomplete integration:** Completely replaced the broken Ahrefs `get_keyword_ideas` endpoint with Google Autocomplete API (`suggestqueries.google.com`). This provides:
  - Free, no-auth keyword suggestions
  - Question-based keyword discovery via prefix variations (what is, how to, etc.)
  - ~20-30 suggestions per seed theme
  - No CapSolver cost for the ideas phase (only KD checks use CapSolver now)

---

## Expected Outcomes

| Metric | Current | After Quick Wins | After Full Redesign |
|--------|---------|------------------|---------------------|
| Typical suggestions | 2-4 | 4-7 | 8-10 |
| Time per run | ~7-9 min | ~10-12 min | ~15 min |
| Seed quality | Regex/heuristic | Regex/heuristic | Agentic |
| KD hit rate | ~20-30% | ~30-40% | ~30-40% |
| User control | None | None | Budget slider + seed preview |
