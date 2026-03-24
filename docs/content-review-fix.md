# Fix: Content Review Pipeline

## Problem

The content review workflow runs but produces poor results. Three root causes:

### 1. No article selection — floods the board with tasks

The CLI picks the 10 highest-impact articles using a scoring formula (GSC position × impressions × CTR gaps × audit health × staleness). The app skips this entirely and creates an `optimize_article` task for every non-"good" article — typically 25–30 tasks.

### 2. Agent gets no structured context

The CLI builds a tight JSON payload per article (GSC snapshot, failing checks, source excerpt) and asks for JSON output matching an exact schema. The app sends the raw SKILL.md (a human workflow doc) for the review step, and a 6-line stub for each optimize step. The agent has to search the filesystem to find the file and guess what "optimize" means.

### 3. One agent call per article instead of one call total

The CLI makes one agent call with 10 pre-selected articles, gets back one `recommendations.json`, and creates one follow-up task. The app spawns N independent agent processes (3–5 min each), each cold-starting with no context.

## What already works (keep as-is)

- `exec_gsc_sync_articles` — native Rust GSC sync. Writes metrics into articles.json.
- `exec_content_audit` — native Rust 13-check audit. Writes content_audit.json.
- `exec_content_sync` — validates articles.json ↔ content files.
- `ContentReviewHandler` 4-step plan (steps 1–3 are solid).
- `TaskRunner` UI — progress overlay for running tasks.

## Changes needed

### A. Add deterministic article selection after audit (Rust)

Port `_select_priority_articles()` from `pageseeds-cli/dashboard_ptk/dashboard/tasks/content_review.py` lines ~480–560.

New function in `executor.rs`:

```
fn select_priority_articles(audit: &Value, max_items: usize) -> Vec<Value>
```

Scoring tiers (from CLI):
- Tier 1 (+1000): position 5–20, impressions > 200, CTR < 3% → quick CTR wins
- Tier 2 (+700): health="poor" and never reviewed or reviewed >12 months ago
- Tier 3 (+15 per check): checks_failed × 15
- Tier 3 (+inverse health): max(0, 100 - health_score)
- Penalty (−600): position 1–4 and CTR ≥ 5% → already performing well

Return top N (default 5) with score > 0, sorted descending.

### B. Build structured context with source excerpts (Rust)

New function in `executor.rs`:

```
fn build_review_context(articles: &[Value], project_path: &str, max_excerpt_chars: usize) -> Value
```

For each selected article:
1. Resolve file path from `file` field relative to project root
2. Read first `max_excerpt_chars` (2600) of the source file
3. Build JSON object: `{ article_id, title, file, url_slug, target_keyword, gsc_snapshot, failed_checks, source_excerpt }`

Return: `{ generated_at, articles: [...] }`

### C. Replace SKILL.md agent call with structured prompt (Rust)

New function in `executor.rs`:

```
fn build_review_prompt(context: &Value) -> String
```

Prompt template (from CLI's `_build_recommendations_prompt`):

```
Generate SEO recommendations JSON from the provided article context.

Return ONLY one valid JSON object. No markdown fences, no commentary.

Input context:
{context_json}

Output schema:
{
  "generated_at": "<ISO>",
  "articles": [
    {
      "article_id": <id>,
      "article_title": "<title>",
      "article_file": "<path>",
      "url_slug": "<slug>",
      "target_keyword": "<keyword>",
      "suggestions": [
        {
          "category": "title|meta_description|intro|h1|internal_links|faq|eeat|cta",
          "current": "<what's there now>",
          "proposed": "<specific replacement>",
          "reason": "<one sentence why>"
        }
      ]
    }
  ]
}

Requirements:
- 4–8 actionable suggestions per article.
- Use only the provided context.
- Preserve article metadata fields exactly from input.
```

### D. Replace spawn-per-article with single recommendations artifact

Instead of `spawn_content_review_tasks()` creating N optimize_article tasks:

1. Parse agent JSON output → write `recommendations.json`
2. Create **one** `content_review_apply` task with the recommendations as an artifact
3. That task's prompt includes the full recommendations.json and instructs the agent to apply the top-priority fixes to the actual files

### E. Update ContentReviewHandler step plan

Replace step 4 with a new step kind `content_review_recommend` that:
1. Calls `select_priority_articles` on the audit output
2. Calls `build_review_context` with the selected articles
3. Calls `build_review_prompt` with the context
4. Runs the agent with the structured prompt
5. Parses and persists `recommendations.json`
6. Creates one follow-up task

This replaces the current "agentic" step that sends raw SKILL.md.

## Acceptance criteria

- Content review produces `recommendations.json` with 4–8 suggestions per article
- Only 5–10 priority articles reviewed (not all 27)
- One agent call for review, not N
- Agent prompt includes source excerpts and GSC data, not just check names
- `content_review_apply` task has recommendations as artifact context
- Total wall-clock time: under 5 minutes (down from 90+)

## Files to modify

| File | Change |
|------|--------|
| `src-tauri/src/engine/executor.rs` | Add `select_priority_articles`, `build_review_context`, `build_review_prompt`. Replace `spawn_content_review_tasks` with single-task creation. Add `exec_content_review_recommend` step runner. |
| `src-tauri/src/engine/workflows/handlers.rs` | Update `ContentReviewHandler.plan()` step 4 from `"agentic"` to `"content_review_recommend"`. Remove or repurpose `ContentHandler` for `optimize_article`. |
| `src-tauri/src/engine/agent.rs` | Enforce safe non-interactive Copilot permissions (`--allow-all-tools` + deny git shell commands). |
