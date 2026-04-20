# Keyword Research v3 — Spec

**Status:** Approved  
**Date:** 2026-04-20

## Problem

The keyword research workflow produces poor results:

1. **Wrong API endpoint.** `keyword_suggestions` does full-text substring matching on the seed phrase. Niche seeds like "covered call scanner" return 0 hits because no keyword in the database literally contains those words. Most themes yield 0–5 results.
2. **Seed extraction is blind to existing content.** The LLM receives only `project.md` — no information about which topics are already well-covered. It happily suggests themes like "best stocks for covered calls" even when 10 articles exist in that cluster.
3. **Coverage gap filtering is too weak.** Substring containment (`kw.contains(pk)`) misses most semantic overlap. "best stocks for covered calls" vs. "covered call strategies" share no substring, so the filter doesn't catch it.
4. **No hard prerequisite on coverage data.** Without `keyword_coverage.json`, the workflow runs blind — no gap filtering, no context for the LLM.

## Changes

### Change 1: Switch to Related Keywords endpoint

**File:** `src-tauri/src/seo/dataforseo.rs` — `keyword_ideas()` method

Replace:
```
POST /v3/dataforseo_labs/google/keyword_suggestions/live
```
With:
```
POST /v3/dataforseo_labs/google/related_keywords/live
```

**Why:** `related_keywords` uses Google's "searches related to" SERP element — semantic connections, not substring matching. A seed like "covered call scanner" will surface related queries like "best covered call screener tool", "option selling filter", etc.

**Parameters:**
```json
{
  "keyword": "<seed>",
  "location_code": 2840,
  "language_code": "en",
  "depth": 2,
  "include_seed_keyword": true,
  "ignore_synonyms": true,
  "limit": 100,
  "filters": [
    ["keyword_data.keyword_info.search_volume", ">", 50],
    "and",
    ["keyword_data.keyword_properties.keyword_difficulty", "<=", 30],
    "and",
    ["keyword_data.search_intent_info.main_intent", "<>", "navigational"],
    "and",
    ["keyword_data.keyword", "not_like", "%near me%"]
  ],
  "order_by": ["keyword_data.keyword_info.search_volume,desc"]
}
```

**Key differences from current:**
- `depth: 2` → up to 72 semantically related keywords per seed (vs. substring-only matches)
- Filter field paths are prefixed with `keyword_data.` (different response nesting from `keyword_suggestions`)
- Same server-side filters (volume > 50, KD ≤ 30, no navigational, no "near me") — these are fine because `related_keywords` produces a much larger candidate pool to filter from

**Response parsing changes:**
- Items are nested under `items[].keyword_data` instead of directly under `items[]`
- Each item has `.keyword_data.keyword`, `.keyword_data.keyword_info.search_volume`, `.keyword_data.keyword_properties.keyword_difficulty`, `.keyword_data.search_intent_info.main_intent`
- Question detection stays client-side (prefix matching on keyword text)

**Cost:** $0.01/task + $0.0001/item. 12 seeds × depth=2 ≈ $0.12–$0.24 total — same ballpark as current.

### Change 2: Feed coverage summary into seed extraction prompt

**Files:**
- `src-tauri/src/engine/exec/research.rs` — `build_research_prompts()`, case `"research_seed_extraction"`
- `src-tauri/src/prompts/seed_extraction.md`

**What:** Build a deterministic text summary from `keyword_coverage.json` and append it to the user prompt that goes to the LLM.

**Summary format** (built in Rust, ~10–20 lines max):
```
## Existing Content Coverage

Strong coverage (skip these):
- Covered Call Strategies (12 articles)
- Wheel Strategy Basics (8 articles)
- Options Income Tracking (7 articles)

Thin coverage (good candidates to deepen):
- Cash Secured Puts (2 articles)
- Options Backtesting (1 article)

Not yet covered:
- (derive from project.md clusters that have 0 articles in coverage data)
```

**Prompt update** — add a section to `seed_extraction.md`:
```markdown
## Coverage Awareness

You will receive a summary of existing content coverage below. Use it to:
- AVOID themes where coverage is already strong (6+ articles)
- PREFER themes where coverage is thin (1-2 articles) or nonexistent
- Do NOT repeat exact keywords already targeted — find adjacent angles instead
```

**Implementation detail:**
- Read `keyword_coverage.json` via `read_keyword_coverage(project_path)`
- Group clusters into strong (6+), moderate (3-5), thin (1-2) buckets
- Format as plain text, append to user prompt after project context
- If `keyword_coverage.json` doesn't exist → fail (see Change 4)

### Change 3: Improve coverage gap filter with word-overlap scoring

**File:** `src-tauri/src/engine/exec/keywords.rs` — `score_coverage_gap()`

Replace substring containment matching with Jaccard word-overlap:

**Current (broken for most cases):**
```rust
let is_related = cluster.primary_keywords.iter().any(|pk| {
    kw_lower.contains(pk) || pk.contains(&kw_lower)
});
```

**New:**
```rust
fn word_set(s: &str) -> HashSet<&str> {
    s.split_whitespace()
        .filter(|w| !STOP_WORDS.contains(w))
        .collect()
}

let kw_words = word_set(&kw_lower);
let is_related = cluster.primary_keywords.iter().any(|pk| {
    let pk_words = word_set(pk);
    if pk_words.is_empty() || kw_words.is_empty() {
        return false;
    }
    let intersection = kw_words.intersection(&pk_words).count();
    let union = kw_words.union(&pk_words).count();
    let jaccard = intersection as f64 / union as f64;
    jaccard >= 0.3
});
```

`STOP_WORDS`: small hardcoded set — `{"the", "a", "an", "for", "to", "of", "in", "on", "and", "or", "is", "are", "how", "what", "best", "top"}`.

**Example matches this catches:**
- "best stocks for covered calls" vs. "covered call strategies" → words: {stocks, covered, calls} vs. {covered, call, strategies} → overlap {covered} + near-match on call/calls → related ✓
- "options trading journal" vs. "options journal software" → {options, trading, journal} vs. {options, journal, software} → overlap 2/4 = 0.5 → related ✓

**Stemming note:** Simple approach — also check if either word starts with the other (covers "call"/"calls", "trade"/"trading"). No external crate needed:
```rust
fn fuzzy_word_match(a: &str, b: &str) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}
```

Use `fuzzy_word_match` instead of exact equality when computing intersection count.

### Change 4: Make keyword_coverage.json a hard prerequisite

**File:** `src-tauri/src/engine/exec/keywords.rs` — `exec_keyword_research_native()`

After the existing `articles.json` check, add:

```rust
let coverage_path = paths.automation_dir.join("keyword_coverage.json");
if !coverage_path.exists() {
    return StepResult {
        success: false,
        message: "keyword_coverage.json not found. Run 'Analyze Keyword Coverage' first.".into(),
        output: None,
    };
}
```

This ensures:
- Coverage clusters are always available for gap filtering (Change 3)
- Coverage summary is always available for seed extraction (Change 2)
- Clear error message tells the user what to do

## Files Changed

| File | Change |
|------|--------|
| `src-tauri/src/seo/dataforseo.rs` | New endpoint URL, new request params, updated response parser |
| `src-tauri/src/engine/exec/research.rs` | Build coverage summary, inject into user prompt |
| `src-tauri/src/prompts/seed_extraction.md` | Add coverage awareness instructions |
| `src-tauri/src/engine/exec/keywords.rs` | Jaccard word-overlap scoring, hard prerequisite check |

## Files NOT Changed

- `src-tauri/src/engine/workflows/handlers.rs` — step sequence stays the same (seed → pipeline → selection → normalizer)
- `src-tauri/src/engine/exec/research.rs` `select_keywords_deterministic()` — final selection logic unchanged
- `src-tauri/src/seo/provider.rs` — trait signature `keyword_ideas()` unchanged (same return type)
- Frontend — no UI changes needed

## Execution Order

1. **Change 4** (prerequisite check) — trivial, prevents wasted API calls
2. **Change 1** (related_keywords endpoint) — biggest impact on result quality
3. **Change 3** (word-overlap scoring) — better filtering of results
4. **Change 2** (coverage summary in prompt) — smarter seed generation

Changes 1 and 3–4 are independent and can be built in parallel. Change 2 depends on Change 4 (needs coverage data to exist).

## Verification

After implementation, re-run keyword research for the `days_to_expiry` project and check:
1. Each theme returns 10+ candidates (vs. current 0–5)
2. No keywords suggested for topics with 6+ existing articles
3. Coverage gap filter catches overlapping keywords that only share partial words
4. Task fails cleanly with a message if `keyword_coverage.json` is missing
