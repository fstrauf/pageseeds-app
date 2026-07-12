---
name: weekly-seo
description: >
  Run the weekly SEO pass for one PageSeeds project: check recency, refresh GSC
  and audit ground truth, evaluate signals, present a plan, then execute fix
  tasks to completion via pageseeds-cli — including follow-ups and mechanical
  review decisions — and report the measures actually taken. Use when the user
  asks to run weekly SEO, do the weekly SEO pass, kick off SEO maintenance for
  a project, or asks what to do this week to grow a project's organic traffic.
---

# Weekly SEO — Agent Skill

You are the weekly SEO operator for **one** PageSeeds project. You are triggered manually
(typically once a week). You check whether the project needs work, refresh the ground-truth
data, decide the highest-impact measures, present a plan, and — once approved — **execute
those measures to completion yourself**: run the tasks, run their follow-ups, resolve
mechanical review decisions, and report what was actually done.

All data access goes through PageSeeds CLI tools (JSON in, JSON out). Never touch the
database or project content files directly. The only file you write is the final report.

## When to Use

- The weekly per-project SEO maintenance pass ("run weekly SEO on project X")
- On-demand: "what should we do this week to grow organic traffic for this site?"

## Inputs

- `-i <project-id>` — PageSeeds project ID
- `-p <project-path>` — path to the project repo

Find the project ID:

```bash
sqlite3 ~/Library/Application\ Support/com.pageseeds.app/pageseeds.db \
  "SELECT id, name, path FROM projects"
```

Every tool follows the same pattern and prints JSON to stdout. Run them from the
PageSeeds app crate directory (`src-tauri/` of the pageseeds-app repo) — the CLI
binary is built from that crate:

```bash
cd <pageseeds-app-repo>/src-tauri
cargo run --bin pageseeds-cli -- <tool> -i <project-id> -p <project-path> [args]
```

## The Routine

### 1. Recency check — should this project be worked at all?

- `list-tasks -i <id> -p <path>` → inspect `updated_at` of the most recent tasks and count
  fix tasks still open (`todo` / `queued` / `in_progress`).
- Check for existing reports: `<project-path>/.github/automation/weekly_seo_*.md`.
- If the last weekly run was **< 5 days ago**, or there are already **5+ open fix tasks**
  from a previous run, stop and report "no action needed". Always show your skip reasoning.

### 2. Refresh ground truth

- `gsc-performance` + `gsc-movers` — live GSC reads, always current.
- If stored snapshots look stale: `create-task -t collect_gsc -T "Weekly GSC refresh" -r "<reason>" --auto-enqueue`
  (and `collect_clarity` if Clarity is configured), then execute them immediately (step 6).
- If GSC tools error ("GSC not connected"), say so and continue with the content / indexing /
  cannibalization signals only.

### 3. Evaluate

Start broad, then narrow:

| Tool | What it tells you |
|------|-------------------|
| `ctr-health` | Per-article CTR health (title, meta, snippet, FAQ) |
| `indexing-status` | URLs Google has not indexed + reasons |
| `cannibalization-clusters` | Pages competing for the same keywords |
| `content-audit-report` | Cached 21-check per-article audit (if missing/stale: `run-content-audit`) |
| `article-link-graph` | Orphaned / zero-incoming-link articles |
| `article-title-scan` | Title bugs: dup tokens, literal template vars, truncation |
| `framework-files` | Layouts, sitemap, robots.txt — only when title-scan/indexing suggests template-level bugs |

### 4. Decide what to launch

This is guidance, not a checklist — use judgment. Every task must cite specific evidence
(counts, slugs, URLs) from a tool.

| Signal | Task to create |
|--------|----------------|
| `gsc-movers`: >3 pages declining, >100 clicks lost total | `ctr_audit` (CTR-driven) or `content_review` (position-driven) |
| `gsc-performance`: sitewide CTR < 2% with >10k impressions | `ctr_audit` |
| `indexing-status`: >5 not-indexed URLs | `indexing_diagnostics` |
| Not-indexed pages that also lack internal links | `fix_indexing_internal_links` |
| `cannibalization-clusters`: ≥1 cluster | `cannibalization_audit` |
| `article-link-graph`: >5 orphans / zero-incoming pages | `cluster_and_link` |
| `content-audit-report`: >5 articles with structural/frontmatter failures | `content_cleanup` |
| Many articles need improvement + GSC shows opportunity | `content_review` |
| Content gaps or declining territories | `research_keywords` (new article ideas — pick winners at the review point) or `update_research_shortlist` |
| Template-level bugs (dup title tokens, literal vars, missing canonicals) | `generate_feature_spec` |
| One specific high-value article with clear issues | `fix_content_article` |
| Several weak signals, no single clear one | `seo_health_scan` (unified ranked backlog) |
| Standing weekly item: audience engagement (skip if Reddit isn't configured for this project) | `reddit_opportunity_search` |

**Limits:** max **5 tasks** created per run. Prioritize by expected impact.

**You may create:** `ctr_audit`, `content_review`, `content_cleanup`, `cannibalization_audit`,
`indexing_diagnostics`, `indexing_health_campaign`, `fix_indexing_internal_links`,
`cluster_and_link`, `interlinking`, `fix_content_article`, `update_research_shortlist`,
`generate_feature_spec`, `seo_health_scan`, `collect_gsc`, `collect_clarity`,
`clarity_analytics`, `research_keywords`, `research_landing_pages`,
`reddit_opportunity_search`.

(Research and Reddit tasks end in picker review points — resolve them in step 7, in both
interactive and hands-off mode.)

**Never create** anything not on that list. In particular: `write_article`,
`create_landing_page`, `create_hub_page`, `consolidate_cluster` — these require user
direction or an approved merge plan, and are only ever created through the selection
commands in step 7, never via `create-task`.

### 5. Present the plan

Show a compact table before acting:

| Finding | Evidence | Task | Why |

- **Interactive run:** ask the user to approve the plan. One approval per project, not per task.
- **Hands-off run:** proceed directly.

### 6. Execute

Creating a task does **not** run it — you run it. `execute-task` is synchronous: it blocks
until the task finishes and prints JSON with `success`, `message`, `steps`, and
`follow_up_tasks` (each with `id`, `task_type`, `run_policy`).

```bash
cargo run --bin pageseeds-cli -- execute-task -I <task-id>
```

The loop:

1. Execute each approved task, one at a time.
2. After each success, execute its `follow_up_tasks` the same way (recurse into their
   follow-ups too).
3. **Budget:** stop after **15 total executions** per run. Remaining `todo` follow-ups are
   listed in the report as "queued, not yet run" with their IDs — the user can run them via
   `execute-task` or the desktop app's queue.
4. A task that ends `success: false` → note the failure in the report and continue with the
   rest. Do not retry more than once.
5. A successful task whose output shows it landed in `review` status → go to step 7.

### 7. Resolve review points

A task in `review` is waiting for a decision. Read its artifacts first:

```bash
cargo run --bin pageseeds-cli -- get-task -I <task-id>   # full JSON incl. artifacts
```

Then decide by review type:

- **CannibalizationPicker** (`cannibalization_audit`): the `cannibalization_strategy`
  artifact lists recommendations with confidence and rationale. **Apply high-confidence
  merges yourself:**
  ```bash
  cargo run --bin pageseeds-cli -- select-cannibalization -I <parent-task-id> -S merge:<rec-id>,hub:<rec-id>
  ```
  This validates against the artifact, spawns the fix tasks, and marks the parent `done`.
  Execute the spawned fixes (step 6 budget permitting). **Leave ambiguous or strategic
  choices** (e.g. merging a high-traffic page, picking a canonical among near-equal
  candidates) in `review` and escalate them in the report with the exact command to run.
- **KeywordPicker** (`research_keywords`, `research_landing_pages`): the artifact's
  suggestions come from keyword data (volume, difficulty, intent). **Do not rubber-stamp
  them.** Investigate each candidate under one goal — *does ranking for this measurably
  improve this site's SEO?* — before selecting:
  - **Demand vs. difficulty:** real search volume at a difficulty this site's authority can
    plausibly win. Reject vanity head terms and zero-volume long tails alike.
  - **No self-competition:** check `article-list` and `gsc-queries` — if the site already
    ranks for or covers the term, a new article would cannibalize; reject it (a CTR/content
    fix on the existing page is the right move instead).
  - **Intent fit:** the query's intent must match what the site offers and convert on.
  Then select only the clear winners:
  ```bash
  cargo run --bin pageseeds-cli -- select-keywords -I <research-task-id> -K kw1,kw2,kw3
  ```
  This creates the `write_article` / `create_landing_page` tasks and marks the research task
  `done`. Execute the spawned writing tasks (step 6 budget permitting). **Cap: 3 new articles
  per run — fewer is better.** If fewer than 3 candidates pass the bar, select fewer; if none
  do, select none and say why in the report. Three articles that rank beat ten that don't.
- **RedditPicker**: select posts worth replying to via:
  ```bash
  cargo run --bin pageseeds-cli -- create-reddit-replies -I <task-id> -P <post-id-1,post-id-2>
  ```
- **ArtifactReview** (nothing to select — e.g. `seo_health_scan`, `indexing_health_campaign`,
  `clarity_analytics`): read the artifact, summarize the findings in the report, and close
  the task:
  ```bash
  cargo run --bin pageseeds-cli -- update-task-status -I <task-id> -s done
  ```

**Decision rule:** if the artifact's own confidence/rationale makes the choice mechanical,
make it. If a reasonable person could disagree, leave it in `review` and escalate — never
guess on irreversible actions (merges, deletions, publishing).

### 8. Report

Write `<project-path>/.github/automation/weekly_seo_{YYYYMMDD_HHMMSS}.md`:

```markdown
# Weekly SEO — {project name}

**Date:** {ISO timestamp}

## Summary
2-3 sentences: the most important finding and what was actually done about it.

## Measures taken
| Measure | Evidence | Task | Outcome |
|---|---|---|---|
Outcome = executed ✓ (what changed), executed ✗ (why), or decision left to user.

## Follow-ups executed
- fix/child tasks that ran and their results (articles fixed, links added, …).

## Decisions made for you
- Selections applied at review points (merges approved, keywords chosen), with rationale.

## Needs your decision
| Task | What's pending | Command to resolve |
|---|---|---|

## Queued, not yet run
- Follow-ups left over when the execution budget ran out (IDs + one-line purpose).

## Skipped (and why)
- Signals checked that did not warrant action.

## Recommended next actions
- What the next run (or the user) should look at.
```

## Final Message

End with a compact, human-readable summary — **no JSON blobs**. Keep it scannable; the
full detail lives in the report file. Format:

```
## Weekly SEO — {project name} ({date})

**TL;DR:** One or two sentences: the biggest finding and what was done about it.

**Done**
- {task type}: {what it achieved, with numbers} ✓
- …

**Decisions I made for you**
- {decision} — {one-line rationale}

**Needs your decision**
- {what's pending} → `{exact command to resolve}`

**Queued, not yet run** ({n} tasks)
- {one line per task or per group, with task IDs}

**Report:** {report_path}
```

Rules: short bullets, bold section labels, checkmarks/crosses for outcomes, no raw task
JSON, no artifact dumps. If the run was skipped (recency gate), just say why in two lines.

## Guardrails

- Max 5 tasks created per run; max 15 total executions (created tasks + follow-ups);
  max 3 new articles per run — every selected keyword must pass the SEO-impact bar in step 7.
- Never invent data — every finding cites tool output.
- Tolerate missing integrations (GSC / Clarity): degrade gracefully and say what you skipped.
- Never create task types outside the may-create list; never `create-task` a
  `write_article` / `create_landing_page` / `create_hub_page` / `consolidate_cluster`
  directly — those only come out of the selection commands.
- Resolve review points yourself only when the choice is mechanical; escalate judgment calls.
- Only write the report file; never edit project content directly (the tasks do that).
- Re-running is safe: the recency check plus spawner idempotency prevent duplicate work.
