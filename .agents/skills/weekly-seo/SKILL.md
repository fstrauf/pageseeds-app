---
name: weekly-seo
description: >-
  Run the weekly SEO pass for one PageSeeds project via pageseeds-cli
  (desk reads → ≤5 actions → report). Use when the user wants weekly SEO,
  SEO maintenance, organic growth this week, or /weekly-seo.
  Operator only — never edit pageseeds-app source.
when-to-use: >-
  Triggers on "/weekly-seo", "weekly SEO", "run weekly SEO", "SEO pass",
  "SEO maintenance", "what should we do this week for organic traffic",
  "grow this site's SEO".
argument-hint: "[project-name-or-id]"
user-invocable: true
metadata:
  short-description: "Weekly SEO pass via pageseeds-cli (desk-first)"
---

# Weekly SEO — Agent Skill (desk-first)

> **Desk model (epic #117):** explore **Site State** (GSC + catalog) then act.
> Soft audits are optional — not the weekly spine, not ground truth.

## Invocation

```
/weekly-seo
/weekly-seo coffee
/user:weekly-seo
```

Prefer the **customer project** (cwd outside `pageseeds-app`). Requires
`pageseeds-cli` on PATH (`pnpm install:cli` / `./scripts/install-cli.sh`).

You are the weekly SEO operator for **one** project. Find the highest-impact
organic growth opportunity, propose ≤5 measures, execute via PageSeeds tasks —
not by editing content or product source yourself.

| Layer | Role |
|-------|------|
| **Capability** | `pageseeds-cli` JSON tools (≈ MCP surface) |
| **Policy** | This skill — budgets, lifecycle, report, isolation |
| **Agent** | You — choose tools within hard rails |
| **Product source** | **Out of scope** — never patch `pageseeds-app` |

---

## When to use

- Weekly per-project SEO maintenance
- On-demand: “what should we do this week for organic traffic?”

## Separation of concerns (mandatory)

| Role | Workspace | May write |
|------|-----------|-----------|
| **This skill** | Customer project / neutral cwd | Only weekly report under project automation |
| **pageseeds-cli** | N/A (binary on PATH) | Tasks/DB/content **via tools only** |
| **Product engineer** | `pageseeds-app` (separate session) | App source / PRs |

If the session is inside the product repo (`pageseeds-app` + editing Rust/TS),
**stop** and re-run with only the customer project open. Missing CLI features are
product gaps — report them; do not implement mid-run.

---

## Inputs

- `-i <project-id>` — PageSeeds project ID  
- `-p <project-path>` — absolute path to the **customer** repo  

```bash
sqlite3 ~/Library/Application\ Support/com.pageseeds.app/pageseeds.db \
  "SELECT id, name, path FROM projects"
```

State `id` / `name` / `path` once. Pass `-i` / `-p` on every tool that needs them.

```bash
pageseeds-cli <tool> -i <project-id> -p <project-path> [args...]
```

Use the installed binary from any directory. Never `cd` into `pageseeds-app` or
`cargo run` for this skill. All tools print **JSON**. Never invent numbers.

CLI → same SQLite as the desktop app. UI need not be running.

---

## Hard rails (always)

Breaking these fails the run.

| # | Rule |
|---|------|
| 1 | **CLI only** for data/tasks (+ the report file). No direct DB writes, no hand-editing MDX. |
| 2 | **No product source edits** under `pageseeds-app` / `src-tauri` / product manifests. |
| 3 | **Missing capability → escalate**, don’t implement. Document gap; work around or stop that branch. |
| 4 | **Budgets:** ≤**5** creates · ≤**15** executions · ≤**3** new articles from keyword selection. |
| 5 | **May-create list only** (below). Never `create-task` for `write_article`, `create_landing_page`, `create_hub_page`, `consolidate_cluster` — those come from selection after review. Path B uses `write-context` / `write-submit`, not `create-task write_article`. |
| 6 | **Evidence:** every task / major finding cites tool output (counts, slugs, URLs). |
| 7 | **Reviews:** mechanical only; escalate judgment (high-traffic merges, strategic keywords). |
| 8 | **Report only file write:** `weekly_seo_{YYYYMMDD_HHMMSS}.md` under `<project-path>/.github/automation/`. |
| 9 | **Missing integrations:** GSC/Clarity/Reddit fail → degrade and say so; never fake data. |

### May-create via `create-task`

`fix_content_article` (**always** `-S`/`--slug` — never bare),
`research_keywords`, `research_landing_pages`, `indexing_diagnostics`,
`indexing_health_campaign`, `fix_indexing_internal_links`, `content_cleanup`,
`cluster_and_link`, `interlinking`, `ctr_audit`, `cannibalization_audit`,
`update_research_shortlist`, `generate_feature_spec`, `seo_health_scan`,
`collect_gsc`, `collect_clarity`, `clarity_analytics`, `reddit_opportunity_search`.

`content_review` not on may-create — UI/unattended only; never create from this skill.

**Prefer when desk data already supports the action:** `fix_content_article`,
`research_keywords`, `research_landing_pages`, indexing tasks.
Do **not** invent work via soft audits when desk reads suffice.

### Hard ban: `content_review` is not this skill's strategy brain

**Do not** `create-task -t content_review` for weekly explore / strategy /
"what's wrong with the site" investigation. That nested task is the **desktop
UI / unattended product umbrella** — on GrokCli/KimiCli backends it falls back
to scripted recommend and **degrades** strategy vs desk tools.

| Path | Use |
|------|-----|
| **Weekly CLI (this skill)** | Desk reads → agent judgment → hard actions |
| **Desktop / unattended product** | `content_review` remains valid when a nested investigation task is wanted |

**CLI dispose path (UI not required):** when desk evidence is enough, create
fixes directly:

```bash
pageseeds-cli create-task -i <id> -p <path> \
  -t fix_content_article -S <url-slug> \
  -T "Fix content: <title>" -r "<reason citing desk evidence>"
pageseeds-cli execute-task -I <task-id>
```

No ContentReviewPicker, no nested investigate step. Desk → `fix_content_article`
is a complete weekly path. Optionally use scoped specialist audits
(`ctr_audit`, `cannibalization_audit`, indexing) when the problem is already
scoped — still **not** `content_review` as the brain.

---

## Soft guidance (default path)

```text
recency → refresh ground truth (if stale) → site-overview
  → articles / article / gsc-queries → ≤5 actions → report
```

Reorder/deepen when a clear anomaly appears. Still honor hard rails and plan
before mass create (interactive: approval; hands-off: short plan then go).

### A. Recency / load

```bash
pageseeds-cli list-tasks -i <id> -p <path>
```

- Latest `weekly_seo_*.md` under automation.
- **Skip run** only if last weekly **&lt; 5 days** *or* **≥ 5** fix-like tasks
  open (`todo`/`queued`/`in_progress`) **and** user did not force. State why.
- Override: “run anyway” → continue.

### B. Refresh ground truth (if stale)

There is **no** `refresh_ground_truth` CLI yet (dual-path until it lands).

| Need | Do |
|------|-----|
| Live demand / deltas | `gsc-performance`, `gsc-movers`, `gsc-queries` (cheap truth) |
| Stale snapshots / desk cache | `create-task -t collect_gsc` then **`execute-task` this run** if needed |
| Clarity (if configured) | same pattern with `collect_clarity` |

If GSC disconnected: continue on catalog/indexing tools only; note it.

### C. Desk exploration (primary)

**Goal:** *What is the highest-leverage SEO problem/opportunity this week?*

#### Primary desk tools (explore these first)

| Tool | Role |
|------|------|
| `site-overview` | Compact weekly desk entry: totals, top pages, movers, freshness, hints |
| `articles` | GSC-aware catalog list (filters: status, min impressions, period) |
| `article` | Full package for one slug: frontmatter, body outline, top queries, neighbors (`-S`/`--slug`) |
| `gsc-performance` | Site/page traffic, CTR, impressions (`-l`, default 50, max 200) |
| `gsc-movers` | Gained/lost clicks 30d vs prior (`-l`, default 30, max 200) |
| `gsc-queries` | Query-level demand; page filter `-u <url>` |
| `list-tasks` / `get-task` | Open work, artifacts, review state |
| `create-task` / `execute-task` | Act within may-create + budgets |
| Selection cmds | `select-keywords`, `select-cannibalization`, `select-content-review`, `create-reddit-replies`, `update-task-status` |
| Path B write | `write-context` / `write-submit` — outer-agent prose after keyword selection (preferred CLI path) |

#### Optional / secondary (NOT ground truth, not required path)

| Tool | Note |
|------|------|
| `cannibalization-clusters` | Soft TF-IDF clusters — **fail open** on mono-niche; **not merge authority** |
| `ctr-health` | Productized composite — prefer impressions/CTR from desk + `gsc-queries` |
| `seo_health_scan` (task) | Optional backlog only when desk data is insufficient |
| `content-audit-report` / `run-content-audit` | Optional deep structural checks |
| `indexing-status`, `article-title-scan`, `article-body-hash`, `article-link-graph`, `framework-files`, `research-shortlist`, `article-quality-reviews`, `score-zero-impression-articles`, `article-list` / `article-frontmatter` | Use when desk points there |

**Exploration budget:** prefer **≤ ~25** tool calls before locking a plan.
Stop early when the story is clear; do not thrash the same tool without a new hypothesis.

#### How to explore

1. **Wide:** `site-overview` (+ `gsc-movers` / `gsc-performance` if needed).  
2. **Catalog:** `articles` for filters (high impressions, low CTR, status).  
3. **Deep:** `article -S <slug>` + `gsc-queries -u <url>` on top candidates.  
4. **Act** only with evidence; gap growth → research (below).

#### Soft hints (priors — CTR & cannibalization emerge from desk data)

| Pattern from desk | Action preference |
|-------------------|-------------------|
| High impressions + low CTR + weak title/meta | `fix_content_article -S <slug>` from desk evidence; optionally `ctr_audit` if you need that pipeline — **not** `content_review` |
| Same query on **2+ URLs** (`gsc-queries`) or same intent competing | Optionally `cannibalization_audit` **only with hard evidence**; never treat soft clusters as ground truth |
| Many not-indexed | Indexing diagnostics / internal links |
| Orphans / weak links | `cluster_and_link` / `interlinking` |
| Structural MDX issues | `content_cleanup` (not strategy; not `content_review`) |
| Template/title systemic bugs | `generate_feature_spec` + evidence |
| Quiet site + thin backlog | `research_keywords` / `research_landing_pages` |
| Desk insufficient across levers | Optional `seo_health_scan` or scoped specialist audit (not default; **not** nested `content_review`) |
| Reddit configured + capacity | `reddit_opportunity_search` |

**Research:** generative. Prefer `research-shortlist` health
(`promising` / `depleted` / `unproven`). Never claim “no gaps found” if research
did not run — say **skipped** + why + last research date.

Avoid-heavy keyword pickers (AIO-blocked heads, mostly `winnability: avoid`):
prefer shortlist **promising** themes/seeds and re-run research; pick only
`differentiate` / `target` rows when possible. Residual avoids = last resort.

#### Known limits (branch, don’t dead-end)

| Limit | Do *this run* if budget allows |
|-------|--------------------------------|
| `gsc-movers` ~30 rows | Default limit — raise `-l 100`/`200` or cross-check `gsc-performance` |
| Empty `gsc_page_daily` | Run `collect_gsc` + execute if day-level series needed; movers use live API windows |
| No SERP scrape tool | Infer from position deltas + query mix only; use research for gaps |
| Top 3–4 URLs are the problem | Deep-dive each with `article` + `gsc-queries` **now**, then fix tasks |

**Anti-pattern:** parking “deep-dive later” when tools + budgets allow it now.

---

## D. Plan

| Finding | Evidence (tool + numbers/slugs) | Proposed task | Why this week |

- Interactive: one approval per project. Hands-off: state plan, proceed.  
- Max **5** creates; impact first.

---

## E. Execute

```bash
pageseeds-cli create-task -i <id> -p <path> \
  -t <task_type> -T "<title>" -r "<reason citing evidence>"
pageseeds-cli execute-task -I <task-id>
```

**`fix_content_article` always needs a slug:**

```bash
pageseeds-cli create-task -i <id> -p <path> \
  -t fix_content_article -S <url-slug> \
  -T "Fix content: <title>" -r "<reason citing evidence>"
```

Bare create without `-S` is rejected. CLI attaches `recommendations_{article_id}`
(SERP categories: title / description / h1 / intro).

Loop: execute one-by-one → follow-ups within budget → stop at **15** → note
leftovers → fail once continue (≤1 retry) → resolve `review` mechanically.

### Expected auto follow-ups

- Selection → `write_article` tasks created for provenance — **complete via Path B**
  (`write-context` / write MDX / `write-submit`), not `execute-task write_article`
- Path B `write-submit` → marks write task done + spawns `cluster_and_link`
- Desktop nested writer still auto-spawns quality review + cluster link on success
- If a pre-existing `content_review` is already open (not created by this skill),
  resolve mechanically via `select-content-review` and execute spawned fixes —
  still do **not** create new `content_review` as strategy

### Quality gate

Failed `review_article_quality` → create `fix_content_article` **with** `-S`
if none exists, then execute (counts toward 15).

**Do not** use `fix_content_article` to recover Path B min_word_count / thin-body
failures — expand the draft and re-run `write-submit` instead.

### Review resolution

```bash
pageseeds-cli get-task -I <task-id>
```

- **CannibalizationPicker:** mechanical high-confidence only —
  `select-cannibalization -I <parent> -S merge:<id>,hub:<id>`; escalate ambiguous.
  Soft clusters are **not** merge authority.
- **KeywordPicker:** no rubber-stamp. Check demand/difficulty, self-competition
  (`articles` / `gsc-queries`), intent. Prefer non-avoid / `differentiate` /
  `target`. Then `select-keywords -I <id> -K kw1,kw2` — max **3**, fewer better.
  **After select-keywords, use Path B for articles** (below) — do **not**
  `execute-task` the spawned `write_article` tasks.
- **ContentReviewPicker:** `select-content-review -I <parent> -P id1,id2`
- **RedditPicker:** `create-reddit-replies -I <id> -P id1,id2`
- **ArtifactReview:** summarize; `update-task-status -I <id> -s done`

### Path B — CLI write package (happy path after `select-keywords`)

`select-keywords` still creates `write_article` tasks for provenance / queue
tracking. For **CLI best-path**, complete those tasks via write-context +
session prose + write-submit — **not** nested `execute-task write_article`
(weak global providers produce thin single-shot MDX).

```bash
# 1. Package (deterministic — no LLM)
pageseeds-cli write-context -i <id> -p <path> \
  -I <research-task-id> -K "<keyword>"
# → JSON: content_brief, target_file, target_path, publish_date,
#   skill_name + skill_content (content-write craft rules),
#   min_words 800, target_words 1200, write_task_id (if any)

# 2. Session agent writes full MDX to target_file using skill_content + brief
#    (min 800 words, proper frontmatter title/description/slug/date, H1, links)

# 3. Submit until ok (or give up within execution budget)
pageseeds-cli write-submit -i <id> -p <path> \
  -f <target_file> [-I <write_task_id>] [-K "<keyword>"]
# → ok:false + checks → expand and resubmit (file kept)
# → ok:true → article registered; write_article marked done; cluster_and_link spawned
```

| Rule | Path B |
|------|--------|
| **Do** | `write-context` → write MDX to `target_file` → `write-submit` until `ok` |
| **Ban** | `execute-task write_article` on the happy path |
| **Ban** | `fix_content_article` for min_word_count / length recovery — expand and **resubmit** instead |
| **Budget** | Each `write-submit` attempt counts toward the **15** execution budget |
| **Provenance** | `select-keywords` may still spawn `write_article`; Path B completes them via submit |

---

## F. Report

`<project-path>/.github/automation/weekly_seo_{YYYYMMDD_HHMMSS}.md`

```markdown
# Weekly SEO — {project name}

**Date:** {ISO timestamp}

## Summary
2–3 sentences: biggest finding and what was done.

## Exploration path
Desk path chased, detours, what you skipped (and why).

## Measures taken
| Measure | Evidence | Task | Outcome |

## Follow-ups executed
…

## Decisions made for you
…

## Needs your decision
| Task | What's pending | Command to resolve |

## Queued, not yet run
…

## Skipped (and why)
- Including research skip vs “not run” honesty rule.

## Product / CLI gaps (if any)
- e.g. no `refresh_ground_truth` yet — used collect_gsc / live gsc-* dual-path

## Recommended next actions
…
```

### Final user message (no JSON dumps)

```
## Weekly SEO — {project name} ({date})

**TL;DR:** …

**Exploration:** one line (desk path)

**Done**
- …

**Decisions I made for you**
- …

**Needs your decision**
- … → `command`

**Queued, not yet run** (n)
- …

**Report:** {path}
```

---

## Guardrails (summary)

- Desk-first exploration; hard rails **mandatory**.  
- **Ban** nested `content_review` as weekly strategy brain — desk → hard actions.  
- CLI dispose: `fix_content_article -S <slug>` when desk evidence is enough (UI not required).  
- Installed `pageseeds-cli` only — never product `cargo run`.  
- No product source edits. Missing tools → report gap.  
- Max 5 creates / 15 executions / 3 new articles.  
- Evidence required; no invented data; no illegal create-task types.  
- Soft clusters **not** ground truth / merge authority.  
- Mechanical reviews only; only write the weekly report file.  
- Idempotent re-runs: recency + spawner keys.

---

## Design note

**Desk model (epic #117 / #139):** ~10-tool mental model — Site State reads
(`site-overview` / `articles` / `article` + GSC) then few hard actions.
`content_review` is **not** the weekly CLI brain (degrades under GrokCli/KimiCli
scripted fallback); keep it for desktop UI / unattended product flows only.
Soft clusters and specialist audits remain available but are **optional**, not
the weekly spine. No separate `propose-fixes` CLI — agent judgment + direct
`create-task -t fix_content_article -S <slug>` is the dispose path.

**Dual-path freshness:** until `refresh_ground_truth` exists, use `collect_gsc`
and/or live `gsc-*` then desk reads. Prefer desk over soft audits when both
answer the same question.

**MCP (#92):** mount **desk tools first**; skill = operator policy. Tighten soft
guidance if agents thrash — not hard rails first.
