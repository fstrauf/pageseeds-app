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
  short-description: "Weekly SEO pass via pageseeds-cli (desk-first + Path B)"
---

# Weekly SEO — CLI Operator Bible (desk-first + Path B)

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
| 5 | **May-create list only** (below). Never `create-task` for `write_article`, `create_landing_page`, `create_hub_page`, `consolidate_cluster` — those come from selection after review. Path B write uses `write-context` / `write-submit`; Path B merge uses `merge-context` / `merge-submit`; Path B fix uses `fix-context` / `fix-submit` when available. |
| 6 | **Evidence:** every task / major finding cites tool output (counts, slugs, URLs). |
| 7 | **Reviews:** mechanical only; escalate judgment (high-traffic merges, strategic keywords). |
| 8 | **Report only file write:** `weekly_seo_{YYYYMMDD_HHMMSS}.md` under `<project-path>/.github/automation/`. |
| 9 | **Missing integrations:** GSC/Clarity/Reddit fail → degrade and say so; never fake data. |

### May-create via `create-task`

`fix_content_article` (**always** `-S`/`--slug` — never bare), `content_review`,
`research_keywords`, `research_landing_pages`, `indexing_diagnostics`,
`indexing_health_campaign`, `fix_indexing_internal_links`, `content_cleanup`,
`cluster_and_link`, `interlinking`, `ctr_audit`, `cannibalization_audit`,
`update_research_shortlist`, `generate_feature_spec`, `seo_health_scan`,
`collect_gsc`, `collect_clarity`, `clarity_analytics`, `reddit_opportunity_search`.

**Prefer when desk data already supports the action:** `fix_content_article -S`,
`research_keywords`, `research_landing_pages`, indexing tasks.
Do **not** invent work via soft audits when desk reads suffice.
**Demote for weekly CLI:** `ctr_audit` — see [CTR / content fix policy](#ctr--content-fix-policy).


**Not for weekly strategy:** `content_review` — desktop UI / unattended only (#139).
Do **not** `create-task content_review` for weekly explore. Desk → judgment → hard actions.

### CTR / content fix policy

CLI weekly best-path for low CTR is **desk-selected targeted fixes**, not a full
`ctr_audit` fan-out.

1. **Best path (default):** Identify high-impression / low-CTR URLs from desk —
   `site-overview` (top_pages + high-impr low-CTR hints), `articles` with min
   impressions, `gsc-performance`, then `article -S` + `gsc-queries` on
   candidates. Create **targeted** `fix_content_article` with `-S <slug>` for
   top waste URLs only (counts toward ≤5 creates). Cite impressions / CTR /
   position evidence in `-r`.
2. **Do NOT** enqueue `ctr_audit` as the default weekly action. Full `ctr_audit`
   has `BackendAuto` and spawns many `fix_ctr_article` children — burns the ≤15
   execution budget on title-only / no-op work.
3. **`ctr_audit` is rare/optional:** Only when desk cannot narrow candidates and
   you explicitly need the productized CTR pipeline (still honor budgets). If
   you create it, note why and expect many auto-spawned children — prefer fewer
   pages via desk instead.
4. **UI vs CLI:** Desktop/UI unattended automation may still AutoEnqueue
   `ctr_audit` and BackendAuto-spawn children — intentional product path. This
   skill is the **CLI operator best-path**; do not flip lifecycle metadata.

---


## Explicit bans (CLI best-path)

| Ban | Do instead |
|-----|------------|
| Nested weak write: `execute-task write_article` on happy path | Path B: `write-context` → session MDX → `write-submit` |
| Nested weak merge: `execute-task consolidate_cluster` on happy path | Path B: `merge-context` → session MDX → `merge-submit` |
| `fix_content_article` for length / min_word_count recovery after Path B write failure | Expand draft + re-run `write-submit` |
| `content_review` as strategy brain (`create-task content_review` for weekly explore) | Desk → agent judgment → hard actions (#139) |
| Soft clusters (`cannibalization-clusters`) as truth / merge authority | Hard evidence only (same query on 2+ URLs, exact keyword dupe, etc.) |
| Full `ctr_audit` spawn by default (#140) | Desk → targeted `fix_content_article -S`; scoped `ctr_audit` only when needed |
| Nested `execute-task` LLM for write/fix/merge when Path B tools exist | Path B package → session edit → submit |

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
| Path B merge | `merge-context` / `merge-submit` — outer-agent merge after approved keep+redirects (preferred CLI path) |

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
| High impressions + low CTR + weak title/meta | Desk → targeted `fix_content_article` (`-S`) for top waste URLs; Path B fix when available. **Not** full `ctr_audit` first (see CTR policy); **not** `content_review` as strategy brain |
| Same query on **2+ URLs** (`gsc-queries`) or same intent competing | Optionally `cannibalization_audit` **only with hard evidence**; never treat soft clusters as ground truth |
| Many not-indexed | Indexing diagnostics / internal links |
| Orphans / weak links | `cluster_and_link` / `interlinking` |
| Structural MDX issues | `content_cleanup` / `content_review` |
| Template/title systemic bugs | `generate_feature_spec` + evidence |
| Quiet site + thin backlog | `research_keywords` / `research_landing_pages` |
| Desk insufficient across levers | Optional `seo_health_scan` (not default) |
| Reddit configured + capacity | `reddit_opportunity_search` |


### Research strategy package (#141)

Session owns themes/seeds; CLI owns Ahrefs pull:

```bash
# Optional: session proposes seeds from desk + research-shortlist
pageseeds-cli research-pull -i <id> -p <path> --seeds "theme one,theme two" ...
# → candidates for select-keywords / write Path B
```

Prefer this over relying solely on nested research_seed_extraction when tools exist.

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
- Approved merge → `consolidate_cluster` tasks for provenance — **complete via Path B merge**
  (`merge-context` / write merged MDX / `merge-submit`), not `execute-task consolidate_cluster`
- Desktop nested writer still auto-spawns quality review + cluster link on success
- `content_review` may spawn fixes / feature-spec (execute what appears)

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
  **After approved merges, use Path B merge** (below) — do **not**
  `execute-task` the spawned `consolidate_cluster` tasks on the happy path.
- **KeywordPicker:** no rubber-stamp. Check demand/difficulty, self-competition
  (`articles` / `gsc-queries`), intent. Prefer non-avoid / `differentiate` /
  `target`. Then `select-keywords -I <id> -K kw1,kw2` — max **3**, fewer better.
  **After select-keywords, use Path B for articles** (below) — do **not**
  `execute-task` the spawned `write_article` tasks.
- **ContentReviewPicker:** desktop/unattended only — not weekly strategy. If inherited, dispose mechanically; do not start new `content_review` for weekly explore. `select-content-review -I <parent> -P id1,id2`
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


### Path B — CLI fix package (when tools available)

Preferred for targeted content/CTR edits with full file context:

```bash
pageseeds-cli fix-context -i <id> -p <path> -S <slug> -k content|ctr [-g goals]
# session agent edits full file using package
pageseeds-cli fix-submit -i <id> -p <path> -S <slug> -k content|ctr [--file mdx]
```

Until tools land: `create-task fix_content_article -S <slug>` + `execute-task` with desk evidence.
Do **not** use `content_review` as middleman. Do **not** use fix_content for Path B write length recovery.

### Path B — CLI merge package (happy path after approved consolidate)

`select-cannibalization` / create-from-approved still create `consolidate_cluster`
tasks for provenance. For **CLI best-path**, complete merges via merge-context +
session prose + merge-submit — **not** nested `execute-task consolidate_cluster`
(weak global providers run irreversible nested draft_patch).

```bash
# 1. Package (deterministic — no LLM)
pageseeds-cli merge-context -i <id> -p <path> \
  -I <consolidate-task-id>
# → JSON: plan, keep + redirects with FULL MDX, outlines, soft GSC,
#   skill_name + skill_content (merge-content), keeper_file, min 400 words,
#   requires_human_confirm, instructions

# Or without a task: -K /blog/keep -R /blog/src-a,/blog/src-b
# Or: --keep-id <id> --redirect-ids <id,id,...>

# 2. Session agent writes complete merged MDX to keeper_file
#    (preserve unique tables/FAQs/examples from redirects; match keeper tone)

# 3. Submit until ok (high-traffic needs -y)
pageseeds-cli merge-submit -i <id> -p <path> \
  -I <consolidate-task-id> [-y]
# → ok:false + checks → fix keeper and resubmit (no redirects applied yet)
# → ok:true → redirects.csv, inbound rewrites, sources redirected, task done
```

| Rule | Path B merge |
|------|----------------|
| **Do** | `merge-context` → write MDX to `keeper_file` → `merge-submit` until `ok` |
| **Ban** | `execute-task consolidate_cluster` on the happy path |
| **Confirm** | When `requires_human_confirm` (clicks ≥ 50 or impressions ≥ 1000), pass `-y` only after human OK |
| **Budget** | Each `merge-submit` attempt counts toward the **15** execution budget |
| **Provenance** | consolidate tasks may still exist; Path B completes them via submit |

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
- Installed `pageseeds-cli` only — never product `cargo run`.  
- No product source edits. Missing tools → report gap.  
- Max 5 creates / 15 executions / 3 new articles.  
- Low CTR → desk-selected `fix_content_article` (`-S`); not default full `ctr_audit`.  
- Evidence required; no invented data; no illegal create-task types.  
- Soft clusters **not** ground truth / merge authority.  
- Mechanical reviews only; only write the weekly report file.  
- Idempotent re-runs: recency + spawner keys.

---

## Design note

**Desk model (epic #117):** ~10-tool mental model — Site State reads
(`site-overview` / `articles` / `article` + GSC) then few hard actions. Soft
clusters and specialist audits remain available but are **optional**, not the
weekly spine. CLI weekly CTR: desk-ranked waste URLs → targeted fixes; full
`ctr_audit` BackendAuto fan-out is the UI/unattended path, not CLI default.

**Dual-path freshness:** until `refresh_ground_truth` exists, use `collect_gsc`
and/or live `gsc-*` then desk reads. Prefer desk over soft audits when both
answer the same question.

**MCP (#92):** mount **desk tools first**; skill = operator policy. Tighten soft
guidance if agents thrash — not hard rails first.
