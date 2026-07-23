# Overview Tool Catalog

The agent-facing reference for the **user-facing capabilities** surfaced on the Overview screen. Each entry is a workflow task type that can be enqueued (not an in-process Rig tool). Use this catalog to decide *which* task to start, then enqueue it via the queue — never execute it directly.

> **Source of truth:** `src-tauri/src/config/task_definitions.rs` owns lifecycle metadata (`run_policy`, `review_surface`, `follow_up_policy`, `handler_family`). The Overview UI mirrors this in `QUICK_ACTIONS` (`src/components/overview/Overview.tsx`). If the two ever disagree, the Rust file wins.

> **Desk model (epic #117):** The primary agent path for weekly organic growth is **Site State reads** (`site-overview` / `articles` / `article` + GSC tools via CLI/investigate) then a **few hard actions**. Specialist audits (`ctr_audit`, `cannibalization_audit`, `seo_health_scan`, etc.) remain available as optional pipelines when the problem is already scoped — they are **not** the default weekly spine. Soft TF-IDF clusters are exploratory only, never merge authority. See the [weekly-seo skill](../.agents/skills/weekly-seo/SKILL.md).

## How to invoke

All capabilities are **task types**, not function calls. Enqueue them; do not execute them.

| Context | API |
|---|---|
| Frontend component | `enqueueTasks([{ taskType, projectId, ... }])` in `src/lib/tauri.ts` |
| Backend follow-up after a task | `TaskSpawner::spawn` / `spawn_follow_up` (`engine/spawner.rs`) |
| Programmatic system task | `TaskSpawner::spawn` (never `task_store::create_task` directly) |
| Weekly SEO operator (CLI) | Desk reads via `pageseeds-cli` then `create-task` / `execute-task` — see weekly-seo skill |

See the [Task Lifecycle Contract](../AGENTS.md#task-lifecycle-contract) for which lane applies.

## Decision guide: which tool when?

```
"What should I do next?"
│
├─ Weekly organic growth / explore the site
│  └─→ Desk path (epic #117): site-overview → articles / article / gsc-*
│       then ≤5 hard actions (fix_content_article, content_review, research_*, indexing…)
│       Do NOT default to running every specialist audit.
│
├─ No fresh data / it's been a while
│  └─→ collect_* tasks run automatically (AutoEnqueue). Don't start them manually.
│       (CLI/agent weekly path may create+execute collect_gsc when desk data is stale.)
│
├─ Need NEW content topics to write about
│  ├─→ CLI Path B: research-context → session seeds → research-pull
│  │     (custom_keyword_research; no nested theme LLM) then select-keywords
│  ├─→ research_keywords            (desktop/UI nested theme agent; informational)
│  └─→ research_landing_pages       (conversion / high-intent pages)
│
├─ Existing content underperforming — cause unknown
│  ├─→ Prefer desk reads (GSC + catalog) and/or content_review (umbrella)
│  ├─→ indexing_health_campaign     when not-indexed is already clear
│  ├─→ clarity_analytics            when UX/behavioral signals are the question
│  ├─→ ctr_audit / cannibalization_audit  only when already scoped or desk
│  │     shows a clear low-CTR / same-query pattern needing that pipeline
│  └─→ seo_health_scan              optional backlog when desk + content_review
│        still insufficient (not the default “brain”)
│
├─ Need to engage an audience off-site
│  └─→ reddit_opportunity_search    (find Reddit posts to reply to)
│
├─ Need to fix the repo itself, not the content strategy
│  ├─→ content_cleanup              (structural MDX issues: headings, broken frontmatter)
│  └─→ sanitize_content             (normalize frontmatter field names)
│
└─ Need to plan a feature for THIS app
   └─→ generate_feature_spec        (agentic investigation → developer spec)
```

**Disambiguation rules:**
- **Desk first for exploration.** CTR and cannibalization signals **emerge from** GSC page×query + catalog (`site-overview` / `articles` / `article` / `gsc-queries`). Specialist tasks are for when the problem is already scoped or desk shows a clear pattern that needs that pipeline — not the default weekly checklist.
- `content_review` is the **umbrella** task investigation. Prefer it (or desk + targeted fixes) over running every specialist audit when the cause is unknown.
- Specialist audits (`ctr_audit`, `cannibalization_audit`, `indexing_health_campaign`, `clarity_analytics`) only when already scoped to that domain.
- `seo_health_scan` is an **optional multi-signal backlog**, not a mandatory unified brain. Use when desk reads + `content_review` are insufficient and you want a ranked cross-lever TODO list.
- Soft cannibalization clusters (CLI/investigate `cannibalization-clusters`) are **not ground truth** and never merge authority.
- `content_cleanup` = broken/structural file problems. `sanitize_content` = rename frontmatter fields (`metaDescription` → `description`). Don't use them for prose or strategy fixes.
- `research_keywords` vs `research_landing_pages`: same picker UX, different intent model. Landing pages are conversion-focused and carry strategic context.

## Catalog

### Research — find new work to do

| Field | CLI Path B (`research-context` / `research-pull`) |
|---|---|
| **Does** | Packages shortlist/health for session strategy; pulls keyword candidates from **explicit seeds** via `custom_keyword_research` (no nested seed extraction/validation LLM). |
| **When** | Weekly CLI / outer-agent path when desk shows gap growth. Session proposes seeds; CLI runs deterministic Ahrefs/DataForSEO pipeline. |
| **After completion** | Same `KeywordPicker` artifacts → `select-keywords` → write Path B. |

| Field | `research_keywords` |
|---|---|
| **Does** | Finds new long-tail keyword opportunities via Ahrefs, then presents a picker so the user selects which to write about. Nested agentic seed extraction/validation. |
| **When** | Desktop/UI, or monthly when the editorial backlog is thin. Blog/informational intent. Prefer `research-pull` on CLI weekly path. |
| **After completion** | `KeywordPicker` review surface → user selects → spawns `write_article` children (`UserSelection`). |

| Field | `research_landing_pages` |
|---|---|
| **Does** | Researches high-intent keywords for conversion-focused landing pages with strategic context. |
| **When** | When the goal is conversion pages rather than blog content. |
| **After completion** | `KeywordPicker` → user selects → spawns `create_landing_page` children (`UserSelection`). |

### Investigation — diagnose existing content

| Field | `content_review` |
|---|---|
| **Does** | Syncs GSC data and generates recommendations for the highest-priority article. |
| **When** | The umbrella content diagnostic. Start here when "something is underperforming" and the cause is unknown. |
| **After completion** | `ContentReviewPicker` → user selects proposals → spawns `fix_content_article` children (`UserSelection`). |

| Field | `seo_health_scan` |
|---|---|
| **Does** | Runs content audit, CTR context, cannibalization clusters, indexing contexts, and Clarity summary; scores each article; and writes a ranked `seo_opportunities.json` backlog. |
| **When** | **Optional** — desk + `content_review` insufficient and you want a single prioritized cross-lever backlog. Not the default weekly spine (epic #117). |
| **After completion** | `ArtifactReview` (Phase 1), followed by user-selected fix tasks (`UserSelection`). |

| Field | `ctr_audit` |
|---|---|
| **Does** | Analyzes titles, meta descriptions, and snippet readiness; spawns per-article CTR fixes. |
| **When** | Problem already scoped to low CTR (impressions ok, clicks low), or desk data already shows that pattern and you need the CTR pipeline. Prefer reading impressions/CTR from Site State first. Runs automatically on `AutoEnqueue`. |
| **After completion** | No review surface → spawns `fix_ctr_article` children automatically (`BackendAuto`). |

| Field | `cannibalization_audit` |
|---|---|
| **Does** | Detects overlapping content, finds merge candidates, and identifies hub gaps. |
| **When** | Problem already scoped, or desk/`gsc-queries` shows the same query on 2+ URLs (hard evidence). Soft TF-IDF clusters alone are not sufficient authority. Runs automatically on `AutoEnqueue`. |
| **After completion** | `CannibalizationPicker` → user chooses merges/hubs → spawns downstream tasks (`UserSelection`). |

| Field | `indexing_health_campaign` |
|---|---|
| **Does** | Unified workflow: checks prerequisites, reviews distinctiveness against cluster siblings, and spawns targeted fixes for non-indexed pages. |
| **When** | Pages exist but Google hasn't indexed them. Prefer this over the granular `fix_indexing*` tasks. |
| **After completion** | `ArtifactReview` → spawns targeted child fix tasks (`BackendAuto`). |

| Field | `clarity_analytics` |
|---|---|
| **Does** | Collects Microsoft Clarity behavioral data, scores pages for UX anomalies, and surfaces ranked findings. |
| **When** | You want on-page UX/behavioral signals (rage clicks, dead clicks, scroll depth) layered onto content decisions. |
| **After completion** | `ArtifactReview` only — surfaces findings, does not spawn work (`None`). |

### Off-site engagement

| Field | `reddit_opportunity_search` |
|---|---|
| **Does** | Searches subreddits for posts to engage with and saves pending opportunities. |
| **When** | Weekly audience engagement. Runs automatically on `AutoEnqueue`. |
| **After completion** | `RedditPicker` → user picks posts → spawns `reddit_reply` children (`UserSelection`). |

### Repo hygiene — fix the files, not the strategy

| Field | `content_cleanup` |
|---|---|
| **Does** | Scans MDX files for structural issues — heading duplicates, broken frontmatter. |
| **When** | As-needed maintenance when file structure is suspected corrupt. |
| **After completion** | No review surface, no follow-ups (`None`). |

| Field | `sanitize_content` |
|---|---|
| **Does** | Normalizes frontmatter field names (e.g. `metaDescription` → `description`) across all MDX files. |
| **When** | After ingesting content from another system with non-standard frontmatter. |
| **After completion** | No review surface, no follow-ups (`None`). |

### App self-improvement

| Field | `generate_feature_spec` |
|---|---|
| **Does** | Agentic investigation of the project that produces a prioritized developer feature specification. |
| **When** | Planning what to build next in this app. Not a content task. Runs automatically on `AutoEnqueue`. |
| **After completion** | No review surface, no follow-ups — the spec itself is the output (`None`). |

## Lifecycle cheat sheet

| Task type | Run policy | Review surface | Follow-up policy |
|---|---|---|---|
| `research_keywords` | UserEnqueue | KeywordPicker | UserSelection |
| `research_landing_pages` | UserEnqueue | KeywordPicker | UserSelection |
| `content_review` | UserEnqueue | ContentReviewPicker | UserSelection |
| `seo_health_scan` | UserEnqueue | ArtifactReview | UserSelection |
| `ctr_audit` | AutoEnqueue | None | BackendAuto |
| `cannibalization_audit` | AutoEnqueue | CannibalizationPicker | UserSelection |
| `indexing_health_campaign` | UserEnqueue | ArtifactReview | BackendAuto |
| `clarity_analytics` | UserEnqueue | ArtifactReview | None |
| `reddit_opportunity_search` | AutoEnqueue | RedditPicker | UserSelection |
| `content_cleanup` | UserEnqueue | None | None |
| `sanitize_content` | UserEnqueue | None | None |
| `generate_feature_spec` | AutoEnqueue | None | None |

## Related

- [Task Lifecycle Contract](../AGENTS.md#task-lifecycle-contract) — the four lanes for creating/queuing/spawning tasks.
- [Business Processes](./BUSINESS_PROCESSES.md) — how these workflows connect end to end.
- [Workflow Engine](./WORKFLOW_ENGINE.md) — handlers, steps, executor mechanics.
- [weekly-seo skill](../.agents/skills/weekly-seo/SKILL.md) — desk-first weekly operator path (epic #117).
