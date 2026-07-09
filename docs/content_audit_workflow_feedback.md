# Content Audit Workflow Feedback — From Manual Dump to Built-in Loop

> Based on the Days to Expiry audit run on 2026-07-08.  
> Goal: turn the static `content_audit` JSON output into an actionable, repeatable workflow inside PageSeeds.

---

## Problem we just hit

Running `run-content-audit` produced a 26 KB JSON report. To turn it into work, a human had to:

1. Parse the summary (`good / needs_improvement / poor`).
2. Scan every article for repeated warnings.
3. Manually group issues into patterns (missing external links, duplicate keywords, meta length, temporal URLs, thin content).
4. Cross-reference health scores to pick a priority list.
5. Open individual MDX files to confirm findings.
6. Decide which lever to pull first.
7. Create tasks / skills manually.
8. Re-run the audit and compare numbers by memory.

This is slow and does not scale across multiple projects. The audit already knows everything; the app should surface it as a workflow.

---

## Proposed workflow: Find → Pattern → Task → Fix → Re-audit

The content audit should drive a dedicated **Content Health** view that groups findings by pattern and lets the user enqueue fixes directly.

### 1. Pattern detection over raw warnings

Instead of showing per-article warnings, aggregate them into canonical patterns:

| Pattern | How to detect | Count in DTE audit |
|---|---|---|
| **Missing external links** | `quality_warnings` contains "Too few external links (0)" | 95 articles |
| **Keyword not in H1** | `quality_critical` / checks: primary keyword missing from H1 | ~40 articles |
| **Keyword not in first 100 words** | `quality_critical` / checks | ~40 articles |
| **Meta title too short** | `quality_warnings` regex "Meta title too short" | ~30 articles |
| **Meta title too long** | `quality_warnings` regex "Meta title too long" | ~10 articles |
| **Meta description too short/long** | `quality_warnings` regex | ~40 articles |
| **Thin content (< 2000 words)** | `word_count < 2000` AND `quality_critical` contains "too short" | 11 poor + 17 needs |
| **Duplicate target keywords** | Group articles by `target_keyword`; flag groups > 1 | 7 groups |
| **Temporal URLs** | `temporal_url == true` OR slug/title matches year/month/week patterns | 13 articles |
| **Large score drop** | Compare current audit to previous run per article | — |

These patterns should be the top-level navigation in the Content Health view, not individual articles.

### 2. Priority scoring

Each pattern instance should have a priority score derived from:

- `health_score` (lower = more urgent)
- `gsc_clicks` / `gsc_impressions` if available
- Whether the article is in `poor` vs `needs_improvement`
- Pattern severity (e.g. missing external links is an easy win; thin content is harder)

Suggested formula:

```text
priority = (100 - health_score) * 10
         + (poor ? 500 : 0)
         + log10(gsc_impressions + 1) * 50
         + pattern_weight
```

This lets the user sort by "biggest SEO upside" rather than just alphabetical article title.

### 3. One-click task creation

For each pattern, provide an **Enqueue fixes** button that creates tasks using the existing `TaskSpawner` / queue system:

| Pattern | Task type | Skill / handler |
|---|---|---|
| Missing external links | `content_fix` | `add_external_links` — deterministic, no agent needed |
| Meta title/description | `content_fix` | `rewrite_meta` — deterministic |
| Keyword not in H1/intro | `content_fix` | `align_keyword_and_h1` — agentic |
| Thin content | `content_fix` | `expand_content` — agentic |
| Duplicate keywords | `content_review` | `resolve_cannibalization` — review surface |
| Temporal URLs | `content_review` | `evergreen_temporal_pages` — review surface |

Tasks should be batched per pattern (e.g. "Fix missing external links on 95 articles") and auto-enqueued. The user should see a confirmation dialog with the count and estimated cost/time.

### 4. Article-level drill-down

Clicking a pattern row opens a table of affected articles showing:

- ID, title, slug, health, health_score
- The exact warning text
- A quick link to open the MDX file
- Checkbox to include/exclude from the batch task

### 5. Audit diff / trend view

Store each `content_audit_runs` row (already happening) and show:

- Before / after counts for good / needs_improvement / poor
- Articles that moved up or down between runs
- Patterns that are shrinking or growing

This removes the need to remember the previous run’s numbers.

### 6. Re-audit trigger

After a batch of tasks completes, the app should offer **Re-run audit** from the same view. The result updates the pattern counts and shows the delta. This closes the loop.

---

## Suggested UI layout

```
Content Health (new tab / route)
├── Summary cards: Good / Needs / Poor / Total
├── Trend chart: last N audit runs
├── Patterns table
│   ├── Pattern name
│   ├── Affected articles
│   ├── Avg health score
│   ├── Priority
│   ├── [Enqueue fixes] [Review list]
│   └── Drill-down table
└── Recent audit runs
    ├── Run date
    ├── Counts
    └── [Re-run] [View report]
```

All UI should use existing shadcn primitives (`Table`, `Badge`, `Button`, `Dialog`, `Tabs`) and follow the Tailwind v4 tokens.

---

## Architecture fit

This feature should reuse existing PageSeeds primitives as much as possible:

- **Backend:**
  - `engine::exec::content_audit::exec_content_audit` already produces the raw audit.
  - `db::content_audit` already persists runs and per-article results.
  - New helper: `engine::content_health::analyze_patterns(project_id, run_id)` to aggregate patterns.
  - New command: `commands::content_health::get_content_health(project_id)` returning patterns + affected articles.
  - Task creation goes through `engine::spawner::TaskSpawner::spawn` (or batch variant).
  - Re-audit can reuse `run-content-audit` logic or trigger a queue task of type `content_audit`.

- **Frontend:**
  - New command wrapper in `src/lib/tauri.ts`.
  - New types in `src/lib/types.ts`.
  - New component/feature directory: `src/components/content-health/`.
  - Use existing `useQueueStore` to reflect enqueued tasks.

- **Skills:**
  - Add new skill files under `.github/skills/` (or embedded defaults in `src-tauri/src/skills/`):
    - `add-external-links/SKILL.md`
    - `rewrite-meta/SKILL.md`
    - `align-keyword-and-h1/SKILL.md`
    - `expand-content/SKILL.md`
    - `resolve-cannibalization/SKILL.md`
    - `evergreen-temporal-pages/SKILL.md`

---

## Concrete next steps for implementation

1. **Add the pattern-analysis backend.**
   - Function: `analyze_patterns(conn, project_id, run_id) -> Vec<ContentPattern>`.
   - Define `ContentPattern` struct with name, severity, affected article IDs, priority scores.

2. **Add the command.**
   - `get_content_health(project_id) -> ContentHealthResponse`.
   - Thin wrapper calling the analysis function.

3. **Add the frontend view.**
   - Route: `/content-health`.
   - Summary cards + patterns table + drill-down.

4. **Add batch task creation.**
   - `enqueue_content_fixes(project_id, pattern_name, article_ids)` command.
   - Maps pattern to skill and creates one task per article (or one batch task, depending on task model).

5. **Add re-audit button.**
   - Calls existing `content_audit` execution path and refreshes the view.

6. **Add skills.**
   - Start with deterministic ones (`add-external-links`, `rewrite-meta`) since they are safest and highest ROI.

---

## Why this matters

Without this workflow, every content audit is a one-off research project. With it, PageSeeds becomes a continuous content-improvement system:

- Users see what is broken at a glance.
- They can enqueue fixes in batches.
- They can measure progress run-over-run.
- The same patterns work for any project (BrewedLate, Days to Expiry, etc.).

The Days to Expiry audit is a perfect proof case: 95 articles need external links, 7 keyword groups are duplicated, and 13 URLs are temporal. These are all detectable automatically and fixable in batches.

---

## Acceptance criteria

- [ ] Content Health view shows pattern-level breakdown, not just per-article warnings.
- [ ] Each pattern displays affected article count and average health score.
- [ ] User can sort/filter patterns by priority.
- [ ] User can enqueue a batch fix task for a pattern with one click.
- [ ] Re-audit button updates the view and shows deltas.
- [ ] Trend chart shows good/needs/poor counts over the last 5+ runs.
- [ ] Works for any project with a content audit stored in `content_audit_runs`.
