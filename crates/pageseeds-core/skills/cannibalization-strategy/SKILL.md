# Cannibalization Strategy Skill

<!-- skill-version: 3 -->

Used by the `can_analyze_candidates` agentic step.

## Input

A single merge candidate JSON containing:
- `candidate_id`: Machine-readable identifier
- `lane`: Evidence lane — one of `"exact_keyword"`, `"shared_query"`, `"near_dupe"` (required)
- `candidate_type`: Skill-facing type mapped from lane:
  - `"exact_keyword_dupe"` ← `lane: "exact_keyword"`
  - `"shared_query"` ← `lane: "shared_query"`
  - `"near_dupe"` ← `lane: "near_dupe"`
- `theme`: Common theme / target keyword / shared query
- `pages`: Array of pages (2–4), each with:
  - `id`, `url`, `title`, `h1`, `target_keyword`
  - `impressions`, `clicks`, `avg_position`
  - **`word_count`**: real body word count when article evidence is available
  - **`outline_text`**: heading outline (when available) — prefer this over thin `excerpt`
  - **`top_queries`**: top GSC queries for the page (when available)
  - `incoming_internal_links`, `outgoing_internal_links`, `published_date`
  - `excerpt`: 60-word fallback only when outline/evidence is missing
- `top_shared_queries` / `shared_queries`: Queries shared across pages in this candidate
- `shared_query_count`: Number of distinct overlapping queries
- `total_impressions`: Sum of impressions across all pages
- `pair_similarity` / `max_pairwise_sim` (optional): Cosine similarity for near_dupe pairs

## Evidence lanes

| Lane | `candidate_type` | Meaning | Merge posture |
|------|------------------|---------|---------------|
| `exact_keyword` | `exact_keyword_dupe` | Identical non-empty `target_keyword` | **Mandatory merge** |
| `shared_query` | `shared_query` | Same GSC query on ≥2 pages (real SERP competition) | Merge bias OK when same intent |
| `near_dupe` | `near_dupe` | High pairwise content similarity only | **Refuse by default** |

Soft TF-IDF topical clusters are **not** evidence lanes. Do not invent merges from theme cohesion alone.

## Analysis Rules

### 1. Identify True Cannibalization
- Analyze the candidate cluster and distinguish **true cannibalization** from mere topical similarity.
- True cannibalization occurs when two or more articles target the **same search intent** for the **same keyword/query** and compete against each other in SERPs.
- **Exact keyword duplicates** (`candidate_type: "exact_keyword_dupe"` / `lane: "exact_keyword"`): guaranteed cannibalization — you MUST recommend a merge. Do NOT return `no_action: true` for these.
- **Shared query** (`candidate_type: "shared_query"`): pages already compete for the same SERP query. Merge when titles/H1s/outlines show the **same intent**; if intents clearly differ (e.g. commercial vs informational for the same head term), prefer `no_action` with a clear reason.
- **Near dupe** (`candidate_type: "near_dupe"`): high content similarity is **not** enough. Refuse by default unless there is strong same-intent / shared-query evidence in the package.
- Topical overlap alone is not sufficient. Do not set `confidence: "high"` without citing concrete evidence (shared queries, matching keywords, same-intent outline sections).

### 2. Refuse-by-default for near_dupe
- For `near_dupe`, set `no_action: true` unless there is **strong same-intent / same-query evidence** in the payload.
- Strong evidence includes: non-empty `top_shared_queries` / `shared_queries` / `shared_query_count`, identical or near-identical target keywords, and outlines/titles/H1s that clearly target the same search intent.
- Soft topical cohesion or a high `pair_similarity` / `max_pairwise_sim` alone is **not** enough to recommend a merge.
- When pages have **distinct non-empty target_keywords** and **no shared queries**, do **not** recommend a merge (multi-intent without SERP evidence).
- When you **do** recommend a merge on a non-exact candidate, the `reason` **must** cite the intent/query evidence. Do not justify merges only with impressions or traffic.

### 3. Use package evidence
- Prefer `outline_text` + `top_queries` + real `word_count` over thin `excerpt` snippets when judging intent and content depth.
- Use `top_queries` per page to see whether pages actually compete for the same SERP queries.
- Depth/structure signals (outline length, word_count) inform keeper selection, not whether cannibalization exists.

### 4. Merge Recommendations
For true cannibalization, recommend which article to **KEEP** and which to **redirect**:

- **Mandatory merges**: If 3+ articles share the identical target_keyword, you MUST recommend a merge unless one article has 10x more impressions than all others combined.
- **Exact duplicate candidates**: GSC performance data is already provided. Use it as the primary authority signal:
  - The article with the **highest impressions + lowest avg_position** is usually the best keeper.
  - If the top performer also has the cleanest URL and deepest content, the choice is obvious.
  - Only override GSC rank if the top performer has a terrible URL or clearly outdated content.
- **Keeper selection criteria** (evaluate all, then decide):
  - **Impressions**: Higher impressions = stronger authority signal.
  - **avg_position**: Lower position (closer to 1.0) = better SERP ranking.
  - **URL quality**: Shorter, cleaner, more keyword-aligned URLs are preferred.
  - **Content depth**: Longer, more thorough, better-structured content wins (use `word_count` + `outline_text`).
  - **Publish date**: More recent content is preferred if depth and authority are comparable.
- The keeper should be the **strongest overall article** in the cluster.
- Redirect targets should be merged into the keeper **before** applying 301s: preserve unique examples, data points, or angles.
- Set `confidence` to `high`, `medium`, or `low` based on evidence strength. High confidence requires shared-query or exact-keyword evidence — not similarity alone.
- **no_action is the default for near_dupe** unless same-intent/same-query evidence is strong. For exact keyword dupes, `no_action` is never appropriate.

## Output Contract

You are analyzing **ONE candidate cluster**. Return **ONE JSON object** with exactly these fields.

**You identify pages by their `id` (the integer from each page's `id` field) — never by URL string.** The workflow resolves your id selection to canonical URLs deterministically. This is mandatory: returning a URL you typed by hand will be rejected.

```json
{
  "cluster_id": "cash_secured_puts_best_stocks",
  "cluster_theme": "cash-secured-puts",
  "keep_id": 17,
  "redirect_ids": [42, 88],
  "merge_before_redirect": true,
  "merge_instructions": [
    "Move the risk-management table from the cash-secured-puts-playbook page into the keeper.",
    "Preserve the brokerage-specific example as a subsection."
  ],
  "reason": "Keeper has highest impressions, cleanest URL, strongest internal link count, and best position.",
  "no_action": false,
  "confidence": "high"
}
```

**Field descriptions:**
- `cluster_id`: Copy the `candidate_id` from the input.
- `cluster_theme`: Copy the `theme` from the input.
- `keep_id`: The `id` of the single best article to keep. Must be one of the `id`s in the provided `pages`.
- `redirect_ids`: Array of `id`s to 301-redirect to the keeper. Each must be one of the `id`s in the provided `pages`.
- `merge_before_redirect`: `true` if unique content from redirect targets should be merged into the keeper first.
- `merge_instructions`: Array of specific instructions for what content to preserve during the merge. Reference pages by title or `target_keyword`, not by URL.
- `reason`: One-sentence justification for the keeper choice. For non-exact merges, must cite intent/query evidence.
- `no_action`: `true` if the pages do not clearly cannibalize (default for near_dupe without strong evidence). `false` for true cannibalization and always for exact keyword dupes.
- `confidence`: `"high"`, `"medium"`, or `"low"`. Do not use `high` without shared-query or exact-keyword evidence.

**CRITICAL:**
- Return ONLY a single JSON object. Do NOT wrap it in arrays or return multiple recommendations.
- Do NOT return `no_action: true` for exact keyword duplicates (`candidate_type: "exact_keyword_dupe"`).
- For `candidate_type: "near_dupe"`, default to `no_action: true` unless same-intent / same-query evidence is strong; when merging, state that evidence in `reason`.
- For `candidate_type: "shared_query"`, merge when intent aligns; refuse when outlines/keywords show distinct intents for the shared query.
- Every merge recommendation must name a keeper `id` and at least one redirect `id`.
- Every `keep_id` and `redirect_id` must be one of the `id`s present in the provided candidate `pages`. An id not in the page set cannot be resolved and the recommendation will be discarded.
- `keep_id` must not appear in `redirect_ids`.
- Do NOT output `keep_url`, `redirect_urls`, or any URL string. Output ids only.
