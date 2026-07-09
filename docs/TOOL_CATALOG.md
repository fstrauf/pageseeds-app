# Overview Tool Catalog

The agent-facing reference for the **user-facing capabilities** surfaced on the Overview screen. Each entry is a workflow task type that can be enqueued (not an in-process Rig tool). Use this catalog to decide *which* task to start, then enqueue it via the queue — never execute it directly.

> **Source of truth:** `src-tauri/src/config/task_definitions.rs` owns lifecycle metadata (`run_policy`, `review_surface`, `follow_up_policy`, `handler_family`). The Overview UI mirrors this in `QUICK_ACTIONS` (`src/components/overview/Overview.tsx`). If the two ever disagree, the Rust file wins.

## How to invoke

All capabilities are **task types**, not function calls. Enqueue them; do not execute them.

| Context | API |
|---|---|
| Frontend component | `enqueueTasks([{ taskType, projectId, ... }])` in `src/lib/tauri.ts` |
| Backend follow-up after a task | `TaskSpawner::spawn` / `spawn_follow_up` (`engine/spawner.rs`) |
| Programmatic system task | `TaskSpawner::spawn` (never `task_store::create_task` directly) |

See the [Task Lifecycle Contract](../AGENTS.md#task-lifecycle-contract) for which lane applies.

## Decision guide: which tool when?

```
"What should I do next?"
│
├─ No fresh data / it's been a while
│  └─→ collect_* tasks run automatically (AutoEnqueue). Don't start them manually.
│
├─ Need NEW content topics to write about
│  ├─→ research_keywords            (blog / informational long-tail)
│  └─→ research_landing_pages       (conversion / high-intent pages)
│
├─ Existing content underperforming — need a diagnosis
│  ├─→ content_review               (general: syncs GSC, recommends fixes per article)
│  ├─→ seo_health_scan              (unified: fuses all signals into a ranked opportunity backlog)
│  ├─→ ctr_audit                    (specific: titles/meta/snippets — low CTR)
│  ├─→ cannibalization_audit        (specific: overlapping pages competing)
│  ├─→ indexing_health_campaign     (specific: pages not indexed by Google)
│  └─→ clarity_analytics            (specific: UX/behavioral anomalies from Clarity)
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
- `content_review` is the **umbrella** investigation. Reach for the specific audits (`ctr_audit`, `cannibalization_audit`, `indexing_health_campaign`, `clarity_analytics`) only when the problem is already scoped to that domain.
- `seo_health_scan` is the **unified backlog** investigation: it fuses content audit, CTR, indexing, cannibalization, and Clarity signals into a single ranked opportunity list. Use it when you want a prioritized TODO list across all SEO levers rather than a deep dive into one area.
- `content_cleanup` = broken/structural file problems. `sanitize_content` = rename frontmatter fields (`metaDescription` → `description`). Don't use them for prose or strategy fixes.
- `research_keywords` vs `research_landing_pages`: same picker UX, different intent model. Landing pages are conversion-focused and carry strategic context.

## Catalog

### Research — find new work to do

| Field | `research_keywords` |
|---|---|
| **Does** | Finds new long-tail keyword opportunities via Ahrefs, then presents a picker so the user selects which to write about. |
| **When** | Monthly, or whenever the editorial backlog is thin. Blog/informational intent. |
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
| **After completion** | `FollowUpTasks` → spawns concrete fix tasks automatically (`BackendAuto`). |

| Field | `seo_health_scan` |
|---|---|
| **Does** | Runs content audit, CTR context, cannibalization clusters, indexing contexts, and Clarity summary; scores each article; and writes a ranked `seo_opportunities.json` backlog. |
| **When** | You want a single prioritized list of the biggest SEO opportunities across all levers. |
| **After completion** | `ArtifactReview` (Phase 1), followed by user-selected fix tasks (`UserSelection`). |

| Field | `ctr_audit` |
|---|---|
| **Does** | Analyzes titles, meta descriptions, and snippet readiness; spawns per-article CTR fixes. |
| **When** | The problem is specifically low click-through rate from search results (impressions ok, clicks low). Runs automatically on `AutoEnqueue`. |
| **After completion** | No review surface → spawns `fix_ctr_article` children automatically (`BackendAuto`). |

| Field | `cannibalization_audit` |
|---|---|
| **Does** | Detects overlapping content, finds merge candidates, and identifies hub gaps. |
| **When** | Two or more of your own pages compete for the same query. Runs automatically on `AutoEnqueue`. |
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
| `content_review` | UserEnqueue | FollowUpTasks | BackendAuto |
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
