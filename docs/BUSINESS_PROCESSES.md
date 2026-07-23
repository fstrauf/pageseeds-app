# Business Processes Overview

This document maps the **why** — the core workflows PageSeeds enables for SEO content operations.

Each process represents a user-facing capability with a defined input, transformation, and output. The unifying principle: **close the loop between data and action**. PageSeeds doesn't just show you SEO data — it executes the work that data implies.

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
│          │                       │                      │                   │
│          ▼                       ▼                      ▼                   │
│  ┌──────────────────┐    ┌──────────────────┐    ┌──────────────────┐      │
│  │  INVESTIGATE     │    │  PUBLISH         │    │  FIX             │      │
│  │  (Ask AI)        │    │  (Deploy)        │    │  (Apply)         │      │
│  └──────────────────┘    └──────────────────┘    └──────────────────┘      │
│          │                                               │                  │
│          ▼                                               ▼                  │
│  ┌──────────────────┐                           ┌──────────────────┐       │
│  │  MONITOR         │◀──────────────────────────│  PROMOTE         │       │
│  │  (GSC/Health)    │                           │  (Social/Reddit) │       │
│  └──────────────────┘                           └──────────────────┘       │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 1. Keyword & Landing Page Research Process

**Purpose:** Find new content opportunities with search volume and manageable difficulty.

**Business value:** Eliminates guesswork in content planning. Instead of "what should we write about?", you get a validated shortlist of keywords your domain can realistically rank for.

### Two Research Modes

| Mode | Task Type | Intent | Output |
|------|-----------|--------|--------|
| **Informational** | `research_keywords` | Blog articles (how-to, guides, tutorials) | Curated keyword shortlist |
| **Commercial** | `research_landing_pages` | Landing pages (best, vs, alternative, software) | Landing page candidates |
| **Custom** | `custom_keyword_research` | User-provided themes | Validated keyword list |

### Process Flow

```
Task: research_keywords / research_landing_pages / custom_keyword_research
  ↓
Handler: ResearchHandler
  ↓
Step 1: research_theme_selection_agent (agentic) — IF no explicit themes
  ├─ Reads project brief and context
  ├─ Extracts 3-4 focused themes to research
  └─ Output: {"themes": [...]}
  ↓
Step 2: research_seed_validation (agentic)
  ├─ Validates themes for domain relevance
  ├─ Proposes 1-3 seed phrasings per on-topic theme
  └─ Output: Validated seeds
  ↓
Step 3: keyword_research_native (deterministic)
  ├─ Ahrefs API: keyword ideas + difficulty scores
  ├─ Iterates until 10+ qualified keywords found
  └─ Output: {"difficulty": {"results": [...]}}
  ↓
Step 4: research_final_selection (deterministic)
  ├─ Filters by volume, KD, intent
  ├─ Deduplicates against existing content
  └─ Output: Final selection JSON
  ↓
Status: review (user must select keywords)
  ↓
User selects → create_article_tasks_from_keywords command
  ↓
Creates: write_article tasks for each selected keyword
```

### Key Files
- `engine/workflows/handlers.rs` — workflow definition
- `engine/exec/keywords.rs` — research execution
- `seo/keywords.rs` — Ahrefs API integration
- `components/tasks/KeywordPicker.tsx` — selection UI

### Tools Required
- `CAPSOLVER_API_KEY` + Ahrefs credentials in Settings → Secrets

---

## 2. Content Creation Process

**Purpose:** Write new SEO-optimized articles from keyword targets.

**Business value:** Transforms keyword research directly into publishable content. No brief handoffs, no writer bottlenecks, no copy-paste from ChatGPT. Articles are written in your brand voice, saved to your repo, and tracked in your inventory.

### Supported Content Types

| Task Type | What It Creates |
|-----------|----------------|
| `write_article` | Standard blog article from keyword |
| `optimize_article` | Rewrite/improvement of existing article |
| `create_landing_page` | Commercial landing page |
| `create_content` | Generic content piece |
| `optimize_content` | Content optimization without full rewrite |
| `create_hub_page` | Hub/spoke pillar page (legacy — use skill-based approach) |

### Process Flow

**Desktop / nested path (ContentHandler):**

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

**CLI Path B (preferred for weekly-seo / outer agents — issue #135):**

```
select-keywords  →  write_article tasks (provenance only; do not execute-task)
  ↓
write-context (-I research-task-id -K keyword)
  → deterministic package: content_brief, target_file/path, publish_date,
    content-write skill body, min_words 800 / target_words 1200
  ↓
Session agent writes full MDX to target_file (uses package skill + brief)
  ↓
write-submit (-f path | -S slug) until validation ok
  → structural gates (validate_article, ≥800 words)
  → ingest_orphans + keyword tag + mark write_article done
  → spawn cluster_and_link
```

Path B avoids nested `execute-task write_article` under a weak global provider
(thin single-shot articles). Freeform MDX without submit is not supported —
submit is the quality gate.

### Key Files
- `engine/workflows/handlers.rs` — ContentHandler (nested path)
- `engine/write_package.rs` — CLI Path B package + submit
- `engine/agent.rs` — LLM provider calls (nested path)
- `content/validate_article.rs` — structural floors (shared)
- `content/ops.rs` — MDX file operations

### Output
- New `.mdx` file in project content directory
- Entry in `articles.json`

---

## 3. Content Review & Optimization Process

**Purpose:** Identify and apply improvements to existing content based on performance data.

**Business value:** Content decays — competitors improve, information becomes stale, rankings drop. This process continuously finds the highest-impact articles to improve and applies specific, measurable fixes.

### Two Audit Types

| Audit | Scope | Trigger |
|-------|-------|---------|
| `content_audit` | 21-rule deterministic health check | Manual or scheduled |
| `content_review` | Health check + AI prioritization + recommendations | Manual or scheduled |
| `ctr_audit` | CTR-focused analysis (titles, meta, snippets) | Manual or scheduled |

### Process Flow (Content Review)

```
Task: content_review
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
  └─ 21-rule health check → content_audit.json
  ↓
Step 4: content_review_investigate (agentic, tool-calling when supported)
  ├─ Provider gate: KimiBridge / Claude / OpenAI / Ollama → RO tool investigation
  │   ├─ investigation_kit(ReadOnly) multi-turn agent (≤20 tool calls)
  │   ├─ Typed Extractor → InvestigationFindings
  │   └─ Output: investigation_findings artifact (no recommendations.json)
  └─ Else (e.g. KimiCli): fall back to content_review_recommend
      ├─ Score/select priority articles, per-article structured recommendations
      └─ Output: recommendations.json artifact
  ↓
Status: done
  ↓
Auto-spawns (recommend path only): fix_content_article tasks from recommendations.json
  (investigate path leaves proposed_tasks for a later wiring issue)
  ↓
Each fix_content_article runs 4-step pipeline:
  1. Context (deterministic) — load recommendations + file content
  2. Generate (agentic) — structured ContentFixPatch extraction
  3. Apply (deterministic) — apply patch with snapshot/restore
  4. Verify (deterministic) — re-run health checks
```

### Process Flow (CTR Audit)

```
Task: ctr_audit
  ↓
Handler: CtrAuditHandler
  ↓
Step 1: ctr_analyze (deterministic)
  └─ Score articles by CTR potential
  ↓
Step 2: ctr_fix_generate (agentic)
  └─ Generate structured CTR fix patches
  ↓
Step 3: ctr_fix_apply (deterministic)
  └─ Apply title/meta/snippet fixes
  ↓
Step 4: ctr_verify_fix (deterministic)
  └─ Re-run CTR health checks
```

### Key Files
- `engine/exec/content/` — review orchestration + fix pipeline
- `engine/exec/content_audit.rs` — 21-rule audit
- `engine/exec/ctr_audit/` — CTR optimization pipeline
- `content/dates.rs` — date analysis
- `content/cleaner.rs` — structure validation

### Artifacts
- `content_audit.json` — health scores per article
- `recommendations.json` — suggested improvements
- `ctr_audit.json` — CTR analysis results

---

## 4. Publishing Process

**Purpose:** Transition articles from `ready_to_publish`/`draft` to `published` with proper date handling.

**Business value:** Prevents publishing errors that hurt SEO: duplicate dates, future-dated posts, year mismatches between titles and publication dates. Handles bulk publishing with intelligent date redistribution.

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

---

## 5. GSC Collection & Investigation Process

**Purpose:** Diagnose indexing issues and spawn actionable fix tasks.

**Business value:** Catches technical SEO problems before they crater traffic. Indexing issues, coverage errors, and crawl problems are detected automatically and routed to the right fix workflow.

### Two Separate Operations

**Operation A: URL Inspection (`collect_gsc`)**
- Fetch sitemap URLs
- Call GSC URL Inspection API for each
- Classify: robots_blocked, noindex, fetch_error, canonical_mismatch, etc.
- Spawn fix tasks based on reason codes

**Operation B: Analytics Sync (`gsc_sync_articles`)**
- Fetch Search Analytics (clicks, impressions, CTR)
- Match URLs to articles
- Update articles.json with GSC block

**Operation C: Performance Analysis (`analyze_gsc_performance`)**
- Deep-dive into GSC performance data
- Identify movers, trends, and opportunities

**Operation D: Indexing Recovery (`gsc_indexing_recovery`)**
- Systematic recovery workflow for not-indexed pages
- Internal link fixes, content improvements, re-submission

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

### Key Files
- `engine/exec/gsc.rs` — GSC task execution
- `gsc/indexing.rs` — URL Inspection API
- `gsc/analytics.rs` — Search Analytics API
- `gsc/classification.rs` — reason_code classification

### Artifacts
- `gsc_collection.json` — URL inspection results
- `gsc_summary.json` — grouped counts by reason

---

## 6. CTR Optimization Process

**Purpose:** Improve click-through rates from search results.

**Business value:** Higher CTR = more traffic without ranking improvements. A page ranking #5 with a compelling title can out-click a bland #4 result. This process systematically identifies and fixes underperforming snippets.

> **Agent desk model (epic #117):** Low-CTR patterns **emerge from Site State** (GSC impressions/CTR + catalog title/meta via `site-overview` / `articles` / `article` / `gsc-queries`). The `ctr_audit` pipeline below remains available when the problem is already scoped or desk data clearly warrants the specialist path — it is **not** a required weekly spine. Prefer targeted `fix_content_article` / `content_review` when desk evidence is enough.

### Process Flow

```
Task: ctr_audit
  ↓
Handler: CtrAuditHandler
  ↓
Step 1: ctr_analyze (deterministic)
  ├─ Read GSC data + article frontmatter
  ├─ Score by: title length, meta quality, snippet optimization, FAQ presence
  └─ Output: ctr_audit.json
  ↓
Step 2: ctr_fix_generate (agentic)
  ├─ Load skill → call extraction with structured schema
  └─ Output: CtrFixPatch JSON artifact
  ↓
Step 3: ctr_fix_apply (deterministic)
  ├─ Snapshot → apply patch → validate → restore on corruption
  └─ Modified MDX files
  ↓
Step 4: ctr_verify_fix (deterministic)
  └─ Re-run CTR health checks → report pass/fail
  ↓
Auto-spawns: fix_ctr_article tasks for site-wide template issues
```

### Key Files
- `engine/exec/ctr_audit/` — 4-step fix pipeline
- `components/health/HealthDashboard.tsx` — CTR health display

---

## 7. Cannibalization Detection & Consolidation Process

**Purpose:** Find and resolve keyword cannibalization — multiple pages competing for the same query.

**Business value:** Cannibalization dilutes ranking signals and confuses search engines. Consolidating overlapping content into authoritative pages typically results in stronger rankings and cleaner site architecture.

> **Agent desk model (epic #117):** Same-query / same-intent competition **emerges from Site State** (`gsc-queries` page×query, catalog neighbors). Soft TF-IDF clusters are exploratory only. The `cannibalization_audit` pipeline remains available when hard evidence warrants it — it is **not** the required weekly spine. Never treat soft clusters as merge authority.

### Process Flow

```
Task: cannibalization_audit
  ↓
Handler: CannibalizationHandler
  ↓
Step 1: Load GSC data + article inventory
  ↓
Step 2: Cluster articles by overlapping keywords/target queries
  ↓
Step 3: Score cannibalization severity per cluster
  ↓
Step 4: Generate merge/consolidation recommendations
  ↓
Status: review
  ↓
User approves → consolidate_cluster tasks created
  ↓
Task: consolidate_cluster
  ├─ Merge selected articles into authoritative page
  ├─ Create redirects from deprecated URLs
  └─ Update internal links
```

**CLI Path B (preferred for weekly-seo / outer agents — issue #138):**

```
select-cannibalization / approved keep+redirects
  → consolidate_cluster tasks (provenance only; do not execute-task on happy path)
  ↓
merge-context (-I consolidate-task-id | --keep-id + --redirect-ids | -K + -R)
  → deterministic package: full MDX for keep + sources, outlines, soft GSC,
    merge-content skill body, min_keeper_words 400, requires_human_confirm
  ↓
Session agent writes complete merged MDX to keeper_file (no nested draft_patch)
  ↓
merge-submit until validation ok [-y if high-traffic]
  → structural gates (valid MDX, ≥400 words, no cycle, redirect files exist)
  → high-traffic confirm when keep clicks ≥ 50 or impressions ≥ 1000
  → redirects.csv + inbound link rewrite + depublish sources + sync
  → mark consolidate_cluster done when -I bound
```

Path B avoids nested `execute-task consolidate_cluster` under a weak global
provider (irreversible nested `extract_structured` draft_patch). Session agents
can revise the keeper file and resubmit until gates pass — apply steps run only
after validation (fail closed). Desktop nested merge remains for in-app runs.

### Evidence lanes for merge candidates (fail-closed)

Soft TF-IDF clusters (low similarity threshold in build context; CLI `cannibalization-clusters`) are **exploratory only** — they are **not** merge authority and **not** ground truth for weekly SEO. They fail open on mono-niche sites. The shortlist emits candidates only from three evidence lanes (#117 / #121):

1. **`exact_keyword`** — exact same `target_keyword` groups via `exact_keyword_duplicates.json` (`candidate_type: "exact_keyword_dupe"` — mandatory merge for the strategy skill).
2. **`shared_query`** — same GSC query on ≥2 article_ids via `ctr_query_metrics` with a per-page impression floor (10); real SERP competition (`candidate_type: "shared_query"`).
3. **`near_dupe`** — high pairwise similarity only: embedding neighbors (≥0.85) when `article_evidence` has vectors, else TF-IDF pairs (≥0.45). Emitted as `candidate_type: "near_dupe"` (not soft mega-clusters).

Soft transitive topical cohesion (e.g. mono-niche theme bags) never becomes a top-N traffic grab-bag merge set. Analyze enriches each candidate with article-evidence packages (real `word_count`, `outline_text`, `top_queries`) and applies product guards beyond ID resolution (valid lane, 2–4 pages, multi-intent near_dupe without shared queries forced to `no_action`). The strategy skill must refuse near_dupe by default unless same-intent / same-query evidence is present (prefer hard GSC same-query evidence from desk reads). User picker / `review_surface` is unchanged — merges are never auto-approved.

### Key Files
- `engine/exec/cannibalization/` — detection logic (build context, candidates, analyze, reduce)
- `engine/exec/consolidate_cluster/` — nested consolidation execution (desktop path)
- `engine/merge_package.rs` — CLI Path B package + submit (no LLM)
- `components/cannibalization/CannibalizationReview.tsx` — review UI

---

## 8. Internal Linking & Content Clustering

**Purpose:** Strengthen site architecture by connecting related articles.

**Business value:** Internal links distribute PageRank (or "link equity"), help search engines discover content, and keep users engaged longer. This process autonomously builds "Related Articles" sections and hub/spoke clusters.

### Process Flow

```
Task: cluster_and_link
  ↓
Handler: ClusterLinkHandler
  ↓
Step 1: cluster_link_scan (deterministic)
  ├─ Build link graph from all MDX files
  ├─ Identify orphaned articles
  ├─ Find semantic clusters
  └─ Output: link_graph.json
  ↓
Step 2: cluster_link_generate (agentic)
  ├─ Generate "Related Articles" sections
  └─ Suggest hub/spoke relationships
  ↓
Step 3: cluster_link_apply (deterministic)
  └─ Append related sections to MDX files
```

### Key Files
- `engine/exec/content/cluster_link.rs` — link graph + application
- `content/linking.rs` — link scanning

---

## 9. Reddit Opportunity Process

**Purpose:** Find Reddit posts relevant to your content and engage authentically.

**Business value:** Reddit is high-intent traffic with strong community trust. Done well, Reddit engagement drives qualified visitors and builds brand authority. Done poorly, it gets you banned. This process finds the right conversations and drafts value-first replies.

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
Status: review
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

## 10. Social Media Marketing Process

**Purpose:** Transform content into platform-native social media posts with AI-generated image prompts.

**Business value:** Every article you publish should be promoted across channels. This process generates ready-to-post content for TikTok, Instagram Feed/Reels/Stories, and other platforms — complete with hooks, captions, hashtags, and visual direction.

### Supported Workflows

| Task Type | Purpose |
|-----------|---------|
| `social_generate_campaign` | Full campaign from content sources |
| `social_regenerate_campaign` | Regenerate existing campaign |
| `social_generate_from_article` | Single post from one article |
| `social_regenerate_post` | Regenerate a single post |
| `social_design_template` | Create platform template |
| `social_save_template` | Save template to library |
| `social_create_template` | Create from existing post |

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
  └─ Load platform-specific templates
  ↓
Step 3: social_generate_posts (agentic)
  ├─ For each source × template × platform:
  ├─ Generate: hook, caption, hashtags, CTA
  ├─ Generate: visual_description, overlay_text
  └─ Generate: image_generation_prompt
  ↓
Step 4: social_build_visuals (deterministic)
  └─ Copy source images / prepare assets
  ↓
Step 5: social_save_campaign (deterministic)
  └─ Persist posts to SQLite
  ↓
Status: done
```

### Image Generation Workflow
The app cannot generate images directly. Instead, it produces detailed `image_generation_prompt` fields that users copy into Midjourney, DALL-E, Leonardo, etc. The prompt includes visual style, composition guidance, mood, and "no text in image" directives.

### Key Files
- `engine/exec/social.rs` — Campaign execution
- `social/prompts.rs` — Agent prompts
- `social/db.rs` — Post persistence
- `components/social/` — Campaign UI

---

## 11. Agentic Investigation Process

**Purpose:** Answer open-ended questions about your site's performance with evidence-backed insights.

**Business value:** Pre-defined audits catch known issues. Investigation discovers unknown issues — the template bug hiding in your layout file, the duplicate content you didn't know existed, the CTR pattern that only shows up when you look across data sources.

### How It Works

```
User: "Why am I plateauing at 10K impressions?"
  ↓
Investigation loads tool catalog → builds Rig agent with data tools
  ↓
Agent explores freely (up to 20 tool calls):
  ├─ get_gsc_performance() → impressions flat
  ├─ scan_article_titles() → brand duplicated
  ├─ hash_article_bodies() → 6 exact dupes
  └─ read_framework_files() → template bug
  ↓
Synthesizes findings → structured InvestigationResult
  ↓
Saved to: .github/automation/investigations/{id}/
```

### Available Tools

**Desk / Site State first (epic #117):**
- `site_overview` — Compact site health desk (totals, top pages, movers, hints)
- `articles` / `article` — GSC-aware catalog list and full per-slug package
- `gsc_performance` / `gsc_movers` / `gsc_queries` — Demand and deltas

**Optional / secondary (not ground truth):**
- `article_list` / `article_frontmatter` / `article_body_hash` / `article_title_scan` — Lightweight / deep content inventory
- `content_audit_report` / `run_content_audit` — Full health check data (optional deep)
- `cannibalization_clusters` — Soft clusters only; not merge authority
- `indexing_status` — GSC indexing status
- `ctr_health` — Productized CTR composite; prefer desk GSC metrics when possible
- `framework_files` — Next.js config, sitemap, robots.txt
- `article_link_graph` — Internal linking structure
- `create_task` — Create fix tasks from findings

### Key Files
- `engine/exec/investigate.rs` — Investigation execution
- `engine/tools/` — Rig tool implementations
- `components/health/InvestigationPanel.tsx` — Ask AI UI

---

## 12. Fix Implementation Process

**Purpose:** Address specific issues identified by collection workflows.

**Business value:** The output of every audit and collection process is a set of fix tasks. This process applies those fixes autonomously, with deterministic validation and rollback on failure.

### Spawned By
- `collect_gsc` → `fix_technical`, `fix_indexing`, `fix_gsc_access`
- `content_review` / `content_audit` → `fix_content_article`
- `ctr_audit` → `fix_ctr_article`, `fix_ctr_site_template`
- `cannibalization_audit` → `consolidate_cluster`
- Manual creation → `fix_404s`, `fix_redirects`, `technical_seo`, etc.

### The 4-Step Fix Pipeline (Canonical Pattern)

Every per-article fix follows the same reliable structure:

| Step | Type | Responsibility |
|------|------|----------------|
| 1. Context | Deterministic | Load audit data + read target file → structured JSON |
| 2. Generate | Agentic | Load skill → call `extract_with_backend::<PatchType>()` → validate |
| 3. Apply | Deterministic | Snapshot → apply patch → validate MDX → restore on corruption |
| 4. Verify | Deterministic | Re-run health checks → report pass/fail |

### Key Files
- `engine/workflows/handlers.rs` — ImplementationHandler
- `engine/exec/content/fix_*.rs` — Content fix pipeline
- `engine/exec/ctr_audit/` — CTR fix pipeline

---

## 13. Territory Research & Strategy Process

**Purpose:** Research and plan content territory (topic domain) expansion.

**Business value:** Before committing to a new content vertical, understand the competitive landscape, keyword opportunities, and required investment. This process produces a strategy artifact that feeds into your editorial calendar.

### Process Flow

```
Task: territory_research
  ↓
Handler: TerritoryResearchHandler
  ↓
Step 1: Gather competitive landscape data
  ↓
Step 2: Identify keyword whitespace
  ↓
Step 3: Assess content gaps vs competitors
  ↓
Step 4: Generate territory strategy artifact
  ↓
Status: review
```

---

## 14. Calculator/Tool Rollout Process

**Purpose:** Plan and execute interactive tool (calculator, generator) content.

**Business value:** Interactive tools attract backlinks, rank for high-intent queries, and convert visitors. This process helps plan the content surrounding a tool launch.

### Task Type
- `calculator_rollout` — End-to-end calculator content strategy

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

ctr_audit ────▶ fix_ctr_article ──▶ (title/meta fixes)
       │
       └──▶ fix_ctr_site_template ──▶ (global template fixes)

cannibalization_audit ──approved──▶ consolidate_cluster ──▶ (merge + redirect)

collect_gsc ──issues found──▶ fix_* tasks ──▶ (manual resolution)
       │
       └──▶ investigate_gsc (if all indexed)

reddit_opportunity_search ──enriched──▶ OpportunityFeed ──▶ Reply posted

write_article ──published──▶ social_generate_campaign ──▶ SocialPosts
       │                                                           │
       │                                                           ▼
       └───────────────────── Image Gen Prompt (manual workflow) ─┘

gsc_performance ──▶ analyze_gsc_performance ──▶ (insights + recommendations)

investigate (agentic) ──findings──▶ create_task ──▶ fix_* tasks
```

---

## Status Lifecycle by Process

| Process | Start | Success | Failure |
|---------|-------|---------|---------|
| Keyword Research | todo | **review** (user selects) | todo |
| Content Creation | todo | done | todo |
| Content Review | todo | done (+ spawns apply) | todo |
| CTR Audit | todo | done (+ spawns fixes) | todo |
| Cannibalization Audit | todo | **review** (user approves) | todo |
| GSC Collection | todo | done (+ spawns fixes) | todo |
| Reddit Search | todo | **review** (user selects) | todo |
| Social Campaign | todo | done | todo |
| Investigation | todo | done | todo |
| Fix Tasks | todo | done | todo |

**Critical:** Tasks finishing with `review` status require user action before follow-ups are created. All others go to `done` or reset to `todo` on failure.

---

## See Also

- [Workflow Engine](./WORKFLOW_ENGINE.md) — How processes are executed
- [Workflow Engine](./WORKFLOW_ENGINE.md) — How processes are scheduled and run
- [Data Persistence](./DATA_PERSISTENCE.md) — Where process state lives
- [Agent Integration](./AGENT_INTEGRATION.md) — How AI agents power the agentic steps
