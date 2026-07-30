---
name: seo-program-review
description: >-
  Monthly (or on-demand) SEO program rebalance for one PageSeeds customer
  project: deep-dive product repo + CLI desk + PostHog, then review/update
  seo_program.yaml and project.yaml strategy gates. Use for /seo-program-review,
  north-star review, 90-day SEO plan, cluster rebalance, or program board update.
  Operator only — never edit pageseeds-app product source mid-run.
when-to-use: >-
  Triggers on "/seo-program-review", "SEO program review", "north star review",
  "rebalance SEO strategy", "update seo_program", "monthly SEO plan",
  "review primary backlog", "90-day SEO review".
argument-hint: "[project-name-or-id]"
user-invocable: true
metadata:
  short-description: "Monthly SEO program + strategy rebalance (YAML SOT)"
---

# SEO program review — monthly rebalance (skill + CLI desk)

> **Layering:** `project.yaml` = theme gates (what we may expand).  
> `seo_program.yaml` = ops (goal, metrics, mode, primary / harvest / tools /
> prune queues).  
> This skill **produces/updates** those files. `/weekly-seo` **consumes** them.  
> Schema: [docs/SEO_PROGRAM.md](../../../docs/SEO_PROGRAM.md).

## Invocation

```
/seo-program-review
/seo-program-review days_to_expiry
/seo-program-review coffee
```

Prefer the **customer project** cwd (outside `pageseeds-app`). Requires
`pageseeds-cli` on PATH (prebuilt install preferred).

You are the **program manager for SEO ops** for **one** project — not a weekly
executor and not a product engineer. Deep-dive product + desk, then write
structured YAML + a review report.

| Layer | Role |
|-------|------|
| **Capability** | `pageseeds-cli` JSON desk + strategy tools |
| **Behavioral** | PostHog MCP (conversion north star) |
| **Product truth** | Customer repo (routes, CTAs, tools, onboarding) — **read** |
| **Policy** | This skill |
| **Writes** | `seo_program.yaml`, proposed/actual `project.yaml` strategy edits, review report |
| **Out of scope** | `pageseeds-app` Rust/TS product source; mass MDX rewrites; inventing Primary outside strategy |

---

## When to use

- ~**30 days** since `last_reviewed_at` (or user asks)
- Product messaging / free-tool surface / pricing changed
- Weekly runs keep shipping off-Primary volume while Primary backlog is open
- First-time seed of `seo_program.yaml` for a project

**Not** a substitute for `/weekly-seo`. Do not execute ≤5 weekly creates unless
the user explicitly asks to chain a weekly pass after review.

---

## Separation of concerns

| Role | Workspace | May write |
|------|-----------|-----------|
| **This skill** | Customer project | `seo_program.yaml`, `project.yaml` (strategy fields only), review report under automation |
| **pageseeds-cli** | Binary on PATH | Tasks/DB **only if user asks** for a chained weekly — default is **no** create-task spam |
| **Product engineer** | App source (separate session) | Webapp CTAs, import, pricing UI |

If session is inside `pageseeds-app` product crates intending code edits: **stop**
and re-open the customer project. Strategy file edits in the content repo are fine.

---

## Hard rails

| # | Rule |
|---|------|
| 1 | **No mass content rewrite.** Do not Path B fix 50 slugs here. Produce queues for weekly. |
| 2 | **No second Primary list.** `primary_backlog` keywords ⊆ or align with `project.yaml` `search_keywords.primary`. |
| 3 | **YAML is SOT for ops.** Do not leave the living plan only in the review markdown. |
| 4 | **Evidence** for every cluster status change and every queue add (GSC numbers, product route, PostHog). |
| 5 | **Never invent** GSC/PostHog metrics. Missing integration → degrade + say so. |
| 6 | **No pageseeds-app product PRs** mid-run. Product gaps go under Needs attention. |
| 7 | **Report file:** `seo_program_review_{YYYYMMDD_HHMMSS}.md` under `.github/automation/`. |
| 8 | Prefer **human confirm** before demoting high-impression pillars to LEGACY (use MAINTAIN). |
| 9 | Do **not** use `content_review` task as strategy brain. Desk + product read + judgment. |
| 10 | **Noindex never agent-executed.** Every `prune_queue` noindex row must have `confirm: required` and appear under report **Needs your decision**. Skills/CLI never bulk deindex. |

---

## Inputs / setup

```bash
pageseeds-cli setup --path . --yes   # if needed
pageseeds-cli list-projects
pageseeds-cli strategy -p .
pageseeds-cli project-config-status -p .
```

Resolve project id + path. All CLI tools print **JSON**.

Files:

| Path | Required |
|------|----------|
| `.github/automation/project.yaml` | Yes (migrate if legacy MD only) |
| `.github/automation/seo_program.yaml` | Create if missing (schema v1) |
| `.github/automation/project.md` | Read-only brand context |

---

## Procedure

### 1. Recency

- Read existing `seo_program.yaml` if present (`last_reviewed_at`, queues, mode).
- Latest `seo_program_review_*.md` and last 2–4 `weekly_seo_*.md`.
- If last review **&lt; 14 days** and user did not force: summarize “still fresh”
  and only patch if they insist. Override: “run anyway.”

### 2. Product deep dive (customer repo — read only)

Spend focused exploration on **what the product actually is today**:

| Area | Look for |
|------|----------|
| Public routes | `app/**/page.tsx` (or framework equivalent): pricing, sign-up, tools, trackers, calculators, scanners, IBKR/Flex |
| Free tools inventory | Hub + each tool’s job-to-be-done |
| Onboarding / import | Flex, broker connect, CSV — activation path after signup |
| CTAs on marketing | What blog should send people to |
| Positioning | README / marketing docs — buyer, not student |

Capture a short **product map** for the report (routes + one-line purpose).
Do **not** patch webapp code.

### 3. CLI desk package (deterministic)

Run (or re-use fresh outputs):

```bash
pageseeds-cli strategy -i <id> -p <path>
pageseeds-cli research-context -i <id> -p <path>
pageseeds-cli site-overview -i <id> -p <path>
pageseeds-cli gsc-performance -i <id> -p <path> -l 50
pageseeds-cli gsc-movers -i <id> -p <path> -l 50
# optional:
pageseeds-cli articles -i <id> -p <path> -m 200
```

Classify roughly:

- **Primary / tool / commercial** click share vs **edu / tax / beginner**
- Open Primary coverage (keyword → live slug or gap)
- Harvest candidates: high impr, weak product path / CTR
- Strategy load: `content_strategy.status` must not be silently empty

### 4. PostHog (conversion north star)

Same fleet map / `posthog.yaml` rules as weekly-seo. Prefer:

- Blog → `signup_started` / signup completed (if events exist)
- Paths involving `/sign-up`, `/pricing`, tools, import
- **Warn** if MCP/map blocked — do not invent funnels

GSC remains demand authority; PostHog is **success metrics** for the program.

### 5. Decide (human-facing plan before write)

Produce a short plan:

1. Goal statement (keep/edit)
2. Metrics list (keep/edit)
3. Cluster status changes in `project.yaml` (table: before → after + why)
4. Primary list tweaks / `do_not_expand` additions (only with evidence of drift)
5. Rebuilt `primary_backlog` / `harvest_queue` / `tools_queue` / `prune_queue` (≤15 open items each — thin board)
6. `current_mode` + `mode_mix_this_month` for the next 30 days
7. Product gaps that SEO cannot fix (activation, UI)

Interactive: get approval on cluster demotions / Primary cuts. Hands-off: state
plan then write, flag aggressive LEGACY demotions under Needs decision.

### 5b. Prune scan (LEGACY / do_not_expand inventory → per-URL actions)

Cross strategy LEGACY clusters + `do_not_expand` keywords with desk package:

- `strategy` (cluster statuses, `do_not_expand`)
- `gsc-performance` / `site-overview` / `articles` (impr, clicks, indexing)
- optional dead-weight / winnability scores when available
  (`score-zero-impression-articles --from-cache`) — secondary only

Emit ≤15 **open** `prune_queue` rows. Prefer high-impr blocked territory and
thin drift (e.g. tax drafts on `do_not_expand`, income/dividend LEGACY with
near-zero clicks).

For each row choose:

- `action: merge_into:<keeper-slug>` when a clear keeper exists (hard cannibal /
  thin dupe / same-intent stronger URL)
- `action: noindex` when no keeper and page is pure drag — **must** set
  `confirm: required` and list under report **Needs your decision**

Evidence required on every row (GSC window string). Preserve useful
`in_progress` / `done` / `measuring` rows when rewriting YAML.

**Hard bans for this step:**

- No mass MDX rewrites
- No CLI noindex / bulk deindex execution
- No new task types

### 6. Write YAML

#### `seo_program.yaml`

- Full schema v1 rewrite is OK if queues are stale; preserve useful `measuring`
  rows (including merge prune rows still measuring after merge-submit)
- Include rebuilt `prune_queue` (≤15 open; every noindex has `confirm: required`)
- Set `last_reviewed_at` to today (ISO date)
- Set `current_mode` to the **next** weekly default
- Keep `schema_version: 1`

#### `project.yaml` (strategy only)

Allowed edits:

- `search_keywords.primary` / `problem` / `audience` / `do_not_expand`
- `clusters[].status` and keywords
- **Not** unrelated Reddit mass rewrites unless clearly wrong

After edits:

```bash
pageseeds-cli strategy -p .
pageseeds-cli project-config-status -p .
# confirm content_strategy.status ok/partial via research-context if needed
```

Traffic rule: high-impression money pillars → **MAINTAIN**, not LEGACY, unless
truly abandoned.

### 7. Report

`<project-path>/.github/automation/seo_program_review_{YYYYMMDD_HHMMSS}.md`

```markdown
# SEO program review — {project name}

**Date:** {ISO}
**Previous last_reviewed_at:** …
**Next suggested review:** +{review_cadence_days}d

## Goal
…

## Product map (routes that matter)
| Route | Role |
|-------|------|

## Desk snapshot
- GSC totals / Primary-or-tool click share (method + numbers)
- Strategy status (ok/partial/empty)
- PostHog: … or WARN

## Changes — project.yaml
| Field | Before | After | Why |

## Changes — seo_program.yaml
- Mode: … → …
- Primary backlog: N open / measuring / done
- Harvest: …
- Tools: …
- Prune: N open merge_into / N noindex needing confirm

## Prune
- Open `merge_into`: n (list key slugs → keepers)
- Open `noindex` (confirm required): n — see Needs your decision
- Measuring / done preserved: …

## Metrics scoreboard (direction only this pass)
| Metric | Source | Signal |

## Recommended weekly modes (next 30d)
…

## Needs your decision
- Include every open prune `noindex` row (slug + evidence + `confirm: required`)
…

## Product gaps (not SEO)
…

## Files touched
- `.github/automation/seo_program.yaml`
- `.github/automation/project.yaml` (if any)
```

### Final user message

```
## SEO program review — {project} ({date})

**TL;DR:** …

**Mode for next weekly:** {current_mode}

**YAML:** seo_program.yaml updated; project.yaml {changed|unchanged}

**Primary open:** n · **Harvest open:** n · **Tools open:** n · **Prune open:** n merge_into / n noindex

**Needs decision:** … (include noindex prune rows)

**Report:** {path}

**Next:** /weekly-seo (consumes mode + queues incl. prune) · re-review in ~{review_cadence_days}d
```

---

## Empty / first-run seed

If `seo_program.yaml` is missing:

1. Still run product + desk + PostHog (as available).
2. Write a full v1 file from evidence (do not copy another project’s keywords).
3. Seed `primary_backlog` from `strategy` primary list; mark shipped when catalog
   already has a matching live slug.
4. Seed harvest from top GSC pages that are edu-heavy with weak product path
   (≤10 rows).
5. Seed tools from public tool routes + commercial landing pages.
6. Optionally seed `prune_queue` from LEGACY / `do_not_expand` inventory when
   GSC evidence exists (≤10 open rows); else empty list. Every noindex seed
   must have `confirm: required`.

---

## Explicit bans

| Ban | Do instead |
|-----|------------|
| Living plan only in markdown report | Always update `seo_program.yaml` |
| Duplicate Primary SOT in program file that fights `project.yaml` | Align backlog to strategy; edit strategy when themes change |
| Weekly-style ≤5 create storm mid-review | Queue work; optional separate `/weekly-seo` |
| LEGACY on high-impr pillars for purity | MAINTAIN + harvest |
| Invent conversion metrics | PostHog schema or WARN |
| Edit pageseeds-app mid-run | Report product/CLI gaps |
| Agent-executed noindex / bulk deindex from prune scan | Queue `noindex` with `confirm: required` + Needs your decision only |

---

## Guardrails (summary)

- Monthly (or product-shift) cadence — not weekly spine.  
- Product read + CLI desk + PostHog → YAML + report.  
- `project.yaml` gates; `seo_program.yaml` ops.  
- Thin queues; evidence; no mass rewrites.  
- Schema: [docs/SEO_PROGRAM.md](../../../docs/SEO_PROGRAM.md).
