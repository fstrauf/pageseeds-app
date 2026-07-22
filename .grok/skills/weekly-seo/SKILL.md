---
name: weekly-seo
description: >-
  Run the weekly SEO pass for one PageSeeds project via pageseeds-cli
  (explore signals, plan, execute tasks, report). Use when the user wants
  weekly SEO, SEO maintenance, organic growth this week, or /weekly-seo.
  Operator only — never edit pageseeds-app source.
when-to-use: >-
  Triggers on "/weekly-seo", "weekly SEO", "run weekly SEO", "SEO pass",
  "SEO maintenance", "what should we do this week for organic traffic",
  "grow this site's SEO".
argument-hint: "[project-name-or-id]"
user-invocable: true
metadata:
  short-description: "Weekly SEO pass via pageseeds-cli"
---

# Weekly SEO — Agent Skill (explore-first)

## Invocation

```
/weekly-seo
/weekly-seo coffee
/user:weekly-seo
```

Prefer opening the **customer project** (or any cwd outside `pageseeds-app`).
Requires installed `pageseeds-cli` on PATH (`pnpm install:cli` from app repo).

You are the weekly SEO operator for **one** PageSeeds project. You run when the
user asks (typically weekly). Your job is to **find the highest-impact truth**
about organic growth this week, propose a small set of measures, then **execute**
them through PageSeeds tasks — not by editing content yourself and **not** by
editing PageSeeds product source.

## Model: tools + skill ≈ MCP + skill

| Layer | What it is here |
|-------|-----------------|
| **Capability surface** | Installed `pageseeds-cli` binary — JSON tools (same functions as app/Rig tools). Treat this like an MCP tool list. |
| **Operator / policy** | This skill — goals, budgets, lifecycle, report, isolation rails. |
| **Agent loop** | You (Grok/Kimi/etc.) — **choose tools freely** within hard rails. |
| **Product source** | **Out of scope.** Never open or patch `pageseeds-app` / `src-tauri` during this run. |

Do **not** treat the old 1→8 phase list as a mandatory script. Use **hard rails**
always; use **soft guidance** when you have no better lead.

---

## When to use

- Weekly per-project SEO maintenance  
- On-demand: “what should we do this week for this site’s organic traffic?”

## Separation of concerns (mandatory)

| Role | Workspace | May write |
|------|-----------|-----------|
| **This skill (SEO operator)** | Customer project path, or neutral cwd | Only the weekly report under project automation |
| **pageseeds-cli** | N/A (binary on PATH) | Tasks/DB/content **via tools only** |
| **Product engineer** | `pageseeds-app` (separate session) | App source / PRs |

**Do not run this skill with `pageseeds-app` as the open workspace.** If the
session is clearly inside the PageSeeds product repo (e.g. path contains
`pageseeds-app` and you are editing Rust/TS product files), **stop** and tell
the user to re-run with only the **customer project** open (or cwd = that
project). Missing CLI features are product gaps — report them; do not implement
them mid-run.

---

## Inputs (project context)

Always establish:

- `-i <project-id>` — PageSeeds project ID  
- `-p <project-path>` — absolute path to the **customer** project repo  

If the user only names a site:

```bash
sqlite3 ~/Library/Application\ Support/com.pageseeds.app/pageseeds.db \
  "SELECT id, name, path FROM projects"
```

State the chosen `id` / `name` / `path` once at the start. Pass `-i` / `-p` on
every tool call that needs them. Prefer the absolute `path` from the DB.

### Tool invocation

Use the **installed binary** from **any** directory. Do **not** `cd` into
`pageseeds-app` or use `cargo run`.

```bash
pageseeds-cli <tool> -i <project-id> -p <project-path> [args...]
```

If `pageseeds-cli` is missing from PATH:

```bash
# One-time install (product machine only — not during a weekly SEO pass)
# From a pageseeds-app checkout:
./scripts/install-cli.sh
# installs to ~/.local/bin/pageseeds-cli
```

Tell the user to install/update the CLI; **do not** build product code as part
of the SEO run unless the user explicitly asked for a product engineering task.

All tools print **JSON to stdout**. Never invent numbers — cite tool output.

The CLI talks to the same SQLite DB as the desktop app
(`~/Library/Application Support/com.pageseeds.app/`). The Tauri UI does not need
to be running.

---

## Hard rails (always)

These are **not** optional. Breaking them fails the run.

1. **Data access only via `pageseeds-cli`** (and the report file). No direct DB
   writes, no hand-editing project MDX/content. Tasks do content changes.
2. **No product source edits.** Never modify files under `pageseeds-app`,
   `src-tauri`, app `Cargo.toml` / `package.json`, or any PageSeeds product
   tree. Never “add a missing CLI command” or patch the app mid-run.
3. **Missing capability → escalate, don’t implement.** If a needed subcommand
   fails or is absent, document the gap in the report / user message and work
   around with existing tools or stop that branch. Open/file a product issue
   only if the user wants that — still not by editing app source here.
4. **Budgets:** max **5** tasks *created* per run; max **15** *executions*
   (created + follow-ups); max **3** new articles from keyword selection.
5. **May-create list only** via `create-task` (see below). Never
   `create-task` for `write_article`, `create_landing_page`, `create_hub_page`,
   or `consolidate_cluster` — those only come from selection commands after review.
6. **Evidence:** every task and major finding cites specific tool evidence
   (counts, slugs, URLs).
7. **Review points:** resolve only when mechanical; escalate judgment calls
   (merges of high-traffic pages, strategic keyword choices).
8. **Report:** write `weekly_seo_{YYYYMMDD_HHMMSS}.md` under
   `<project-path>/.github/automation/`. **Only** content file you write.
9. **Missing integrations:** if GSC/Clarity/Reddit fail, degrade and say so —
   do not fake data.

### May-create via `create-task`

`ctr_audit`, `content_review`, `content_cleanup`, `cannibalization_audit`,
`indexing_diagnostics`, `indexing_health_campaign`, `fix_indexing_internal_links`,
`cluster_and_link`, `interlinking`, `fix_content_article` (**always** requires
`-S` / `--slug <url-slug>` — never bare; attaches SERP recommendations artifact),
`update_research_shortlist`, `generate_feature_spec`, `seo_health_scan`,
`collect_gsc`, `collect_clarity`, `clarity_analytics`, `research_keywords`,
`research_landing_pages`, `reddit_opportunity_search`.

---

## Soft guidance (default path — abandon when evidence says so)

**Default shape of a good run:**

```text
A. Recency / load check
B. Ground truth (if needed)
C. Free exploration  ← primary investigative work
D. Plan (table) + approval if interactive
E. Execute + follow-ups + mechanical reviews
F. Report
```

You may **reorder, deepen, or skip** B–C pieces when a strong anomaly appears.
You must still honor hard rails and still produce a plan before mass create
(interactive: user approval; hands-off: short internal plan then proceed).

### Explicit permission to leave the map

If tool output shows a **clear, high-impact anomaly** (e.g. literal template
vars in titles, mass not-indexed, one cluster eating the site, brand-token
catastrophe), you **should**:

1. Stop covering the full evaluate menu.  
2. Spend more tool calls chasing that thread (related tools, samples, framework).  
3. Propose fewer, sharper tasks aimed at that root cause.  
4. In the report **Exploration path** section, say what you followed and what
   you skipped on purpose.

Do **not** pad the run with low-value checklist tasks after you’ve found the
real story.

---

## A. Recency / load (usually first)

```bash
pageseeds-cli list-tasks -i <id> -p <path>
```

- Check latest `weekly_seo_*.md` under automation.  
- **Skip entire run** only if last weekly was **&lt; 5 days ago** *or* **≥ 5**
  fix-like tasks still open (`todo` / `queued` / `in_progress`) **and** the user
  did not force a run. Always state skip reasoning.  
- User can override: “run anyway” → continue.

---

## B. Ground truth (when needed, not always every tool)

Live GSC is cheap truth:

- `gsc-performance`, `gsc-movers`, optionally `gsc-queries`  

If snapshots / audit look stale:

```bash
pageseeds-cli create-task -i <id> -p <path> \
  -t collect_gsc -T "Weekly GSC refresh" -r "<reason>" --auto-enqueue
# then execute-task when you need fresh data this run
```

Same idea for Clarity if configured. If GSC is disconnected, continue on
content/indexing tools only and note it.

You do **not** need a full audit every week if last audit is fresh and GSC is
calm — use judgment.

---

## C. Free exploration (primary)

**Goal:** answer *“What is the highest-leverage SEO problem or opportunity this
week?”* using tools, not a fixed checklist.

### Tool catalog (capability surface)

| Tool | Use when |
|------|----------|
| `gsc-performance` | Site/page traffic, CTR, impressions (`-l` limit, default 50, max 200) |
| `gsc-movers` | Who gained/lost clicks 30d vs prior 30d (`-l` limit, default 30, max 200) |
| `gsc-queries` | Query-level demand; striking-distance / uncovered queries (`-u` page URL optional; `-l` limit) |
| `ctr-health` | Per-article CTR / SERP snippet health |
| `indexing-status` | Not indexed + reasons |
| `cannibalization-clusters` | Competing pages |
| `content-audit-report` | Cached multi-check audit (if missing/stale: `run-content-audit`) |
| `run-content-audit` | Refresh audit snapshot |
| `article-list` | Inventory / status filter |
| `article-frontmatter` | One slug’s frontmatter (`--slug`) |
| `article-body-hash` | Exact duplicate bodies |
| `article-title-scan` | Title bugs, templates, dup tokens |
| `article-link-graph` | Orphans / weak internal links |
| `framework-files` | Layout, sitemap, robots when template-level bug suspected |
| `research-shortlist` | Theme/keyword backlog + health |
| `article-quality-reviews` | Recent quality gate failures |
| `list-tasks` / `get-task` | Open work, artifacts, review state |
| `score-zero-impression-articles` | Dead-weight / zero-impression candidates |

**Exploration budget:** prefer **≤ ~25 tool invocations** before locking a plan
(not a hard fail — stop earlier if the story is clear; go a bit further if still
confused). Do not thrash the same tool without a new hypothesis.

### How to explore

1. Start with **1–3 wide scans** (often movers + performance + one of
   indexing / title-scan / cannibalization — pick by prior knowledge).  
2. **Follow the strongest anomaly** 2–4 tools deep.  
3. Only then sample other areas if budget remains.  
4. Gap / growth-without-problems: run or schedule `research_keywords` logic
   (see soft hints below) — evaluative tools alone cannot prove “no gaps.”

### Soft hints (not a mandatory order)

Use these as **priors**, not a required table to walk top-to-bottom:

- Big click losses / CTR disaster → `ctr_audit` or `content_review`  
- Many not-indexed → indexing diagnostics / internal links  
- Clusters → `cannibalization_audit`  
- Orphans → `cluster_and_link` / `interlinking`  
- Structural audit pile-up → `content_cleanup` or `content_review`  
- Template/title systemic bugs → `generate_feature_spec` + evidence  
- Quiet site + old research → `research_keywords` (generative gap detector)  
- No single clear signal → `seo_health_scan`  
- Reddit configured + capacity → `reddit_opportunity_search`  

**Research:** still generative. Prefer `research-shortlist` health
(`promising` / `depleted` / `unproven`). Never claim “no gaps found” if research
did not run — say **skipped** and why + last research date.

When keyword picker / research final selection is **avoid-heavy** (AIO-blocked
head terms, mostly `winnability: avoid`), prefer the shortlist **promising**
path rather than rubber-stamping demoted heads:
- Call `research-shortlist -i <id> -H promising` (CLI health filter) and bias
  themes/seeds from those rows for a re-run of `research_keywords` /
  `research_landing_pages` if capacity allows.
- Or filter/prefer promising themes yourself and re-run research, then pick
  only `differentiate` / `target` (non-avoid) rows from the picker.
Soft guidance only — residual avoids as last resort when nothing better exists.

### Known tool limits — do not dead-end as “caveats for later”

Agents often stop with soft caveats instead of using the rest of the surface.
**Treat limits as branching rules, not conversation enders.**

| Limit you hit | What it means | Do this *in the same run* (if budget allows) |
|---------------|---------------|-----------------------------------------------|
| `gsc-movers` only returned ~30 rows | **Default limit**, not “30 pages on the whole property.” Live GSC comparison is ranked; top losses/gains dominate. | If the story is clear from concentration of loss, proceed. If not, re-call `gsc-movers -l 100` (or 200). Cross-check with `gsc-performance -l 100` for absolute traffic ranking (not period delta). |
| “No daily history / `gsc_page_daily` empty” | Daily series is filled by **`collect_gsc`** / GSC sync (append-only snapshots), not by `gsc-movers`. Movers use live Search Analytics API for two windows. | If you need day-level series or outcome measurement later: `create-task -t collect_gsc …` and **`execute-task` this run** when GSC is connected. Do not only suggest it as a human next step unless budget is exhausted or GSC is disconnected. |
| “No competitive SERP scrape” | **Out of tool surface** today — there is no SERP competitor tool in pageseeds-cli. | State that once. Infer pressure only from **position deltas + page type + query mix** (`gsc-queries -u <url>`). Do not invent competitor ranks. If competitive content gaps matter, use `research_keywords` / shortlist — not a fake SERP audit. |
| Story is “top 3–4 URLs are the whole problem” | Deep dive **is** available without a new weekly mode. | For each URL: `gsc-queries -u <url>`, `article-frontmatter --slug <slug>`, optionally title-scan / content-audit / link-graph for that slug. Then propose `ctr_audit`, `content_review`, or `fix_content_article` with evidence. **Do this before** parking work as “if you want a follow-up without weekly SEO.” |

**Anti-pattern:** ending with “if you want a next step, deep-dive top URLs or wire collect_gsc” when those steps are **in your tool list and budgets**. Either do them now or explicitly say which hard rail blocked them (execution budget, user hands-off plan already locked, GSC not connected).

---

## D. Plan

Before creating a batch of tasks, write a short plan:

| Finding | Evidence (tool + numbers/slugs) | Proposed task | Why this week |

- **Interactive:** get user approval once per project.  
- **Hands-off:** proceed after stating the plan briefly.  

Max **5** creates; prioritize impact.

---

## E. Execute

Creating ≠ running. Use:

```bash
pageseeds-cli create-task -i <id> -p <path> \
  -t <task_type> -T "<title>" -r "<reason citing evidence>"
pageseeds-cli execute-task -I <task-id>
```

**`fix_content_article` always requires a slug** — never create it bare:

```bash
pageseeds-cli create-task -i <id> -p <path> \
  -t fix_content_article -S <url-slug> \
  -T "Fix content: <title>" -r "<reason citing evidence>"
```

Bare `create-task -t fix_content_article` without `-S` / `--slug` is rejected.
The CLI attaches a full `recommendations_{article_id}` artifact with SERP
categories (title / description / h1 / intro).

Loop:

1. Execute approved tasks one at a time.  
2. On success, execute `follow_up_tasks` (and theirs) within budget.  
3. Stop at **15** executions; list leftover IDs under “Queued, not yet run.”  
4. Fail once → note, continue; at most one retry per task.  
5. Task lands in `review` → resolve (below).

### Expected auto follow-ups

- `write_article` / hub / landing (from selection) → quality review + cluster link  
- `content_review` / `content_audit` → may spawn fix tasks / feature-spec cooldown  
  (behavior depends on app version; execute what appears in `follow_up_tasks`)

### Quality gate

If `review_article_quality` fails (`overall_pass` false), create
`fix_content_article` **with** `-S <url-slug>` for that file if none exists,
then execute (counts toward 15). Never bare `fix_content_article`.

### Review resolution

```bash
pageseeds-cli get-task -I <task-id>
```

- **CannibalizationPicker:** high-confidence mechanical merges via
  `select-cannibalization -I <parent> -S merge:<id>,hub:<id>`; escalate ambiguous.  
- **KeywordPicker:** do **not** rubber-stamp. Check demand/difficulty, no
  self-competition (`article-list` / `gsc-queries`), intent fit. Prefer
  non-avoid / `differentiate` / `target` rows when present (skip AIO-blocked
  `avoid` heads if product-adjacent long-tails are on the list). Then
  `select-keywords -I <id> -K kw1,kw2` — max **3** articles, fewer is better.  
- **ContentReviewPicker / fix proposals:** `select-content-review -I <parent> -P id1,id2`  
- **RedditPicker:** `create-reddit-replies -I <id> -P id1,id2`  
- **ArtifactReview:** summarize; `update-task-status -I <id> -s done`  

Escalate irreversible or strategic choices.

---

## F. Report

Write:

`<project-path>/.github/automation/weekly_seo_{YYYYMMDD_HHMMSS}.md`

```markdown
# Weekly SEO — {project name}

**Date:** {ISO timestamp}

## Summary
2–3 sentences: biggest finding and what was done.

## Exploration path
What you chased first, detours, what you skipped on purpose (and why).

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
- Missing tools or commands that blocked work — for a **separate** product session.

## Recommended next actions
…
```

### Final message to the user

Compact, human-readable — no JSON dumps:

```
## Weekly SEO — {project name} ({date})

**TL;DR:** …

**Exploration:** one line on the path you followed

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

- Free exploration **encouraged**; hard rails **mandatory**.  
- **Installed `pageseeds-cli` only** — never `cargo run` from app source.  
- **No pageseeds-app / product source edits** during this skill.  
- Missing tools → report gap; do not implement product code.  
- Max 5 creates / 15 executions / 3 new articles.  
- Evidence required; no invented data.  
- No direct content edits; no illegal task types via `create-task`.  
- Mechanical reviews only; escalate judgment.  
- Only write the weekly report file.  
- Idempotent re-runs: recency + spawner keys.

---

## Design note (experiment)

This skill is the **explore-first** experiment (epic #92 option D): same
`pageseeds-cli` capability surface as a future MCP, skill as operator, product
repo out of the operator workspace. If runs feel freer *and* still safe, carry
the pattern into MCP-era skills. If agents thrash or skip real work, tighten
soft guidance—not hard rails first.
