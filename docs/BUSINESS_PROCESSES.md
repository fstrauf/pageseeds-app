# Business Processes Overview

This document maps the **why** — the core workflows PageSeeds enables for SEO content operations.

Each process represents a user-facing capability with a defined input, transformation, and output.

---

## Process Map

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         PAGESEEDS BUSINESS PROCESSES                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌──────────────────┐    ┌──────────────────┐    ┌──────────────────┐      │
│  │  DISCOVER        │───▶│  CREATE          │───▶│  OPTIMIZE        │      │
│  │  (Keywords)      │    │  (Content)       │    │  (Existing)      │      │
│  └──────────────────┘    └──────────────────┘    └──────────────────┘      │
│          │                                               │                  │
│          ▼                                               ▼                  │
│  ┌──────────────────┐                           ┌──────────────────┐       │
│  │  MONITOR         │◀──────────────────────────│  PUBLISH         │       │
│  │  (GSC/Analytics) │                           │  (Deploy)        │       │
│  └──────────────────┘                           └──────────────────┘       │
│          │                                                                  │
│          ▼                                                                  │
│  ┌──────────────────┐                                                       │
│  │  PROMOTE         │                                                       │
│  │  (Reddit/Social) │                                                       │
│  └──────────────────┘                                                       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 1. Keyword Research Process

**Purpose:** Find new content opportunities with search volume and manageable difficulty.

### Inputs
- Keyword themes (comma/newline separated in task description)
- Existing `articles.json` (to deduplicate against current content)
- Optional: min volume, max KD filters

### Process Flow
```
Task: research_keywords or custom_keyword_research
  ↓
Handler: ResearchHandler
  ↓
Step: keyword_research_cli (deterministic)
  ├─ Fetch keyword ideas from Ahrefs (per theme)
  ├─ Deduplicate against existing articles.json
  ├─ Batch difficulty analysis for top candidates
  └─ Output: KeywordResearchResult JSON
  ↓
Status: review (user must select keywords)
  ↓
User selects keywords → create_article_tasks_from_keywords command
  ↓
Creates: write_article tasks for each selected keyword
```

### Key Files
- `engine/exec/keywords.rs` — native keyword pipeline
- `seo/keywords.rs` — Ahrefs API integration
- `components/tasks/KeywordPicker.tsx` — selection UI

### Output Artifacts
- `keyword_research_result` artifact on task (JSON with themes, candidates, difficulty)

### Task Types
- `research_keywords` — standard research
- `custom_keyword_research` — agentic theme curation (ends in review like standard)

---

## 2. Content Creation Process

**Purpose:** Write new SEO-optimized articles from keyword targets.

### Inputs
- Selected keyword from research
- SKILL.md from project automation directory
- Optional: content brief, style guidelines

### Process Flow
```
Task: write_article
  ↓
Handler: ContentHandler
  ↓
Step Plan:
  1. Agentic step with SKILL.md — generates article
  2. Normalizer — extracts structured content
  3. Deterministic — writes MDX file to content directory
  ↓
Auto-spawns: cluster_and_link task (if successful)
```

### Key Files
- `engine/workflows/handlers.rs` — ContentHandler
- `engine/agent.rs` — LLM provider calls
- `content/ops.rs` — MDX file operations

### Output
- New `.mdx` file in project content directory
- Entry in `articles.json`

---

## 3. Content Review & Optimization Process

**Purpose:** Identify and apply improvements to existing content based on performance data.

### Why This Exists
Content drifts — GSC positions drop, competitors improve, information becomes stale. This process finds high-impact articles and recommends specific fixes.

### Inputs
- `articles.json` with GSC analytics
- Content audit results (13-rule health check)
- Scoring formula: GSC position × impressions × CTR gaps × health × staleness

### Process Flow
```
Task: content_review (or content_audit)
  ↓
Handler: ContentReviewHandler
  ↓
Step 1: content_sync (deterministic)
  └─ Validate articles.json ↔ MDX files
  ↓
Step 2: gsc_sync_articles (deterministic) 
  └─ Pull latest GSC metrics into articles.json
  ↓
Step 3: content_audit (deterministic)
  └─ 13-rule health check → content_audit.json
  ↓
Step 4: content_review_recommend (agentic)
  ├─ Select priority articles (top 5-10 by score)
  ├─ Build context: GSC snapshot + failing checks + source excerpt
  ├─ Single agent call with structured prompt
  └─ Output: recommendations.json artifact
  ↓
Status: done
  ↓
Auto-spawns: content_review_apply task (one per review)
  ↓
User runs content_review_apply → agent applies fixes to MDX files
```

### Key Files
- `engine/exec/content.rs` — review orchestration
- `engine/exec/content_audit.rs` — audit logic
- `content/dates.rs` — date analysis
- `content/cleaner.rs` — structure validation

### Artifacts
- `content_audit.json` — health scores per article
- `recommendations.json` — suggested improvements

---

## 4. Publishing Process

**Purpose:** Transition articles from `ready_to_publish`/`draft` to `published` with proper date handling.

### Problem This Solves
- Multiple articles can't share the same publish date
- Future dates are invalid
- Titles may reference years that don't match publish dates

### Process Flow
```
User selects articles in UI → clicks Publish
  ↓
Preflight (deterministic):
  ├─ Structural scan (duplicate H1s, missing frontmatter)
  ├─ Date analysis (future dates, duplicates, missing)
  ├─ Calculate date redistribution (2-day spacing for recent dates)
  └─ Detect year mismatches (title year vs publish year)
  ↓
If year mismatches exist:
  └─ Agentic resolution (update title vs backdate publish)
  ↓
User confirms → Apply publish:
  ├─ Fix structural issues
  ├─ Apply date fixes and resolutions
  ├─ Set status = "published"
  ├─ Patch MDX frontmatter
  └─ Export updated articles.json
```

### Key Files
- `content/publish.rs` — preflight + apply logic
- `content/dates.rs` — date calculation
- `components/articles/PublishPanel.tsx` — UI

### Deterministic vs Agentic Split
| Step | Type | Reason |
|------|------|--------|
| Structural scan | Deterministic | Rule-based |
| Date analysis/redistribution | Deterministic | Arithmetic |
| Year mismatch resolution | Agentic | Requires editorial judgment |

---

## 5. GSC Collection & Investigation Process

**Purpose:** Diagnose indexing issues and spawn actionable fix tasks.

### Two Separate Operations

**Operation A: URL Inspection (collect_gsc)**
- Fetch sitemap URLs
- Call GSC URL Inspection API for each
- Classify: robots_blocked, noindex, fetch_error, canonical_mismatch, etc.
- Spawn fix tasks based on reason codes

**Operation B: Analytics Sync (gsc_sync_articles)**
- Fetch Search Analytics (clicks, impressions, CTR)
- Match URLs to articles
- Update articles.json with GSC block

### Process Flow (URL Inspection)
```
Task: collect_gsc
  ↓
Handler: CollectionHandler
  ↓
Step: collect_gsc_inspect (deterministic)
  ├─ Resolve site_url from manifest
  ├─ Fetch sitemap (support sitemapindex one level deep)
  ├─ Batch inspect URLs (up to 200)
  ├─ Classify into reason_codes with priorities
  └─ Write gsc_collection.json artifact
  ↓
Auto-spawns fix tasks (up to 20):
  ├─ robots_blocked, noindex, fetch_error → fix_technical
  ├─ not_indexed_* → fix_indexing
  ├─ api_error → fix_gsc_access
  └─ All indexed → investigate_gsc (one task)
```

### Process Flow (Investigation)
```
Task: investigate_gsc
  ↓
Handler: InvestigationHandler
  ↓
Step: gsc_summarise (deterministic)
  └─ Group gsc_collection.json by reason_code
  ↓
Step: gsc_investigate_agentic (agentic)
  ├─ Load gsc_summary.json
  ├─ Agent interprets patterns
  └─ Output: investigation recommendations
```

### Key Files
- `engine/exec/gsc.rs` — GSC task execution
- `gsc/indexing.rs` — URL Inspection API
- `gsc/analytics.rs` — Search Analytics API
- `gsc/classification.rs` — reason_code classification

### Artifacts
- `gsc_collection.json` — URL inspection results
- `gsc_summary.json` — grouped counts by reason

---

## 6. Reddit Opportunity Process

**Purpose:** Find Reddit posts relevant to your content and engage authentically.

### Inputs
- `reddit_config.md` in project automation directory
- Config defines: keywords, topics, subreddits, excluded subreddits

### Process Flow
```
Task: reddit_opportunity_search
  ↓
Handler: RedditHandler
  ↓
Step 1: reddit_config_parse (agentic)
  ├─ Agent reads reddit_config.md
  └─ Extracts: trigger_keywords[], seed_subreddits[], excluded[]
  ↓
Step 2: reddit_search (deterministic)
  ├─ Query Reddit JSON API for each keyword
  ├─ Compute engagement + accessibility scores
  └─ Persist raw posts to SQLite
  ↓
Inline enrichment loop (after search succeeds):
  ├─ Batch posts to Kimi
  ├─ Relevance scoring (1-10)
  ├─ Pain point extraction
  ├─ Content match suggestions
  └─ Reply draft generation
  ↓
Status: done
```

### Key Files
- `engine/exec/reddit.rs` — Reddit execution logic
- `reddit/search.rs` — Reddit JSON API
- `reddit/db.rs` — Opportunity persistence
- `components/reddit/OpportunityFeed.tsx` — UI

### Data Model
- `reddit_opportunities` table in SQLite
- Posts enriched with: relevance_score, reply_draft, content_suggestions

---

## 7. Fix Implementation Process

**Purpose:** Address specific issues identified by collection workflows.

### Spawned by
- `collect_gsc` → fix_technical, fix_indexing, fix_gsc_access
- `content_review` → content_review_apply
- Manual creation → fix_404s, fix_redirects, etc.

### Handler Routing
The `ImplementationHandler` catches all task types starting with `fix_`:
```rust
// handlers.rs
pub struct ImplementationHandler;
impl WorkflowHandler for ImplementationHandler {
    fn can_handle(&self, task: &Task) -> bool {
        task.task_type.starts_with("fix_") 
            || matches!(task.task_type.as_str(), 
                "content_review_apply" | "optimize_article" | ...)
    }
    
    fn plan(&self, task: &Task, ctx: &HandlerContext) -> Vec<WorkflowStep> {
        vec![WorkflowStep::new("apply_fix", "agentic")
            .with_param("skill", "apply_fix")]
    }
}
```

### Key Files
- `engine/workflows/handlers.rs` — ImplementationHandler

---

## Process Interconnections

```
research_keywords ──selected──▶ write_article ──success──▶ cluster_and_link
       │                                                           │
       │                                                           ▼
       │                                                    internal_linking
       ▼
content_review ◀────────────── content_audit ◀─────────────── publish
       │
       └──▶ content_review_apply ──▶ (updates MDX files)

collect_gsc ──issues found──▶ fix_* tasks ──▶ (manual resolution)
       │
       └──▶ investigate_gsc (if all indexed)

reddit_opportunity_search ──enriched──▶ OpportunityFeed ──▶ Reply posted
```

---

## Status Lifecycle by Process

| Process | Start | Success | Failure |
|---------|-------|---------|---------|
| Keyword Research | todo | **review** (user selects) | todo |
| Content Creation | todo | done | todo |
| Content Review | todo | done (+ spawns apply) | todo |
| GSC Collection | todo | done (+ spawns fixes) | todo |
| Reddit Search | todo | done | todo |
| Fix Tasks | todo | done | todo |

**Critical:** Only keyword research tasks finish with `review` status. All others go to `done` or reset to `todo` on failure.

---

## See Also

- [Workflow Engine](./WORKFLOW_ENGINE.md) — How processes are executed
- [Task Queue](./TASK_QUEUE.md) — How processes are scheduled and run
- [Data Persistence](./DATA_PERSISTENCE.md) — Where process state lives
