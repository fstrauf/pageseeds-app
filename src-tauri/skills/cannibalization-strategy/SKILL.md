# Cannibalization Strategy Skill

Used by the `can_analyze_candidates` agentic step.

## Input

A single merge candidate JSON containing:
- `candidate_id`: Machine-readable identifier
- `candidate_type`: `"merge_candidate"` or `"exact_keyword_dupe"`
- `theme`: Common theme / target keyword
- `pages`: Array of pages, each with `id`, `url`, `title`, `h1`, `target_keyword`, `impressions`, `clicks`, `avg_position`, `word_count`, `incoming_internal_links`, `outgoing_internal_links`, `published_date`, `excerpt`
- `top_shared_queries`: Most common queries shared across pages
- `shared_query_count`: Number of distinct overlapping queries
- `total_impressions`: Sum of impressions across all pages

## Analysis Rules

### 1. Identify True Cannibalization
- Analyze the candidate cluster and distinguish **true cannibalization** from mere topical similarity.
- True cannibalization occurs when two or more articles target the **same search intent** for the **same keyword** and compete against each other in SERPs.
- **Exact keyword duplicates**: Candidates with `candidate_type: "exact_keyword_dupe"` have the **identical target_keyword**. They are guaranteed cannibalization cases — you MUST recommend a merge. Do NOT return `no_action: true` for these.
- **Do NOT return `no_action: true` for clusters where 3+ articles share the exact same target_keyword.** These are almost certainly cannibalizing each other.
- Topical overlap alone (e.g., two articles mentioning the same broad topic) is not sufficient.

### 2. Merge Recommendations
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
  - **Content depth**: Longer, more thorough, better-structured content wins.
  - **Publish date**: More recent content is preferred if depth and authority are comparable.
- The keeper should be the **strongest overall article** in the cluster.
- Redirect targets should be merged into the keeper **before** applying 301s: preserve unique examples, data points, or angles.
- Set `confidence` to `high`, `medium`, or `low` based on evidence strength.
- **no_action is only for clusters where the pages clearly serve different search intents** (e.g., one is a beginner tutorial, another is an advanced strategy, and the keywords differ).

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
- `reason`: One-sentence justification for the keeper choice.
- `no_action`: `true` ONLY if the pages clearly serve different search intents and do not cannibalize each other. `false` for all true cannibalization cases.
- `confidence`: `"high"`, `"medium"`, or `"low"`.

**CRITICAL:**
- Return ONLY a single JSON object. Do NOT wrap it in arrays or return multiple recommendations.
- Do NOT return `no_action: true` for exact keyword duplicates (`candidate_type: "exact_keyword_dupe"`).
- Every merge recommendation must name a keeper `id` and at least one redirect `id`.
- Every `keep_id` and `redirect_id` must be one of the `id`s present in the provided candidate `pages`. An id not in the page set cannot be resolved and the recommendation will be discarded.
- `keep_id` must not appear in `redirect_ids`.
- Do NOT output `keep_url`, `redirect_urls`, or any URL string. Output ids only.
