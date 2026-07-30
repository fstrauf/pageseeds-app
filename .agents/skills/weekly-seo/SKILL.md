---
name: weekly-seo
description: >-
  Run the weekly SEO pass for one PageSeeds project via pageseeds-cli
  (desk reads → PostHog MCP behavioral desk → ≤5 actions → report).
  Use when the user wants weekly SEO, SEO maintenance, organic growth this
  week, or /weekly-seo. Operator only — never edit pageseeds-app source.
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
`pageseeds-cli` on PATH. Prefer the prebuilt install (no cargo):
`curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash`.
Dev/checkout secondary: `pnpm install:cli` / `./scripts/install-cli.sh` (or `FROM_SOURCE=1`).

You are the weekly SEO operator for **one** project. Find the highest-impact
organic growth opportunity **within the program mode/queues**, propose ≤5
measures, execute via PageSeeds tasks — not by editing content or product
source yourself.

| Layer | Role |
|-------|------|
| **Capability** | `pageseeds-cli` JSON tools (≈ MCP surface) |
| **Program ops SOT** | `<project>/.github/automation/seo_program.yaml` — mode + queues (see [SEO program](#seo-program-ops-seo_programyaml)) |
| **Theme gates SOT** | `project.yaml` via `strategy` / `research-context` — Primary / ACTIVE / LEGACY |
| **Behavioral desk** | PostHog MCP — expected light engagement desk (see [PostHog desk](#posthog-desk)); no CLI wrapper |
| **Policy** | This skill — budgets, lifecycle, report, isolation |
| **Agent** | You — choose tools within hard rails |
| **Product source** | **Out of scope** — never patch `pageseeds-app` |
| **Monthly rebalance** | `/seo-program-review` — not this skill |

---

## When to use

- Weekly per-project SEO maintenance
- On-demand: “what should we do this week for organic traffic?”

## Separation of concerns (mandatory)

| Role | Workspace | May write |
|------|-----------|-----------|
| **This skill** | Customer project / neutral cwd | Weekly report + **narrow** `seo_program.yaml` queue/status updates (see below) |
| **pageseeds-cli** | N/A (binary on PATH) | Tasks/DB/content **via tools only** |
| **Product engineer** | `pageseeds-app` (separate session) | App source / PRs |
| **Program rebalance** | Customer project via `/seo-program-review` | Full `seo_program.yaml` + strategy `project.yaml` |

If the session is inside the product repo (`pageseeds-app` + editing Rust/TS),
**stop** and re-run with only the customer project open. Missing CLI features are
product gaps — report them; do not implement mid-run.

---

## Inputs

Prefer **setup defaults** so you do not pass `-i`/`-p` on every call:

```bash
# Once per customer project (idempotent)
pageseeds-cli setup --path . --yes
# Discover registered projects (no raw sqlite)
pageseeds-cli list-projects
```

After setup, desk tools resolve project context from flags → env →
`.pageseeds.yaml` → global defaults → registry. Explicit `-i`/`-p` still override.

- `-i <project-id>` — optional after setup  
- `-p <project-path>` — optional after setup  

If context is missing: run `pageseeds-cli setup` (do **not** open sqlite by hand).

```bash
pageseeds-cli <tool> [args...]
# or with explicit override:
pageseeds-cli <tool> -i <project-id> -p <project-path> [args...]
```

Use the installed binary from any directory. Never `cd` into `pageseeds-app` or
`cargo run` for this skill. All tools print **JSON**. Never invent numbers.

CLI uses the operator SQLite store (no desktop UI).

---

## Hard rails (always)

Breaking these fails the run.

| # | Rule |
|---|------|
| 1 | **CLI only** for data/tasks, **except** PostHog MCP desk + allowed automation file writes (weekly report; narrow `seo_program.yaml` statuses). No direct DB writes, no hand-editing MDX. No PostHog CLI wrap — MCP is the integration. |
| 2 | **No product source edits** under `pageseeds-app` product crates (unless explicitly requested). |
| 3 | **Missing capability → escalate**, don’t implement. Document gap; work around or stop that branch. |
| 4 | **Budgets:** ≤**5** creates · ≤**15** executions · ≤**3** new articles from keyword selection. |
| 5 | **May-create list only** (below). Never `create-task` for `write_article`, `create_landing_page`, `create_hub_page`, `consolidate_cluster` — those come from selection after review. Path B write uses `write-context` / `write-submit`; Path B merge uses `merge-context` / `merge-submit`; Path B fix uses `fix-context` / `fix-submit`. |
| 6 | **Evidence:** every task / major finding cites tool output (counts, slugs, URLs). |
| 7 | **Reviews:** mechanical only; escalate judgment (high-traffic merges, strategic keywords). |
| 8 | **File writes:** (a) `weekly_seo_{YYYYMMDD_HHMMSS}.md` under automation; (b) **narrow** updates to `seo_program.yaml` only — queue item `status` / `target_slug` / short `notes` when you ship or claim work. **Do not** rewrite goal, metrics, mode mix, or invent Primary keywords mid-weekly (that is `/seo-program-review`). |
| 9 | **Missing integrations:** GSC/Clarity/Reddit fail → degrade and say so; never fake data. |
| 10 | **PostHog desk is default:** after GSC shortlist, run the light PostHog desk via MCP. **Only** source for project id: `project.yaml` → `posthog_project_id`. Assume MCP exists. If MCP missing, auth fails, or `posthog_project_id` absent → **WARN** in report + final message, continue on GSC only — no name/host guessing, no skill-side map, never invent engagement numbers. |
| 11 | **Program mode:** When `seo_program.yaml` exists, lock **Mode** from `current_mode` (unless user forces another mode) and prefer draining matching queues over pure desk noise. Missing file → desk-default + note gap. |
| 12 | **Run spacing (Phase 0):** If the newest **mode-executing** `weekly_seo_*.md` for this project is **&lt; 5 days** old (timestamp from filename `weekly_seo_YYYYMMDD_HHMMSS.md` — same parse as weekly-seo-status; do **not** re-stat mtime), a **mode-executing** weekly run **MUST STOP** before any desk/mode work. Breaking this fails the run. **Measure-only** exemption and explicit **override** only — see [Phase 0 — spacing gate](#a-phase-0--spacing-gate). |

### May-create via `create-task`

`fix_content_article` (**always** `-S`/`--slug` — never bare), `content_review`,
`research_keywords`, `research_landing_pages`, `indexing_diagnostics`,
`indexing_health_campaign`, `fix_indexing_internal_links`, `content_cleanup`,
`cluster_and_link`, `interlinking`, `ctr_audit`, `cannibalization_audit`,
`update_research_shortlist`, `generate_feature_spec`, `seo_health_scan`,
`collect_gsc`, `collect_clarity`, `clarity_analytics`, `reddit_opportunity_search`.

**Prefer when desk data already supports the action:** `fix_content_article -S`,
`research_keywords`, `research_landing_pages`, targeted indexing fixes.
Do **not** invent work via soft audits when desk reads suffice.
**Demote for weekly CLI:** `ctr_audit` — see [CTR / content fix policy](#ctr--content-fix-policy).
**Demote for weekly CLI:** full `indexing_health_campaign` — see
[Indexing / not-indexed policy](#indexing--not-indexed-policy).


**Not for weekly strategy:** `content_review` — historical desktop / unattended only (#139).
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

### Indexing / not-indexed policy

CLI weekly best-path for not-indexed pages is **desk-selected targeted fixes**,
not a full `indexing_health_campaign` fan-out.

1. **Best path (default):** Use `site-overview` `not_indexed_sample` (catalog-
   resolvable slugs only) + `articles` / `article -S` to pick a few actionable
   URLs. Prefer targeted `fix_indexing_internal_links` and/or
   `fix_content_article -S` (counts toward ≤5 creates). Cite reason codes /
   slug evidence in `-r`.
2. **Do NOT** enqueue full `indexing_health_campaign` as the default weekly
   action — same budget-burn risk as full `ctr_audit`.
3. **`indexing_health_campaign` is rare/optional:** Keep it in may-create for
   scoped/rare use when desk cannot narrow candidates and you explicitly need
   the productized campaign (still honor budgets). If you create it, note why.
4. **Hard ban-as-default:** Never treat full IHC as the weekly CLI default for
   “many not-indexed” — desk → targeted fixes first.

### Striking-distance preferred path

CLI weekly best-path for **striking-distance** pages (GSC avg position roughly
page-1 bottom / page-2 top with real impressions) is **desk filter → ≤2
targeted existing actions** — not a campaign type, not a rank tracker.

**Soft prior only:** never mandatory just because the candidate list is
non-empty. Counts toward hard rail ≤5 creates; do not invent creates when
higher-impact levers win.

#### GSC evidence criteria (candidate definition)

A page is **striking-distance** for weekly judgment when **all** hold
(defaults; agent may tighten):

| Criterion | Default | Notes |
|-----------|---------|--------|
| Position band | `avg_position` ∈ **[7.0, 13.0]** inclusive | GSC page rollup window (desk period, usually 28d) |
| Impressions | **≥ ~200** in that window | Aligns with WS5 spirit; raise bar on large sites |
| Live catalog | Resolved slug, **not redirected** | Same desk hygiene as other hard actions |
| Freshness | Honor `freshness.stale` / missing tape | Do not invent the band from empty/stale `gsc_page_daily` |

**Not sufficient alone:** soft TF-IDF clusters, DataForSEO / paid SERP rank,
plateau tool prose without GSC numbers.

#### Candidate sources (priority order)

1. **Primary:** `site-overview.striking_distance` (count + sample from #204) —
   cite overview rows in task `-r`. Empty sample with fresh GSC tape means
   no band inventory this window, not “feature missing.”
2. **Fallback only** when overview sample is empty/degraded but you still
   suspect band opportunity (e.g. stale overview, need higher bar):
   - `articles -m 200` (or higher) → keep rows with `gsc.avg_position` in 7–13
   - and/or `gsc-performance` rows with position in band + meaningful impressions
3. Deep-read **≤2–3** top candidates with `article -S` + `gsc-queries -u`

Do **not** require position-band CLI flags or a new campaign task type.

Operator path:

```text
desk (site-overview.striking_distance first; fallback filter only if needed)
  → rank candidates by impact (impressions × need)
  → deep-read ≤2–3 (article -S + gsc-queries -u)
  → create ≤2 actions from existing types (or skip if better levers win)
  → report under Measures / Skipped soft signals
```

#### Preferred actions matrix (existing types only)

| Observation on candidate | Prefer | Do not |
|--------------------------|--------|--------|
| High impr + weak title/meta/CTR vs peers | Targeted `fix_content_article -S` or Path B fix (`fix-context` / `fix-submit` with `content` or `ctr`) | Full `ctr_audit` as default |
| Decent CTR stuck in band; thin inbound / orphan / weak related links | Soft `cluster_and_link` or `interlinking` (cite link graph / desk) | Full `indexing_health_campaign` as default ranking push |
| Same query hard-cannibal with another URL | Hard-cannibal path / optional scoped `cannibalization_audit` — **not** pure ranking push | Soft clusters as merge authority |
| Stale dated slug / clear dead page | Inventory decision (#206 territory) — not auto-push | Blind noindex here |

#### Budgets, overlap, bans

- **≤2** striking-distance creates per weekly run (subset of hard rail ≤5).
- **May-create / soft prior only** — never mandatory when the list is non-empty.
- **CTR overlap:** one URL should not get both a generic CTR fan-out and a
  striking-distance create; **one targeted fix is enough**.
- **Explicit bans for this lever:** no `striking_distance_campaign` task type;
  no DataForSEO / paid rank tracker; no default full IHC or full `ctr_audit`
  for “push rankings.”
- Cite tool evidence (position, impressions, slug) in `-r` and the weekly report.

### Measurement: two review types (do not conflate)

| Task type | Weekly policy |
|-----------|----------------|
| `ctr_outcome_review` | **Cancel / ignore** — deprecated measurement fan-out (#152). Not weekly backlog. |
| `content_outcome_review` | **Mandatory when due** (≤1–2, after client-side `not_before` filter) — real closed-loop for write/fix/merge ships (#23; Path B spawn = #203). **Non-compliant only if** ≥1 due (post-filter) **and zero executed**. Due count >2: execute ≤1–2, report remainder as **deferred under cap** (still compliant). Explicit “none due” (post-filter) when zero rows pass the filter. |

**CTR closed-loop (no review-task fan-out):** measurement = **`gsc_page_daily`
tape** + sparse **`ctr_outcomes`** change events when a CTR fix ships (Path B
`fix-submit -k ctr` or nested `fix_ctr_article`). In the measurement pass, run
`pageseeds-cli ctr-outcomes` (verifies live titles, classifies ready rows, rolls
up report). Also read `site-overview.outcomes` for stuck/pending counts. Do
**not** run legacy `ctr_outcome_review` tasks.

**Content closed-loop:** system-spawned `content_outcome_review` rows compare
**GSC snapshot windows** on `gsc_page_daily` (not live SERP / rank trackers /
DataForSEO). See [Due `content_outcome_review`](#due-content_outcome_review)
under soft path A.

- **Never** `create-task content_outcome_review` (system spawn only; bare create
  lacks `content_outcome_target`). **Never** add it to may-create.
- **Never** treat “measurement stubs” as “cancel all outcome reviews” — only
  `ctr_outcome_review` is cancel-or-ignore.

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
| Full `indexing_health_campaign` spawn by default (#179) | Desk → targeted `fix_indexing_internal_links` / `fix_content_article -S`; scoped IHC only when needed |
| `striking_distance_campaign` (or any new ranking-push campaign type) (#205) | Skill path only — desk band filter → ≤2 existing `fix_content_article -S` / `cluster_and_link` / `interlinking` |
| DataForSEO / paid SERP rank tracker for ranking push | GSC `avg_position` + impressions only; never invent ranks |
| Full IHC / full `ctr_audit` as default for striking-distance (WS5) | See [Striking-distance preferred path](#striking-distance-preferred-path) |
| Link building / outreach product (task types, competitor backlink acquisition, outreach automation) (#202 / #210) | Human/PR outside CLI; only automated off-site path is Reddit (`reddit_opportunity_search`) when configured. Report gap — do not implement product mid-run |
| Rank-tracker / SERP position as weekly outcome (Accuranker-class) (#202 / #210) | Measure with GSC desk + `gsc_page_daily` tape; SERP only if research/diagnostic path already justified |
| Nested `execute-task` LLM for write/fix/merge when Path B tools exist | Path B package → session edit → submit |
| `create-task content_outcome_review` / may-create addition | System spawn only; execute due rows within ≤1–2 cap (mandatory when due; see path A) |
| `ctr_outcome_review` as weekly action backlog (#152) | Cancel / ignore; call `ctr-outcomes` for CTR closed-loop |
| Video clips as weekly spine / may-create / multi-clip batch (#222) | Elective via `/video-clip` only — see [Optional post-publish video](#optional-post-publish-video-elective) |
| Territory / top-shortlist-by-impressions as default **new-article** seeds when Primary or ACTIVE exist (#275) | Research week: `research-pull -K` from `content_strategy` Primary + ACTIVE first; shortlist/desk only if strategy empty or Primary/ACTIVE exhausted |

## Soft guidance (default path)

```text
Phase 0 spacing gate → load seo_program.yaml (mode + queues)
  → due content_outcome_review (≤1–2; non-compliant only if due≥1 and zero executed; rest deferred under cap; “none due” if filter empty)
  → refresh ground truth (if stale) → site-overview
  → articles / article / gsc-queries
  → optional striking-distance filter (when pos 7–13 inventory looks high-ROI)
  → PostHog desk (default — project.yaml posthog_project_id → switch-project; WARN if missing)
  → plan within Mode → ≤5 actions → update queue statuses (incl. measuring→done) → report
```

Reorder/deepen when a clear anomaly appears (including optional
[striking-distance preferred path](#striking-distance-preferred-path) when the
band has meaningful inventory). Still honor hard rails and plan before mass
create (interactive: approval; hands-off: short plan then go).

### SEO program ops (`seo_program.yaml`)

**Path:** `<project-path>/.github/automation/seo_program.yaml`  
**Schema / full field guide:** [docs/SEO_PROGRAM.md](../../../docs/SEO_PROGRAM.md)  
**Producer:** `/seo-program-review` (monthly). **This skill consumes** it weekly.

Theme gates (Primary, ACTIVE/MAINTAIN/LEGACY, `do_not_expand`) remain in
`project.yaml` / `pageseeds-cli strategy` — the program file does **not** replace
them. It sequences **mode + queues** so weekly runs do not freestyle every time.

#### Load (after Phase 0 spacing gate, before desk deep-dive)

1. Read `seo_program.yaml` if present (`schema_version`, `goal`, `current_mode`,
   `mode_mix_this_month`, `primary_backlog`, `harvest_queue`, `tools_queue`,
   `product_paths`, `metrics`, `last_reviewed_at`).
2. **Lock Mode** for this run:

| `current_mode` | Report label | Prefer |
|----------------|--------------|--------|
| `attract` | Mode A — Attract (Primary write) | Open `primary_backlog` → research-pull Primary/ACTIVE first → Path B write/publish/cluster |
| `harvest` | Mode B — Harvest (TOFU → product) | `harvest_queue` + desk high-impr weak CTA → Path B fix (CTA/intro/links to `product_paths`) |
| `tools` | Mode C — Tools / commercial | `tools_queue` → fix or write calculator/screener/dashboard/alternative pages |
| `measure` | Mode D — Measure | Due outcome reviews + GSC movers + PostHog blog→signup; light creates only if critical |

3. **Every week** still run measure as a **mandatory side-pass** when due
   rows exist: due `content_outcome_review` ≤1–2 even when Mode is A/B/C.
   **Non-compliant only if** ≥1 due (post-`not_before`) **and zero executed**.
   When due >2, leftover rows are **deferred under cap** (still compliant).
   “None due” after filter is fine.
4. **User override:** “research week” / “fix week” / explicit mode → honor user;
   note deviation from `current_mode` under Decisions.
5. **Missing / unparseable file:** continue desk-default (research vs fix from
   signals); state **“seo_program.yaml missing — desk-default mode”** in report.
   Suggest `/seo-program-review` under Recommended next actions. Do not invent a
   full program board mid-weekly.
6. **Stale review:** if `last_reviewed_at` older than `review_cadence_days`
   (default 30), one line under Recommended: run `/seo-program-review`.

#### Mode vs desk (priority)

1. Program mode + matching **open** queue items (with desk evidence).  
2. Desk anomalies that are clearly higher ROI **and** still on-strategy
   (not LEGACY / do_not_expand expansion).  
3. Never expand beginner/tax LEGACY to “fill” a quiet attract week — prefer
   harvest/tools or skip with honesty.

#### End of run — narrow YAML updates only

When you ship or claim work, update the matching queue row:

- `status`: `open` → `in_progress` / `shipped` / `measuring` (if +30d outcome scheduled) / `done`
- When a matching queue row is `measuring` and you executed its
  `content_outcome_review` this run: flip `measuring` → `done` with a
  one-line `notes` classification (`improved` / `regressed` / `neutral` /
  `insufficient_data` from executor — no new thresholds).
- Do **not** flip `measuring` early if the review is still future-dated.
- `target_slug` when a Primary keyword gains a live slug
- short `notes` (one line)

**Do not** change `goal`, `metrics`, `mode_mix_this_month`, `current_mode`, or
cluster policy mid-weekly unless the user explicitly asks to advance mode
(e.g. “set mode to harvest”). Full rebalance = `/seo-program-review`.

### A. Phase 0 — spacing gate

**Mandatory first steps** before desk, mode work, or growth creates. Hard rail #12.

#### Step 1 — newest report age (filename timestamp)

```bash
ls -1t <project-path>/.github/automation/weekly_seo_*.md 2>/dev/null | head -1
```

Parse wall-clock run time from the filename only:
`weekly_seo_YYYYMMDD_HHMMSS.md` (e.g. `weekly_seo_20260723_183104.md` →
`2026-07-23 18:31`). Do **not** re-stat mtime for “run time” (same rule as
weekly-seo-status).

- **Age** = calendar days from that date to today (same day = `0`).
- No report file → age is infinite (gate does **not** fire).

**Which report drives the spacing clock:** prefer the newest report that was
**mode-executing**. If a report header/Summary marks **measure-only**, it does
**not** reset the 5-day clock — look past it to the newest mode-executing
report (or treat as no mode-executing report if none exist). Unlabeled reports
count as mode-executing (fail closed for spacing).

#### Step 2 — refuse mode execution when age &lt; 5 days

| Intent this invocation | Gate |
|------------------------|------|
| **Mode-executing** (desk/mode work, growth creates, queue drain, Path B write/fix/merge, research selects that create work) | If newest **mode-executing** report age **&lt; 5 days** **and** no override → **STOP** before any desk/mode work |
| **Measure-only** (outcome reviews / measurement / report only; **zero** growth creates; **no** mode queue work) | **Exempt** — may proceed without override; does **not** require override |
| Explicit **override** in this invocation | Proceed with mode work; **must** log under **Decisions made for you** |

**Standard refusal message** (mode-executing blocked; copy shape exactly):

```text
Weekly SEO for `<id>` ran `<date>` (<5d ago). Refusing mode execution. Run a measure-only pass, wait until `<date+5d>`, or say 'run anyway' to override.
```

Breaking the gate (continuing mode work without override when age &lt; 5d)
**fails the run**.

#### Measure-only (definition + clock)

A pass is **measure-only** when it creates/executes **no growth actions**: no
mode queue drain, no Path B write/fix/merge for growth, no research selection
creates, no may-create growth tasks — only measurement (e.g. due
`content_outcome_review`, `ctr-outcomes`, light desk reads) and optional
report. Measure-only:

- Needs **no** override when the spacing gate would otherwise fire.
- **Does not reset** the spacing clock. If you write a report, mark it
  **measure-only** in the header/Summary so the next Phase 0 can ignore it for
  the 5d clock.
- Mode-executing end reports **do** reset the clock (newest mode-executing
  filename becomes the gate baseline).

#### Override

Only an **explicit** user phrase in the **current** invocation counts:
`"run anyway"` / `"force weekly"` (or clear equivalent). Do **not** infer
override from silence or from a prior session. When used:

1. Continue with full mode path.
2. Log one line under **Decisions made for you** (e.g. “Spacing override: user
   said run anyway; last mode-executing report was {date} ({n}d ago).”).

#### Step 3 — open-task load check (unchanged; separate signal)

```bash
pageseeds-cli list-tasks -i <id> -p <path>
```

- **≥ 5** open fix-like tasks (`todo` / `queued` / `in_progress` for types such
  as `fix_content_article`, `fix_ctr_article`, `content_review`, indexing
  fixes) **and** user did not force → may **skip** mode work (load signal;
  advisory/skip as before). State why.
- Independent of the hard 5d spacing gate: load can skip even when spacing is
  clear; spacing can refuse even when open-task count is low.
- Override for load: same “run anyway” / “force weekly” family; log under
  Decisions.

#### Due `content_outcome_review`

**Mandatory when due** (after client-side `not_before` filter) — close the
measurement loop for recent write/fix/merge ships before inventing new soft
work. **Non-compliant only if** ≥1 due row (post-filter) **and zero executed**.
When due count >2: execute ≤1–2, report remaining as **deferred under cap**
(still compliant). Zero due after filter is fine; state **“none due”**
explicitly in the report. Outcomes are **GSC window compares**
(`gsc_page_daily`), not live SERP.

```text
list-tasks -t content_outcome_review -s todo
→ keep rows where not_before is null OR not_before ≤ now (ISO)
→ select ≤1–2: prefer high-impr / oldest due / measuring-linked queue rows
→ execute-task selected (counts toward ≤15 exec; NOT a create)
→ get-task: read content_outcome_compare / classification
→ ArtifactReview: summarize → update-task-status -s done
→ report under "Movement since last run" + measuring queue read-back
→ remaining due (beyond cap) → report as deferred under cap (compliant)
→ when matching seo_program.yaml row is measuring: flip measuring→done + notes
```

| Rule | Detail |
|------|--------|
| Cap | **≤1–2** executes per weekly run (measurement must not dominate ≤15) |
| Future `not_before` | **Do not** execute — `execute-task` does **not** enforce delay; skill filters client-side |
| Create | **Never** `create-task content_outcome_review`; never may-create |
| Compliance | **Non-compliant only if** ≥1 due (post-`not_before`) **and zero executed**. Due >2: execute ≤1–2 (prefer high-impr / oldest / measuring-linked); remainder = **deferred under cap** (still compliant). “None due” must be explicit when the filter yields zero. |
| On `regressed` / `insufficient_data` | Note in report; optional soft desk deep-dive later — **do not** auto-spawn fix fan-out from this alone |
| Not required | DataForSEO, Clarity, Reddit, full `ctr_audit` / IHC, or `content_review` as strategy brain |

### B. Refresh ground truth (if stale)

**Dual-path still applies for live ad-hoc probes** (`gsc-performance` /
`gsc-movers` / `gsc-queries`). For **desk tape** (`gsc_page_daily` totals,
movers, inventories), `collect_gsc` + **`execute-task` this run** is enough —
paginated page-daily sync keeps desk totals trustworthy (#262). There is no
separate `refresh_ground_truth` product; do not treat its absence as a blocker.

| Need | Do |
|------|-----|
| Live demand / deltas | `gsc-performance`, `gsc-movers`, `gsc-queries` (cheap ad-hoc truth) |
| Stale snapshots / desk cache | `create-task -t collect_gsc` then **`execute-task` this run** if needed |
| Clarity (if configured) | same pattern with `collect_clarity` |
| PostHog behavioral | **Default** light desk after GSC shortlist — see [PostHog desk](#posthog-desk); MCP only, not a CLI task |

- **Desk tape flag:** On `site-overview` / `articles`, read `freshness.stale` and `freshness.hint` before treating zero impressions/clicks as demand truth (empty/stale `gsc_page_daily` is not “no traffic”).

If GSC disconnected: continue on catalog/indexing tools only; note it.

### C. Desk exploration (primary)

**Goal:** *What is the highest-leverage SEO problem/opportunity this week?*

#### Primary desk tools (explore these first)

| Tool | Role |
|------|------|
| `site-overview` | Compact weekly desk entry: totals, top pages, movers, freshness, hints, plus `zero_impression` / `striking_distance` / `hard_cannibalization` inventory (#204), `redirect_equity` / `non_catalog_gsc` residual inventory (#261), and `outcomes` aggregates (#302) |
| `ctr-outcomes` | CTR closed-loop measure: deploy-verify + classify ready `ctr_outcomes` + report rollup (#302). Free desk tool; not a task type. |
| `articles` | GSC-aware catalog list (filters: status, min impressions, period) |
| `article` | Full package for one slug: frontmatter, body outline, top queries, neighbors (`-S`/`--slug`) |
| `gsc-performance` | Site/page traffic, CTR, impressions (`-l`, default 50, max 200) |
| `gsc-movers` | Gained/lost clicks 30d vs prior (`-l`, default 30, max 200) |
| `gsc-queries` | Query-level demand; page filter `-u <url>` |
| `list-tasks` / `get-task` | Open work, artifacts, review state |
| `create-task` / `execute-task` | Act within may-create + budgets |
| Selection cmds | `select-keywords`, `select-cannibalization`, `select-content-review`, `create-reddit-replies`, `update-task-status` |
| Path B write | `write-context` / `write-submit` → `publish-content -S` — outer-agent prose after keyword selection (preferred CLI path); submit leaves catalog draft; publish is explicit (#257); successful submit schedules +30d `content_outcome_review` (#203) |
| Path B fix | `fix-context` / `fix-submit` — preferred targeted content/CTR edits; content kind schedules +30d outcome review; CTR records `ctr_outcomes` only |
| Path B merge | `merge-context` / `merge-submit` — outer-agent merge after approved keep+redirects; successful submit schedules keeper outcome review (#203) |

#### Behavioral desk (default — not GSC substitute)

| Tool | Note |
|------|------|
| **PostHog MCP** | **Default** light engagement desk after GSC shortlist — see [PostHog desk](#posthog-desk). **Not** demand truth. **WARN** (do not invent data) if MCP missing or `project.yaml` lacks `posthog_project_id` |

#### Optional / secondary (NOT ground truth, not required path)

| Tool | Note |
|------|------|
| `cannibalization-clusters` | Soft TF-IDF clusters — **fail open** on mono-niche; **not merge authority** |
| `ctr-health` | Productized composite — prefer impressions/CTR from desk + `gsc-queries` |
| `seo_health_scan` (task) | Optional backlog only when desk data is insufficient |
| `content-audit-report` / `run-content-audit` | Optional deep structural checks |
| `indexing-status`, `article-title-scan`, `article-body-hash`, `article-link-graph`, `framework-files`, `research-shortlist`, `article-quality-reviews`, `score-zero-impression-articles`, `article-list` / `article-frontmatter` | Use when desk points there — **dead-weight score is secondary** (see [Dead-weight / winnability](#dead-weight--winnability-secondary)) |

**Exploration budget:** prefer **≤ ~25** tool calls before locking a plan
(PostHog desk counts toward this — keep it to a few MCP calls, not a full
product review). Stop early when the story is clear; do not thrash the same
tool without a new hypothesis.

#### How to explore

1. **Wide:** `site-overview` (+ `gsc-movers` / `gsc-performance` if needed).  
2. **Catalog:** `articles` for filters (high impressions, low CTR, status).  
3. **Deep:** `article -S <slug>` + `gsc-queries -u <url>` on top candidates.  
4. **PostHog desk (default):** read `posthog_project_id` from `project.yaml` →
   `switch-project` → light bounce / engagement / top paths — weave into
   ranking; never invent SEO demand from it. If id missing or MCP blocked →
   **WARN** and continue GSC-only.  
5. **Act** only with evidence; gap growth → research (below).

#### Soft hints (priors only — never forced weekly actions)

Priors from desk data including first-class overview inventory fields
(`zero_impression`, `striking_distance`, `hard_cannibalization` — #204;
`redirect_equity`, `non_catalog_gsc` — #261).
**Do not** require DataForSEO, Clarity, Reddit, full `ctr_audit`, full
`indexing_health_campaign`, or `content_review` as strategy brain for these
signals. Empty or `degraded_reason` (e.g. `gsc_missing`) is not a force create.

| Pattern from desk | Action preference |
|-------------------|-------------------|
| High impressions + low CTR + weak title/meta | Desk → targeted `fix_content_article` (`-S`) for top waste URLs; Path B fix preferred. **Not** full `ctr_audit` first (see CTR policy); **not** `content_review` as strategy brain |
| High GSC impressions/clicks + high bounce / low engagement (PostHog) on same URL | Prefer that slug for `fix_content_article -S` — intent/content mismatch more likely; cite both GSC + PostHog in `-r` |
| Strong organic landing (GSC) but weak conversion path (PostHog funnel/path) | SEO action still content/SERP; note product/UX friction in report — do **not** invent non-SEO tasks mid-run |
| Same query on **2+ URLs** (`gsc-queries`) or same intent competing | Optionally `cannibalization_audit` **only with hard evidence**; never treat soft clusters as ground truth |
| Many not-indexed | Desk → targeted `fix_indexing_internal_links` / `fix_content_article -S` on catalog sample slugs. **Not** full `indexing_health_campaign` first (see Indexing policy) |
| Orphans / weak links | `cluster_and_link` / `interlinking` |
| Structural MDX issues | `content_cleanup` / `content_review` |
| Template/title systemic bugs | `generate_feature_spec` + evidence |
| Quiet site + thin backlog | `research_keywords` / `research_landing_pages` |
| Desk insufficient across levers | Optional `seo_health_scan` (not default) |
| Reddit configured + capacity | `reddit_opportunity_search` |

##### `site-overview` inventory signals (#204 / #261)

Always read these on overview. Optional priors only — never mandatory creates
every week.

| Pattern | Preference |
|---------|------------|
| High `zero_impression` count / sample (and not degraded) | Optional: cache-first dead-weight path ([below](#dead-weight--winnability-secondary)) — **not** mandatory; never weekly re-score loop |
| `striking_distance` count / sample (pos ~7–13 + meaningful impr) | See **[Striking-distance preferred path](#striking-distance-preferred-path)** — ≤2 creates; overview sample first; fallback filter only if needed; **not** mandatory |
| `hard_cannibalization` samples (and not degraded) | Optional scoped `cannibalization_audit` **only with hard multi-URL query evidence** — soft clusters still non-authority |
| `redirect_equity` sample (residual GSC on 301 sources → destinations) | After merges: attribute residual landings to keepers before ignoring source URLs; cite source/dest impressions in report — **not** auto-merge |
| `non_catalog_gsc` sample (high-impr never-catalog pages) | Note residual demand outside live catalog; investigate map gaps or content opportunities — do **not** invent keepers without evidence |

### Dead-weight / winnability (secondary)

Scoring and remediating low/zero-impression articles is **optional and secondary**
— not part of the default weekly ≤5 spine, and **not** a weekly re-score loop.

**Do not bulk re-score zero-impression inventory every week.** GSC desk
(`site-overview` / `articles` / `gsc-*`) is ground truth for post-ship outcomes.
`score-zero-impression-articles` is **opt-in paid SERP** (DataForSEO), subject to
`serp_guard` cache (14d keyword+locale) + per-project daily live-call cap (50).
Prefer free desk actions unless a human asks for winnability buckets.

**Cache-first (no paid SERP):**

```bash
# List last scores + bucket counts — $0, no DataForSEO
pageseeds-cli score-zero-impression-articles -i <id> -p <path> --from-cache
# alias: --list
```

**Live score only when cache empty/stale and budget allows** (paid DataForSEO
SERP; local score TTL **60 days**, max **25** assessments/run; shared SERP
cache/cap via `serp_guard`):

```bash
pageseeds-cli score-zero-impression-articles -i <id> -p <path> [-m 10] [--max 25] [--ttl-days 60]
# Re-score even when fresh: --force
```

Scores persist to `article_metadata` namespace `winnability` (`scored_at` + bucket).
JSON reports `scored` / `skipped_fresh` / `skipped_cap` / `skipped_budget` /
`cache_hits` / `live_calls` / `truncated`. Budget skips are **not** Avoid.

#### Bucket → human-compose action (existing tools only)

| Bucket | Prefer | Avoid |
|--------|--------|--------|
| **Avoid** | Merge-context path / `merge-submit` when hard cannibal or thin dupe; **noindex only with human confirm** | **Never** bulk noindex from score output; never auto-noindex |
| **Differentiate** | `create-task -t fix_content_article -S <slug>` (or Path B `fix-context` / `fix-submit`) citing cached reason — **$0, no DataForSEO** | Re-running live score just to act |
| **Target** | `cluster_and_link` / `interlinking` and/or targeted `fix_content_article -S` | Treating zero-impr alone as “delete” |

**Remediation execution does not call DataForSEO** — compose from cached
bucket/reason. Do **not** re-score every weekly pass; prefer `--from-cache`
first. Counts toward ≤5 creates only if you act; inventory note-only is fine.

### PostHog desk

**Role:** light **behavioral / engagement** signal next to GSC demand — not a
second ground truth, not a full product weekly review (that is
`posthog-weekly-insights`). Use only to **re-rank or strengthen** SEO actions
already justified by desk/GSC evidence. **No CLI wrapper** — PostHog is MCP-only.

**Assumption:** PostHog MCP is available in operator sessions. Do **not** design
the happy path around “maybe no MCP.” Treat absence as a **warn + continue**
failure mode, not a normal skip.

**When to run (default path — after primary GSC desk / shortlist):**

| Condition | Action |
|-----------|--------|
| Normal weekly run **and** `posthog_project_id` present | **Always** run the light desk below after GSC shortlist |
| User says “skip PostHog” / “SEO only” | Skip without probing; note under Skipped |
| User wants deep product analytics | Point them to `posthog-weekly-insights`; do **not** expand this pass |
| MCP missing / not connected / auth fail | **WARN** (report + final message); continue GSC-only — never invent data |
| `posthog_project_id` missing / empty | **WARN** — missing config; **do not** guess, match by name/host, or use any skill-side map; continue GSC-only |
| Id present but switch/query fails | **WARN** with error; continue GSC-only |

#### Config (only path — no fallbacks)

**Single source of truth:** one field on the existing project config:

```yaml
# <project-path>/.github/automation/project.yaml
schema_version: 1
product_name: Expense Sorted
posthog_project_id: "131482"   # required for PostHog desk; numeric PostHog project id
# … search_keywords, clusters, reddit …
```

| Rule | Detail |
|------|--------|
| **Required for desk** | `posthog_project_id` in `project.yaml` |
| **Not allowed** | Skill fleet tables, `posthog.yaml`, name/host fuzzy match, “whatever MCP has open,” inventing ids |
| **If missing** | **WARN** + GSC-only — fail the PostHog branch cleanly; do not invent a second config path |

#### Project selection (mandatory before queries)

1. Read `posthog_project_id` from
   `<project-path>/.github/automation/project.yaml`.
2. Missing / empty → **WARN**, stop PostHog for this run (no further MCP
   project discovery).
3. **Always** `call switch-project {"projectId": <id>}` before queries (use
   `info switch-project` if schema unsure). Never query without switching.
4. Confirm taxonomy before querying:  
   `call read-data-schema {"query":{"kind":"events"}}`  
   Never assume `$pageview` / property names without schema confirmation.

#### Light desk (≤ ~4–6 MCP calls total — counts toward exploration budget)

Prefer web-analytics style tools when present; fall back to trends after schema:

| Goal | Prefer |
|------|--------|
| Site KPIs (visitors, bounce, duration) last 7d | `query-web-overview` (or trends on confirmed pageview event) |
| Top paths + bounce | `query-web-stats` broken down by path/pathname; else trends + `$pathname` breakdown after schema |
| Core Web Vitals on top organic pages (optional) | `query-web-vitals` if events exist — only for URLs already on the SEO shortlist |
| Blog → product path (optional, 1 funnel max) | `query-funnel` / `query-paths` only when a blog/CTA event exists in schema |

Window: **last 7 days** (optionally compare prior week if the tool supports it).
Always prefer `filterTestAccounts` / project defaults that exclude internal users.

**How to weave into the plan (judgment rules):**

1. **GSC remains demand authority.** PostHog does not create “write this keyword”
   or replace impressions/CTR/position.
2. **Intersection wins:** URLs that show up in **both** high-impr/low-CTR (or
   movers) **and** high bounce / low engagement → higher priority for
   `fix_content_article -S`.
3. **PostHog-only friction** (rage clicks, funnel drop on app routes, errors) →
   note in report **Needs attention (product)** — not a weekly SEO create
   unless it is clearly content/SERP (e.g. thin landing, broken title intent).
4. **Do not** file product bugs or expand into session-replay archaeology here.
5. **Cite both sources** in task `-r` and the report when PostHog influenced
   ranking (e.g. “GSC 12k impr / 0.8% CTR; PostHog bounce ~78% on /blog/…”).

**Anti-patterns:**

- Treating “PostHog optional / skip if inconvenient” as the happy path
- Fleet maps, name/host matching, or any fallback when `posthog_project_id` is missing
- Full PostHog weekly product review mid SEO pass (`posthog-weekly-insights`)
- Querying the wrong PostHog project “because something returned data”
- Inventing SEO demand from DAU/pageviews alone
- Burning exploration budget on replay deep-dives
- Building a pageseeds-cli PostHog collect task for this skill path


### Research strategy package (#141 / #255 / #275)

Session owns themes/seeds; CLI owns Ahrefs pull:

```bash
# Research week: seeds from content_strategy Primary/ACTIVE first (not territory heads)
pageseeds-cli research-pull -i <id> -p <path> --seeds "theme one,theme two" ...
# → candidates for select-keywords / write Path B
```

Prefer this over relying solely on nested research_seed_extraction when tools exist.

**Week mode (program-first when `seo_program.yaml` exists; else soft prior):**

| Week mode | When | Seed / action rule |
|-----------|------|-------------------|
| **Attract / research** (`current_mode: attract` or user research week) | New articles | Seeds from **open `primary_backlog`** first, then `content_strategy.primary_keywords` + **ACTIVE** clusters (intentional **PLANNED** OK). Territory/shortlist only if strategy empty **or** Primary/ACTIVE exhausted/covered. |
| **Harvest / CTR fix** (`current_mode: harvest` or fix week) | High-impr low-CTR, harvest_queue, striking-distance, cannibal, indexing | Prefer Path B fix on queue + desk waste URLs. Territory shortlist aids **existing** pages only. |
| **Tools** (`current_mode: tools`) | Commercial / calculator / screener | Prefer `tools_queue` + product routes; still honor strategy gates. |
| **Measure** (`current_mode: measure`) | Closed-loop week | Outcome reviews + scoreboard signals; not a research dump. |

**Explicit ban:** Do **not** treat “top shortlist by impressions / promising” as default new-article seeds when Primary or ACTIVE keywords exist in the package.

**Empty strategy:** degrade — seed from shortlist/desk; state “strategy empty/unparseable” in report (pair with #276 when shipped). Do not invent Primary.

**Deviation honesty:** If operator/session still used territory-only seeds while Primary gaps existed, note under Skipped / Decisions — do not pretend Primary-first ran.

**Research:** generative. Prefer `research-context` (auto-refreshes shortlist
when empty/stale via territory) then health (`promising` / `depleted` /
`unproven`). Path B `research-pull` still does **not** write shortlist.

- **Prefer `research-context` first:** It ensures shortlist freshness (empty or
  territory rows older than 7d → territory analysis). Read
  `shortlist_refreshed` / `shortlist_refresh_reason` and territory
  `skip_reasons` when still empty after refresh — not “mystery empty.”
  Read `content_strategy` + `guidance` from the package before proposing seeds.
- **Content strategy (`content_strategy` in research-context JSON, #255 / #275 / #276):**
  On **research weeks**, seed order is **Primary / ACTIVE first** (intentional
  PLANNED when expanding a planned pillar). Territory shortlist and desk are
  **fallback only** when strategy is empty/unparseable **or** Primary/ACTIVE
  themes are exhausted/covered — not equal peers. If `content_strategy.status`
  is `empty` or `partial`, do **not** claim strategy gates applied — prefer fix
  `project.yaml` (structured SOT) via setup / `pageseeds-cli strategy` /
  `project-config-status` / `migrate-project-config` if legacy MD only, or
  report skipped-research honesty before Path B seed inventiveness. If strategy
  missing/empty, inspect `<project-path>/.github/automation/project.yaml`
  (`search_keywords` + `clusters`); if YAML missing and only legacy MD exists,
  migrate first. If still empty, degrade to shortlist/desk and say so.
- **Never seed `do_not_expand` / LEGACY** (and deprioritize MAINTAIN vs
  ACTIVE/primary). After pull, reject those candidates before select-keywords.
  Final selection also hard-drops them in-core (`strategy_rejected` in the
  picker artifact / step message).
- **Shortlist health** (`promising` / pending) ranks **fallback** themes and
  prioritizes re-research of uncovered strategy gaps — **not** the default
  new-article seed list when Primary/ACTIVE exist.
- **After pull:** prefer 1–2 picks when thin inventory; max **3** rail unchanged.
  Reject / do **not** `select-keywords` for LEGACY / do-not-expand candidates
  even if API ranked them high.
- **Manual fallback only:** If `research-context` fails or you need a force
  re-fill, create+execute `update_research_shortlist`. Do **not** treat empty
  shortlist after Path B pull alone as proof there are no research gaps.
- Never claim “no gaps found” if research did not run — say **skipped** + why +
  last research date.

Avoid-heavy keyword pickers (AIO-blocked heads, mostly `winnability: avoid`):
prefer shortlist **promising** themes/seeds **as fallback** and re-run research;
pick only `differentiate` / `target` rows when possible. Residual avoids = last resort.

#### Known limits (branch, don’t dead-end)

| Limit | Do *this run* if budget allows |
|-------|--------------------------------|
| `gsc-movers` ~30 rows | Default limit — raise `-l 100`/`200` or cross-check `gsc-performance` |
| Empty `gsc_page_daily` | Run `collect_gsc` + execute if day-level series needed; movers use live API windows |
| No SERP scrape tool | Infer from position deltas + query mix only; use research for gaps |
| No link-building / outreach product (#202 / #210) | Human/PR outside CLI; only automated off-site is Reddit when configured — escalate/report, do not invent tasks |
| No Accuranker-class rank tracker as outcome (#202 / #210) | GSC desk + daily tape only; SERP only when research/diagnostic path already justified |
| Top 3–4 URLs are the problem | Deep-dive each with `article` + `gsc-queries` **now**, then fix tasks |

**Anti-pattern:** parking “deep-dive later” when tools + budgets allow it now.

---

## D. Plan

**State Mode first** (from program or override), then:

| Finding | Evidence (tool + numbers/slugs) | Queue item (if any) | Proposed task | Why this week |

- Interactive: one approval per project. Hands-off: state plan, proceed.  
- Max **5** creates; impact first; prefer open program queue items on-mode.

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
- Path B `write-submit` → marks write task done + spawns `cluster_and_link` +
  schedules +30d `content_outcome_review` (GSC closed-loop; #203); catalog stays
  **draft** until `publish-content -S <slug>` (#257)
- Path B `fix-submit -k content` → schedules +30d `content_outcome_review`;
  `-k ctr` → sparse `ctr_outcomes` change event only (no content outcome review)
- Path B `merge-submit` → keeper redirects applied + schedules keeper
  `content_outcome_review`
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
- **ContentReviewPicker:** unattended / non-weekly only — not weekly strategy. If inherited, dispose mechanically; do not start new `content_review` for weekly explore. `select-content-review -I <parent> -P id1,id2`
- **RedditPicker:** `create-reddit-replies -I <id> -P id1,id2`
- **ArtifactReview:** summarize; `update-task-status -I <id> -s done`

### Path B — CLI write package (happy path after `select-keywords`)

`select-keywords` still creates `write_article` tasks for provenance / queue
tracking. For **CLI best-path**, complete those tasks via write-context +
session prose + write-submit — **not** nested `execute-task write_article`
(weak global providers produce thin single-shot MDX).

```bash
# 1. Package (deterministic — no LLM)
# Classic: after select-keywords (keyword on research selection list)
pageseeds-cli write-context -i <id> -p <path> \
  -I <research-task-id> -K "<keyword>"
# Intentional Primary (Mode A): keyword ∈ project.yaml Primary/problem —
# no Ahrefs selection membership; -I optional
pageseeds-cli write-context -i <id> -p <path> -K "per-leg P&L options"
# → JSON: content_brief, auth_source (research_selection |
#   strategy_primary_or_problem), target_file, skill_content, write_task_id

# 2. Session agent writes full MDX to target_file using skill_content + brief
#    (min 800 words, proper frontmatter title/description/slug/date, H1, links)

# 3. Submit until ok (or give up within execution budget)
pageseeds-cli write-submit -i <id> -p <path> \
  -f <target_file> [-I <write_task_id>] [-K "<keyword>"]
# → ok:false + checks → expand and resubmit (file kept)
# → ok:true → article registered as catalog **draft**; write_article marked done;
#   cluster_and_link + content_outcome_review (+30d) spawned

# 4. Explicit catalog publish (required before treating post as catalog-live)
pageseeds-cli publish-content -i <id> -p <path> -S <slug>
# → draft/ready_to_publish → published + articles.json export
# → already published = skip/no-op; year-mismatch/blocked leave status unchanged
```

| Rule | Path B |
|------|--------|
| **Do** | `write-context` → write MDX to `target_file` → `write-submit` until `ok` → `publish-content -S <slug>` |
| **Ban** | `execute-task write_article` on the happy path |
| **Ban** | `fix_content_article` for min_word_count / length recovery — expand and **resubmit** instead |
| **Ban** | Treating write-submit alone as catalog-live / published |
| **Budget** | Each `write-submit` / `publish-content` attempt counts toward the **15** execution budget |
| **Provenance** | `select-keywords` may still spawn `write_article`; Path B completes them via submit |
| **Closed-loop** | Successful submit schedules system `content_outcome_review` — never create those tasks yourself |


### Path B — CLI fix package (preferred for targeted content/CTR)

Preferred for targeted content/CTR edits with full file context:

```bash
pageseeds-cli fix-context -i <id> -p <path> -S <slug> -k content|ctr [-g goals]
# session agent edits full file using package
pageseeds-cli fix-submit -i <id> -p <path> -S <slug> -k content|ctr [--file mdx]
# → content: +30d content_outcome_review scheduled
# → ctr: ctr_outcomes change event only (measure later via ctr-outcomes)
```

Nested fallback when needed: `create-task fix_content_article -S <slug>` +
`execute-task` with desk evidence (still prefer Path B when practical).
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
# → ok:true → redirects.csv, inbound rewrites, sources redirected, task done;
#   content_outcome_review (+30d) for keeper slug
```

| Rule | Path B merge |
|------|----------------|
| **Do** | `merge-context` → write MDX to `keeper_file` → `merge-submit` until `ok` |
| **Ban** | `execute-task consolidate_cluster` on the happy path |
| **Confirm** | When `requires_human_confirm` (clicks ≥ 50 or impressions ≥ 1000), pass `-y` only after human OK |
| **Budget** | Each `merge-submit` attempt counts toward the **15** execution budget |
| **Provenance** | consolidate tasks may still exist; Path B completes them via submit |

---

## Optional post-publish video (elective)

**Not** part of the weekly spine. Video clips are elective via **`/video-clip`**;
not may-create; not a `create-task` type (Phase C / #224 is separate).

| Rule | Detail |
|------|--------|
| **When** | After a successful Path B `write-submit` (or another clearly shipped new/updated article this run) **and** `<project-path>/video.config.json` exists |
| **Budget** | **0–1** video candidates per weekly run. Default = **0** |
| **Default action** | Name the slug + packaging reason under **Recommended next actions** / **Optional video** and suggest `/video-clip <slug>` — **do not** burn the weekly session on Playwright render |
| **If user explicitly wants video now** | Invoke the video-clip skill path for that one slug; weekly hard rails unchanged (≤5 creates / ≤15 exec — video is **not** a create-task) |
| **Never** | Add video tools to may-create; treat video as mandatory when config exists; multi-clip batch mid weekly |

Missing `video.config.json` → skip silently (or one line under Skipped). Full
operator runbook: `.agents/skills/video-clip/SKILL.md`.

---

## F. Report

`<project-path>/.github/automation/weekly_seo_{YYYYMMDD_HHMMSS}.md`

```markdown
# Weekly SEO — {project name}

**Date:** {ISO timestamp}
**Mode:** {A Attract | B Harvest | C Tools | D Measure} — source: seo_program.yaml `current_mode`={…} | user override | desk-default (file missing)

## Summary
2–3 sentences: biggest finding and what was done (tie to mode/queues).

## Program
- Goal (one line from seo_program.yaml) or “no program file”
- Queues touched: primary/harvest/tools rows claimed or status-updated
- last_reviewed_at / stale review warning if any

## Exploration path
Desk path chased, detours, what you skipped (and why).
Include PostHog desk outcome: ran (`posthog_project_id=…`) **or**
**WARN** reason (`posthog_project_id` missing / MCP missing / switch failed /
user skip).

## PostHog signals
1–5 bullets: bounce/engagement/CWV or path findings **only where they
intersect SEO candidates**. If blocked: one bold **WARN** line
(`posthog_project_id` missing / MCP / auth / user skip) — never invent metrics.

## Measures taken
| Measure | Evidence | Task | Outcome |
- Call out **striking-distance** picks explicitly when used (slug + avg_position + impressions + why this action).

## Movement since last run
- **Outcome reviews:** executed (slug + classification: improved/regressed/neutral/insufficient_data from executor) **or** explicit “none due” (post-`not_before` filter). **Non-compliant only if** ≥1 due and zero executed. Due beyond the ≤1–2 cap → list as **deferred under cap** (compliant).
- **`measuring` queue read-back:** per `seo_program.yaml` row in `measuring`:
  - review executed this run → flip `measuring`→`done` + one-line classification note
  - review due, was in the selected ≤1–2, but still unexecuted → flag non-compliance (also if zero reviews executed while any were due)
  - review due but **deferred under cap** (not selected) → note deferred; leave `measuring`; **not** non-compliant
  - review still future-dated → leave status; one line “waiting until {not_before}”
  - or “nothing measuring yet”
- **PostHog delta:** one-liner vs previous weekly report’s PostHog signals if a prior `weekly_seo_*.md` exists; else “no prior baseline — this run sets it”.
- Verdicts **inform** (do not dictate) mode/action choices; `regressed` still does **not** auto-spawn fix fan-out.

## Decisions made for you
…
(Spacing overrides — “run anyway” / “force weekly” — must appear here as one line.)

## Needs your decision
| Task | What's pending | Command to resolve |

## Queued, not yet run
…

## Skipped (and why)
- Including research skip vs “not run” honesty rule.
- PostHog **WARN** / skip reason if not already covered above (missing
  `posthog_project_id` is a config fail for that branch).
- Soft desk signals noticed (zero-impr / striking / hard-cannibal counts) or degraded/empty (e.g. `gsc_missing`).
- Striking-distance candidates seen but not acted on (and why — budget, better lever, stale tape, thin evidence).
- Future `not_before` `content_outcome_review` rows left for later (do not execute early).
- Due `content_outcome_review` beyond the ≤1–2 cap → **deferred under cap** (compliant; not non-compliance).

## Product / CLI gaps (if any)
- Real product/CLI gaps only (missing tools, auth). Desk tape refresh is
  `collect_gsc` + execute — not blocked on a `refresh_ground_truth` product.
- PostHog MCP missing or missing `posthog_project_id` → **WARN** and fix
  `project.yaml` — no skill fallbacks, no CLI `collect_posthog`.

## Recommended next actions
…

## Optional video (if any)
- Candidate slug + why (or “none — default 0” / config missing).
- Suggest `/video-clip <slug>` rather than mid-pass render unless user asked.
```

### Final user message (no JSON dumps)

```
## Weekly SEO — {project name} ({date})

**Mode:** … (program | override | desk-default)

**TL;DR:** …

**Exploration:** one line (desk path; PostHog project id or **WARN: …**)

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
- **Due `content_outcome_review` mandatory when due** (≤1–2 exec toward ≤15; not creates; post-`not_before` filter). **Non-compliant only if** ≥1 due and zero executed; remainder beyond cap = **deferred under cap** (compliant). “None due” must be explicit. Never create these tasks. Path B write/merge/content-fix submit schedules them (#203). Report **Movement since last run** every week.  
- Overview inventory fields (zero-impr / striking / hard-cannibal) are **never mandatory** actions; honor `degraded_reason` when tape is missing.  
- Dead-weight scoring is **secondary / cache-first** (`--from-cache`); not default spine; no weekly re-score loop; no auto bulk noindex.  

- Striking-distance (pos **7–13**, impr ≥ ~200): read `site-overview.striking_distance` first; optional soft prior → ≤**2** existing actions; no campaign type / DataForSEO / default full IHC or `ctr_audit`.  
- Outcomes = **GSC windows** (`gsc_page_daily`), not live SERP / DataForSEO.  
- `ctr_outcome_review` cancel/ignore; call `ctr-outcomes` for CTR measure; `content_outcome_review` mandatory when due — do not conflate.  
- Low CTR → desk-selected `fix_content_article` (`-S`); not default full `ctr_audit`.  
- Not-indexed → desk-selected `fix_indexing_internal_links` / `fix_content_article -S`; not default full `indexing_health_campaign`.  
- Empty research shortlist → call `research-context` first (auto-refresh); `update_research_shortlist` only if force/fail. 
- **PostHog default desk:** light engagement via MCP after GSC shortlist; **only** `project.yaml` `posthog_project_id` → `switch-project`; missing id or MCP → **WARN** + GSC-only (no fallbacks); GSC = demand authority; never invent data; no CLI wrap.  
- Evidence required; no invented data; no illegal create-task types.  
- Soft clusters **not** ground truth / merge authority.  
- Mechanical reviews only; file writes = weekly report + narrow `seo_program.yaml` status updates.  
- **Program mode** from `seo_program.yaml` when present; missing → desk-default + note; monthly rebalance = `/seo-program-review`.  
- Idempotent re-runs: **hard Phase 0 spacing gate** (5d on mode-executing reports; measure-only exempt; explicit override logged) + open-task load check + spawner keys.  
- **Video clips are elective via `/video-clip`**; not weekly spine / not may-create.

---

## Design note

**Desk model (epic #117):** ~10-tool mental model — Site State reads
(`site-overview` / `articles` / `article` + GSC) then few hard actions. Soft
clusters and specialist audits remain available but are **optional**, not the
weekly spine. CLI weekly CTR: desk-ranked waste URLs → targeted fixes; full
`ctr_audit` BackendAuto fan-out is the UI/unattended path, not CLI default.
CLI weekly indexing: catalog-aware `not_indexed_sample` → targeted link/content
fixes; full `indexing_health_campaign` is rare/scoped, not CLI default.

**Closed-loop measurement (#209 / #203 / #301):** due system-spawned
`content_outcome_review` is **mandatory when due** (post-`not_before` filter;
≤1–2) early in the weekly path — GSC snapshot windows only. **Non-compliant
only if** ≥1 due and zero executed; due beyond cap are **deferred under cap**
(still compliant). Report under **Movement since last run** (including
`measuring` queue read-back: deferred-under-cap measuring rows are noted, not
flagged non-compliant). Nested success and Path B write/merge/content-fix
submit both schedule these (+30d). Keep `ctr_outcome_review` cancel-or-ignore
(#152). Overview inventory fields (zero-impression / striking-distance / hard
cannibal, #204) are optional priors; never mandatory weekly actions; budgets
≤5 / ≤15 / ≤3 unchanged.

**Striking-distance (#205):** preferred weekly path is skill-only —
`site-overview.striking_distance` first (fallback `articles` /
`gsc-performance` only if needed) → deep-read ≤2–3 → ≤2 existing fix/link
creates. No `striking_distance_campaign`, no DataForSEO, no default full
IHC/`ctr_audit` for ranking push. Soft prior only; one URL one targeted fix
(no CTR + striking double-create).

**Dead-weight (#206 / #208):** `score-zero-impression-articles` persists
winnability to `article_metadata`; prefer `--from-cache` / `--list`; live score
TTL 60d / max 25 per run; DataForSEO SERP also gated by `serp_guard` (14d
keyword cache + 50 live/day/project). **Do not bulk re-score every week** — GSC
desk is outcome ground truth; score-zero is opt-in paid diagnostics. Secondary
only — no auto bulk noindex. Bucket → human compose: Avoid
merge/noindex-with-confirm; Differentiate `fix_content_article -S` ($0); Target
link boost / fix.

**PostHog (default MCP desk):** same-session behavioral layer after GSC desk —
bounce, engagement, top paths, light CWV — used only to re-rank SEO candidates
or flag product friction. Not a CLI tool, not demand truth, not
`posthog-weekly-insights`. **Only** config: **`project.yaml` →
`posthog_project_id`**. Always `switch-project` before query. No fleet maps or
name matching. Missing id or MCP → **WARN + GSC-only**, never silent skip.

**Dual-path freshness:** live ad-hoc probes (`gsc-performance` / `gsc-movers` /
`gsc-queries`) remain available; desk tape is refreshed by `collect_gsc` +
execute (paginated page-daily #262) — sufficient for trustworthy desk totals.
Prefer desk over soft audits when both answer the same question. Desk JSON
exposes `freshness.stale` / `freshness.hint` (and `evidence_coverage`) on
`site-overview` and `articles` — honor those before treating zeros as demand.

**MCP (#92):** mount **desk tools first**; skill = operator policy. Tighten soft
guidance if agents thrash — not hard rails first.

**SEO program ops:** `seo_program.yaml` is the multi-week mode/queue SOT
(schema [docs/SEO_PROGRAM.md](../../../docs/SEO_PROGRAM.md)). Weekly locks
`current_mode` and drains queues; `/seo-program-review` rewrites the board
monthly. Theme gates stay in `project.yaml` — do not re-encode Primary in prose.

**Run spacing (#303):** Phase 0 hard-refuses mode execution when the newest
mode-executing `weekly_seo_*.md` filename is &lt; 5 days old. Measure-only
passes are exempt and do not reset the clock; override requires an explicit
user phrase logged under Decisions. weekly-seo-status surfaces **Runs (7d)** /
cadence collapse retrospectively.
