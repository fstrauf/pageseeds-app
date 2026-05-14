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

## 1. Keyword & Landing Page Research Process

**Purpose:** Find new content opportunities (blog articles or landing pages) with search volume and manageable difficulty.

### Two Research Modes

| Mode | Task Type | Intent | Output Format |
|------|-----------|--------|---------------|
| **Informational** | `research_keywords` | Blog articles (how-to, guides, tutorials) | `{"difficulty": {"results": [...]}}` |
| **Commercial** | `research_landing_pages` | Landing pages (best, vs, alternative, software) | `{"landing_page_candidates": [...]}` |

### Inputs
- Project context from `seo_content_brief.md` (for seed extraction)
- Existing `articles.json` (to deduplicate against current content)

### Process Flow (3-Step Agentic)

```
Task: research_keywords OR research_landing_pages
  ↓
Handler: ResearchHandler
  ↓
Step 1: research_seed_extraction (agentic)
  ├─ Reads project brief and context
  ├─ Agent extracts 3-4 themes to research
  └─ Output: {"themes": ["theme1", "theme2", ...]}
  ↓
Step 2: research_keyword_discovery (agentic)
  ├─ Agent uses Ahrefs API tools:
  │   ├─ keyword_generator → get keyword ideas from themes
  │   └─ keyword_difficulty → get KD for top candidates
  ├─ Iterates until 10+ qualified keywords found (max 25 API calls)
  └─ Output: {"keywords": [...]} OR {"landing_page_keywords": [...]}
  ↓
Step 3: research_final_selection (agentic)
  ├─ Filters by volume (>500), KD (<40), intent
  ├─ Deduplicates (no cannibalization)
  └─ Output: Final selection JSON
  ↓
Status: review (user must select keywords)
  ↓
User selects keywords → create_article_tasks_from_keywords command
  ↓
Creates: write_article tasks for each selected keyword
```

### Key Files
- `engine/workflows/handlers.rs` — 3-step workflow definition
- `engine/tool_agent/http_client.rs` — Tool calling agent
- `engine/tools/keywords.rs` — Ahrefs API integration
- `prompts/keyword_discovery.md` — Informational discovery prompt
- `prompts/landing_page_discovery.md` — Commercial discovery prompt
- `components/tasks/KeywordPicker.tsx` — selection UI

### Output Artifacts
- `research_seed_extraction` — Extracted themes
- `research_keyword_discovery` — Keywords with volume/KD data
- `research_final_selection` — Final filtered selection

### Tools Required
- `keyword_generator` — Generate keyword ideas from Ahrefs
- `keyword_difficulty` — Get KD scores from Ahrefs
- **Requires:** `CAPSOLVER_API_KEY` in Settings → Secrets

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
Auto-spawns: fix_content_article tasks (one per recommended article)
  ↓
System runs fix_content_article → agent applies fixes to MDX files
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

## 6. Social Media Marketing Process

**Purpose:** Transform content (articles, screenshots, specs) into platform-native social media posts with AI-generated image prompts.

### Inputs
- Content sources: articles, screenshots, spec files
- Content templates defining tone and format per platform
- Target platforms (TikTok, Instagram Feed/Reels/Stories)

### Process Flow
```
Task: social_generate_campaign
  ↓
Handler: SocialHandler
  ↓
Step 1: social_collect_sources (deterministic)
  ├─ Discover articles from content directory
  ├─ Find screenshots from assets folder
  └─ Build source manifest
  ↓
Step 2: social_load_templates (deterministic)
  └─ Load platform-specific templates (TikTok hooks, IG carousels)
  ↓
Step 3: social_generate_posts (agentic)
  ├─ For each source × template × platform combination:
  ├─ Agent generates: hook, caption, hashtags, CTA
  ├─ Agent generates: visual_description, overlay_text
  └─ Agent generates: image_generation_prompt (for Midjourney/DALL-E)
  ↓
Step 4: social_build_visuals (deterministic)
  ├─ Copy existing source images OR generate branded fallback
  └─ Prepare assets for text overlay
  ↓
Step 5: social_save_campaign (deterministic)
  └─ Persist posts to SQLite with image_generation_prompt
  ↓
Status: done
```

### Image Generation Workflow
Since the app cannot generate images directly, the agent creates a detailed `image_generation_prompt` that users can:

1. **Copy** from the post editor UI
2. **Paste** into Midjourney, DALL-E, Leonardo, or any AI image generator
3. **Download** the generated image
4. **Upload** back to the post (manual or future automation)

The prompt includes:
- Visual style description (minimalist, professional, on-brand colors)
- Composition guidance (aspect ratio matching the platform)
- Mood and subject matter aligned with the post content
- "No text in image" directive (since text will be overlaid separately)

### Key Files
- `engine/exec/social.rs` — Campaign execution logic
- `social/prompts.rs` — Agent prompts for post generation
- `social/generator.rs` — Simple article-to-post generator
- `social/db.rs` — Post persistence with image_generation_prompt
- `components/social/PostEditor.tsx` — UI with image prompt copy button

### Data Model
- `social_campaigns` table — Campaign configuration
- `social_posts` table — Individual posts with:
  - `hook`, `caption`, `hashtags`, `cta` — Text content
  - `visual_assets` — Image/video paths
  - `image_generation_prompt` — AI image prompt for external generation
  - `overlay_text` — Text to render on the image

---

## 7. Reddit Opportunity Process

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
- `content_review` → fix_content_article
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
                "optimize_article" | ...)
    }
    
    fn plan(&self, task: &Task, ctx: &HandlerContext) -> Vec<WorkflowStep> {
        vec![WorkflowStep::new("apply_fix", StepKind::Agentic)
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
       └──▶ fix_content_article ──▶ (updates MDX files)

collect_gsc ──issues found──▶ fix_* tasks ──▶ (manual resolution)
       │
       └──▶ investigate_gsc (if all indexed)

reddit_opportunity_search ──enriched──▶ OpportunityFeed ──▶ Reply posted

write_article ──published──▶ social_generate_campaign ──▶ SocialPosts
       │                                                           │
       │                                                           ▼
       └───────────────────── Image Gen Prompt (manual workflow) ─┘
```

**Social Media Workflow Note:** Since the app cannot generate images directly, the social process produces `image_generation_prompt` fields that users copy into external AI image generators (Midjourney, DALL-E, etc.). The generated images are then manually uploaded back to complete the post.

---

## Status Lifecycle by Process

| Process | Start | Success | Failure |
|---------|-------|---------|---------|
| Keyword Research | todo | **review** (user selects) | todo |
| Content Creation | todo | done | todo |
| Content Review | todo | done (+ spawns apply) | todo |
| GSC Collection | todo | done (+ spawns fixes) | todo |
| Reddit Search | todo | done | todo |
| Social Campaign | todo | done (posts in `draft`) | todo |
| Fix Tasks | todo | done | todo |

**Critical:** Only keyword research tasks finish with `review` status. All others go to `done` or reset to `todo` on failure.

**Social Post Status Flow:** `draft` → `review` → `approved` → `scheduled` → `posted`

---

## See Also

- [Workflow Engine](./WORKFLOW_ENGINE.md) — How processes are executed
- [Task Queue](./TASK_QUEUE.md) — How processes are scheduled and run
- [Data Persistence](./DATA_PERSISTENCE.md) — Where process state lives
