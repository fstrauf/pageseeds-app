# Cannibalization Growth System Spec

**Status:** Proposed  
**Date:** 2026-04-27  
**Scope:** Cannibalization audit, content consolidation, hub pages, programmatic calculators, new territories, internal linking, and redirect execution.

---

## Problem

PageSeeds needs a reliable workflow for breaking an impressions plateau caused by topic saturation and self-competition.

The current `cannibalization_audit` implementation is a useful start, but it is still mostly a recommendation scaffold. It can trigger an audit, compute rough overlap signals, ask an agent for strategy, and spawn follow-up tasks. It does not yet provide the data quality, approval flow, deterministic execution, redirect safety, hub architecture, or programmatic SEO controls needed to safely run the full strategy.

The target system should help answer and execute four growth moves:

| Growth Move | Goal | Required Outcome |
|---|---|---|
| Merge cannibal pages | Stop multiple articles competing for the same SERP slots | Approved keeper/redirect map, merged content, 301 rules, validation |
| Build hub pages | Concentrate topical authority into broad pillar pages | Hub briefs, hub content, spoke links, homepage and cross-hub linking |
| Programmatic calculators | Open ticker + strategy query inventory | Calculator templates, data pipeline, indexable page rollout, sitemap controls |
| Open new territories | Stop publishing into saturated clusters | Territory map, demand evidence, content plans, task creation |

---

## Design Principle

Use deterministic steps wherever the input/output mapping is computable. Use agentic steps wherever the system must interpret intent, weigh tradeoffs, write prose, or make strategy decisions.

The system should not avoid agentic steps. The goal is not "make everything deterministic." The goal is to prevent agents from doing mechanical work poorly, and prevent rules from making judgment calls blindly.

### Boundary Rules

| Work Type | Mode | Reason |
|---|---|---|
| Fetch GSC metrics | Deterministic | API call with repeatable output |
| Crawl pages and extract titles/H1/meta/canonicals | Deterministic | File/network parsing |
| Count words, headings, links, backlinks, impressions | Deterministic | Pure data extraction or math |
| Compute similarity scores | Deterministic | TF-IDF, embeddings, MinHash, or Jaccard are algorithms |
| Build candidate clusters | Deterministic | Graph/grouping by thresholds and shared query overlap |
| Decide true cannibalization vs topical overlap | Agentic | Requires intent and SERP judgment |
| Choose keeper vs redirect targets | Agentic | Requires weighing authority, content quality, URL quality, recency, and brand fit |
| Merge unique content into a keeper | Agentic draft, deterministic apply | Prose selection is judgment; patching/validation is mechanical |
| Generate redirect rules from approved mappings | Deterministic | Mapping to config syntax is mechanical once approved |
| Decide hub topics and outlines | Agentic | Requires taxonomy and search-intent judgment |
| Write hub page prose | Agentic | Open-ended content generation |
| Validate hub links/frontmatter/schema | Deterministic | Structural checks |
| Generate ticker calculator data | Deterministic | Price/options data and formulas |
| Choose rollout priority and indexability rules | Agentic + deterministic guardrails | Strategy judgment plus fixed safety thresholds |
| Research new territories | Deterministic data collection, agentic strategy | Keyword/competitor data is fetched; territory selection is judgment |

---

## Current State To Fix

The existing implementation should be treated as Phase 0 of this project.

| Current Behavior | Problem | Required Fix |
|---|---|---|
| `can_build_context` writes a rich context file but passes only summary pairs/groups to the agent | Agent lacks impressions, snippets, dates, word counts, link data, and URLs needed for keeper decisions | Pass the full structured context or a compact per-cluster context with all decision fields |
| Handler comments say TF-IDF/cosine but implementation uses Jaccard word sets | Spec and code disagree, and Jaccard is too weak for intent clustering | Implement TF-IDF/cosine or update the spec explicitly; prefer TF-IDF plus shared-query overlap |
| Audit emits similarity pairs, not prioritized clusters | Pair lists are hard for agents and users to act on | Build deterministic connected clusters with total impressions, clicks, positions, and shared queries |
| Hub and territory analysis are prompt-only | Agent is asked to infer structure without structured evidence | Add deterministic `hub_gap_detect` and `territory_analysis` artifacts |
| Follow-up tasks are generic `implementation_agent_stage` or manual fallback | Spawned tasks do not execute real merge/hub/territory workflows | Add dedicated handlers and skills for merge, hub, territory, calculators |
| Redirects are only analyzed, not generated/applied | Consolidation cannot safely ship | Add platform-aware redirect generation from approved mappings |
| No cannibalization review UI | User cannot approve keepers, redirects, or hub plans clearly | Add a review surface before destructive/applying tasks |
| No calculator system exists | Programmatic SEO growth move is missing | Add calculator template/data/sitemap/indexing workflow |

---

## Product Model

The system should have two layers:

1. **Investigation layer:** Produces facts, clusters, recommendations, and projections. No content or redirect changes.
2. **Execution layer:** Applies approved changes with deterministic validation and rollback where possible.

No destructive consolidation should run directly from the audit. The audit can create draft fix tasks, but merge/redirect application requires explicit approval of a merge map.

---

## Workflow Overview

```text
cannibalization_growth_audit
  -> can_collect_inventory          deterministic
  -> can_sync_gsc                   deterministic
  -> can_build_similarity           deterministic
  -> can_build_query_overlap        deterministic
  -> can_detect_clusters            deterministic
  -> can_detect_hub_gaps            deterministic
  -> can_analyze_territories        deterministic
  -> can_strategy                   agentic
  -> can_normalize_strategy         deterministic
  -> can_create_review_plan         deterministic

approved_review_plan
  -> consolidate_cluster tasks
  -> create_hub_page tasks
  -> calculator_rollout tasks
  -> territory_research tasks
  -> internal_link_architecture task
```

---

## Workflow 1: Cannibalization Growth Audit

**Task type:** `cannibalization_growth_audit`  
**Phase:** `investigation`  
**Execution mode:** `automatic` for data collection and strategy generation  
**Writes:** `cannibalization_growth_context.json`, `cannibalization_strategy.json`, review-ready task artifacts

### Steps

| Step | Mode | Output | Why This Mode |
|---|---|---|---|
| `can_collect_inventory` | Deterministic | `page_inventory.json` | Reads repo/live-site inventory, frontmatter, content excerpts, canonicals, status, dates |
| `can_sync_gsc` | Deterministic | `gsc_page_metrics.json`, `gsc_query_metrics.json` | API calls and URL matching |
| `can_scan_link_graph` | Deterministic | `link_graph.json` | Counts incoming/outgoing links, anchors, orphan pages, hub candidates |
| `can_build_similarity` | Deterministic | `similarity_matrix.json` | TF-IDF/cosine or embeddings over title, H1, target keyword, intro, headings |
| `can_build_query_overlap` | Deterministic | `query_overlap.json` | Compares GSC query sets per page to detect same-SERP competition |
| `can_detect_clusters` | Deterministic | `cannibalization_clusters.json` | Groups by shared target keyword, high similarity, high shared-query overlap |
| `can_detect_hub_gaps` | Deterministic | `hub_gaps.json` | Checks whether broad hubs already exist and whether clusters have parent pages |
| `can_analyze_territories` | Deterministic | `territory_analysis.json` | Counts saturated themes and maps existing coverage to configured/new territory candidates |
| `can_strategy` | Agentic | raw strategy JSON | Decides true cannibalization, keeper/redirect mapping, hub topics, territory priorities |
| `can_normalize_strategy` | Deterministic | `cannibalization_strategy.json` | Extracts and validates JSON contract |
| `can_create_review_plan` | Deterministic | `cannibalization_review_plan.json` | Converts strategy to reviewable actions and draft follow-up tasks |

### Agentic Step: `can_strategy`

The agent must receive structured context, not raw bulk files.

Input contract:

```json
{
  "site_summary": {
    "total_pages": 156,
    "total_impressions": 575000,
    "period_days": 90
  },
  "clusters": [
    {
      "cluster_id": "cash_secured_puts_best_stocks",
      "theme": "cash-secured puts",
      "candidate_intent": "best stocks for cash-secured puts",
      "total_impressions": 142319,
      "total_clicks": 91,
      "shared_query_count": 42,
      "hub_exists": false,
      "pages": [
        {
          "id": 12,
          "url": "/blog/best-stocks-csp",
          "title": "Best Stocks for Cash-Secured Puts",
          "h1": "Best Stocks for Cash-Secured Puts",
          "target_keyword": "best stocks for cash-secured puts",
          "impressions": 71802,
          "clicks": 51,
          "ctr": 0.0007,
          "avg_position": 5.5,
          "word_count": 3200,
          "incoming_internal_links": 18,
          "outgoing_internal_links": 7,
          "published_date": "2026-01-10",
          "first_200_words": "..."
        }
      ],
      "top_shared_queries": ["best stocks for cash secured puts"]
    }
  ],
  "hub_gaps": [],
  "territory_analysis": {},
  "calculator_opportunities": {}
}
```

Output contract:

```json
{
  "merge_recommendations": [
    {
      "cluster_id": "cash_secured_puts_best_stocks",
      "confidence": "high",
      "keep_url": "/blog/best-stocks-csp",
      "redirect_urls": ["/blog/cash-secured-puts-playbook"],
      "merge_before_redirect": true,
      "merge_instructions": [
        "Move the risk-management table from /blog/cash-secured-puts-playbook into the keeper.",
        "Preserve the brokerage-specific example as a subsection."
      ],
      "reason": "Keeper has highest impressions, cleanest URL, strongest internal link count, and best position."
    }
  ],
  "hub_recommendations": [
    {
      "topic": "cash-secured puts",
      "suggested_url": "/hub/cash-secured-puts",
      "suggested_title": "Cash-Secured Puts: Complete Guide",
      "intent": "broad pillar",
      "source_pages": [12, 18, 22],
      "spoke_pages": [12, 18, 22, 31],
      "outline": ["What CSPs are", "Best stocks", "Strike selection", "Risks", "Calculators"]
    }
  ],
  "calculator_recommendations": [
    {
      "strategy": "cash-secured-put",
      "ticker_universe": "sp500",
      "priority_tickers": ["AAPL", "MSFT", "NVDA"],
      "indexing_policy": "index when option data and unique computed tables are available",
      "reason": "Ticker + strategy combinations open new long-tail query inventory."
    }
  ],
  "territory_recommendations": [
    {
      "theme": "broker-specific options guides",
      "priority": "high",
      "demand_evidence": ["existing IBKR guide has impressions", "keyword ideas show broker modifiers"],
      "suggested_tasks": ["How to sell covered calls on Schwab", "Fidelity vs Schwab for options sellers"]
    }
  ],
  "risks": [
    {
      "risk": "Merging a page with distinct intent may reduce long-tail coverage.",
      "mitigation": "Require shared-query overlap or agent high-confidence label before redirect."
    }
  ]
}
```

### Required Validation

- Every merge recommendation must name a keeper URL and at least one redirect URL.
- Every keeper and redirect URL must exist in inventory.
- Every redirect URL must be different from the keeper.
- Every recommended hub URL must not collide with an existing non-hub article unless explicitly marked as an update.
- Recommendations with low confidence should create review tasks only, never automatic apply tasks.

---

## Workflow 2: Consolidate Cluster

**Task type:** `consolidate_cluster`  
**Phase:** `implementation`  
**Execution mode:** `spec` or `manual_review_required`  
**Goal:** Merge duplicate/cannibal pages into an approved keeper and generate redirects.

### Steps

| Step | Mode | Output | Why This Mode |
|---|---|---|---|
| `merge_load_plan` | Deterministic | approved merge plan | Reads approved strategy artifact |
| `merge_preflight` | Deterministic | preflight report | Confirms files exist, URLs resolve, no redirect cycles, keeper is indexable |
| `merge_extract_unique_sections` | Deterministic | section inventory | Splits redirect pages into headings, tables, examples, FAQs |
| `merge_draft_patch` | Agentic | `ContentMergePatch` JSON | Decides which unique content belongs in keeper and writes prose transitions |
| `merge_apply_patch` | Deterministic | modified keeper draft | Applies structured patch, snapshots original, validates MDX/frontmatter |
| `merge_generate_redirects` | Deterministic | redirect config patch | Converts approved redirect map to platform-specific syntax |
| `merge_validate_output` | Deterministic | validation report | Confirms keeper builds, redirected pages removed/noindexed, links updated |

### Agentic Step: `merge_draft_patch`

Agentic is correct here because the system must understand whether a section adds unique value or just repeats the keeper. It must also rewrite transitions naturally.

The agent may propose content changes, but it must not directly write files. It returns a structured patch that deterministic code applies.

### Redirect Rules

Redirect generation is deterministic after approval.

Supported target adapters should be added incrementally:

| Adapter | Output |
|---|---|
| `next_config` | `redirects()` entries |
| `vercel_json` | `redirects` array |
| `netlify_redirects` | `_redirects` lines |
| `cloudflare_bulk` | CSV or JSON rules |
| `generic_csv` | source,destination,status export |

If the app cannot detect the platform, use `generic_csv` and leave the task in review.

---

## Workflow 3: Hub Page System

**Task types:** `hub_gap_audit`, `create_hub_page`, `refresh_hub_page`  
**Goal:** Build a hub-and-spoke architecture around saturated topics.

### Hub Gap Detection

| Step | Mode | Output | Why This Mode |
|---|---|---|---|
| `hub_load_clusters` | Deterministic | cluster inventory | Reads cannibalization/coverage clusters |
| `hub_detect_existing_pages` | Deterministic | hub candidates | Finds `/hub/`, `/guide/`, broad slugs, high incoming-link pages |
| `hub_score_gaps` | Deterministic | hub gap scores | Flags clusters with 3+ spokes and no broad parent |
| `hub_strategy` | Agentic | hub recommendations | Decides whether cluster needs new hub, refreshed pillar, or no hub |

### Hub Creation

| Step | Mode | Output | Why This Mode |
|---|---|---|---|
| `hub_build_brief` | Deterministic | structured brief | Gathers spokes, metrics, headings, gaps, calculators, competitor notes |
| `hub_outline` | Agentic | outline JSON | Designs information architecture and narrative flow |
| `hub_write` | Agentic | MDX draft | Writes broad pillar content |
| `hub_apply_links` | Deterministic | link patches | Adds parent-hub links from spokes and spoke links from hub |
| `hub_validate` | Deterministic | validation report | Checks frontmatter, no duplicate H1, required links, schema, word count |

### Hub Requirements

- Hub URL must target a broader keyword than the spokes.
- Hub must link to all approved spoke pages.
- Every spoke should link back to its parent hub with a descriptive anchor.
- Hub should include calculator links where relevant.
- Hubs should avoid absorbing distinct high-performing sub-intents that deserve their own page.

---

## Workflow 4: Programmatic Calculator System

**Task types:** `calculator_template_plan`, `calculator_data_sync`, `calculator_page_rollout`, `calculator_indexing_audit`  
**Goal:** Generate high-quality strategy + ticker calculator pages without creating thin duplicate pages.

### Core Principle

Programmatic pages should be deterministic at scale, but agentic during template design, strategy prioritization, and quality review.

Do not run an agent once per ticker by default. That would be expensive, inconsistent, and hard to validate. Use an agent to design the page template and quality rules, then deterministic code generates pages from data.

### Calculator Rollout Steps

| Step | Mode | Output | Why This Mode |
|---|---|---|---|
| `calc_select_universe` | Deterministic | ticker list | Uses configured S&P 500/Russell/custom list |
| `calc_fetch_market_data` | Deterministic | price/IV/options data | API calls and caching |
| `calc_compute_metrics` | Deterministic | premium, ROC, breakeven, DTE table | Financial formulas |
| `calc_template_strategy` | Agentic | template spec | Decides page layout, explanatory sections, risk framing, SERP intent |
| `calc_generate_pages` | Deterministic | MDX/routes/data files | Applies approved template to ticker data |
| `calc_quality_gate` | Deterministic | index/noindex report | Blocks thin pages with missing data or duplicate content |
| `calc_sample_review` | Agentic | QA report | Reviews representative pages for usefulness and search intent |
| `calc_sitemap_publish` | Deterministic | sitemap/routes update | Adds only index-approved pages |

### Indexability Guardrails

Calculator pages may be indexable only when they include:

- Ticker-specific current price or delayed price.
- Ticker-specific options metrics, or a clear unavailable-data state marked `noindex`.
- Strategy-specific computed tables for at least one expiration window.
- Unique title, H1, canonical URL, and meta description.
- Links back to the relevant hub and calculator index.
- Legal/financial disclaimer.

Pages with missing option data, stale data beyond the configured freshness window, or duplicate fallback copy should be generated as `noindex` or not generated.

### Initial Calculator Types

| Strategy | Example URL | Core Metrics |
|---|---|---|
| Cash-secured put | `/calculator/cash-secured-put/AAPL` | premium, cash required, breakeven, ROC, annualized ROC |
| Covered call | `/calculator/covered-call/MSFT` | premium, upside cap, downside buffer, yield, annualized yield |
| Wheel strategy | `/calculator/wheel/NVDA` | put entry, assignment basis, call exit, monthly income range |

---

## Workflow 5: New Territory Research

**Task type:** `territory_research`  
**Phase:** `research`  
**Goal:** Identify growth areas with real query demand and low overlap with existing content.

### Steps

| Step | Mode | Output | Why This Mode |
|---|---|---|---|
| `territory_load_existing_map` | Deterministic | coverage map | Reads articles, hubs, calculators, clusters |
| `territory_collect_keyword_ideas` | Deterministic | keyword candidates | Calls keyword provider/autocomplete APIs |
| `territory_collect_competitors` | Deterministic | competitor SERPs | Fetches SERP/competitor data where available |
| `territory_score_candidates` | Deterministic | scored candidate list | Scores by volume, difficulty, overlap, business fit inputs |
| `territory_strategy` | Agentic | territory plan | Chooses themes and article angles based on intent and differentiation |
| `territory_spawn_tasks` | Deterministic | research/write tasks | Creates approved follow-up tasks with idempotency keys |

### Territory Categories

Initial territory taxonomy for the options use case:

- Broker-specific guides.
- Tax and accounting.
- Portfolio allocation and risk management.
- Market regime strategies.
- Stock-specific/ticker-specific guides.
- Linkable assets such as calculators, spreadsheets, and data studies.

The taxonomy should be configurable per project. The options categories are defaults, not hardcoded universal rules.

---

## Workflow 6: Internal Link Architecture

**Task type:** `internal_link_architecture`  
**Goal:** Move from flat random linking to hub-and-spoke linking.

### Steps

| Step | Mode | Output | Why This Mode |
|---|---|---|---|
| `link_scan` | Deterministic | link graph | Existing internal link scanner plus live-site graph |
| `link_assign_parent_hubs` | Agentic | parent hub mapping | Decides which hub each article belongs to when ambiguous |
| `link_generate_plan` | Deterministic | link patches | Creates exact hub/spoke/calculator link additions from mapping |
| `link_apply_plan` | Deterministic | changed files | Applies related/internal link blocks safely |
| `link_validate` | Deterministic | validation report | Confirms hubs and spokes link as required |

### Link Rules

- Homepage or top navigation should link to primary hubs where the project supports it.
- Each hub links to all approved spokes and related calculators.
- Each spoke links to its parent hub.
- Each spoke links to 2-3 closely related spokes when relevant.
- Each spoke links to one relevant calculator when a calculator exists.
- Internal links should point to final canonical URLs, not redirected URLs.

---

## Data Model Additions

### Cannibalization Strategy Types

Add typed models for frontend review and backend validation.

```ts
interface CannibalizationStrategy {
  generated_at: string
  merge_recommendations: MergeRecommendation[]
  hub_recommendations: HubRecommendation[]
  calculator_recommendations: CalculatorRecommendation[]
  territory_recommendations: TerritoryRecommendation[]
  risks: StrategyRisk[]
}

interface MergeRecommendation {
  cluster_id: string
  confidence: 'high' | 'medium' | 'low'
  keep_url: string
  redirect_urls: string[]
  merge_before_redirect: boolean
  merge_instructions: string[]
  reason: string
  approval_status: 'pending' | 'approved' | 'rejected' | 'needs_review'
}
```

Rust structs should live in `src-tauri/src/models/` with `#[ts(export)]` if they cross IPC.

### Review State

Persist approval state separately from raw agent output:

- `strategy_id`
- `project_id`
- `recommendation_type`
- `recommendation_id`
- `approval_status`
- `approved_by`
- `approved_at`
- `notes`

This prevents rerunning the audit from erasing review decisions.

---

## UI Requirements

### Cannibalization Review View

The user should not read raw JSON to approve SEO consolidation.

Required panels:

- Cluster overview sorted by total impressions.
- Keeper vs redirect comparison table.
- Shared queries and similarity evidence.
- Agent reasoning and confidence.
- Unique-content merge checklist.
- Approve/reject buttons per recommendation.
- Bulk create tasks from approved recommendations.

### Hub Review View

- Proposed hub URL/title.
- Spoke list with impressions and current links.
- Outline preview.
- Missing parent-link count.
- Create hub task button.

### Calculator Rollout View

- Strategy selector.
- Ticker universe selector.
- Data freshness status.
- Indexable/noindex counts.
- Sample page preview links.
- Sitemap publish gate.

### Territory View

- Saturated themes.
- Open territory candidates.
- Demand evidence.
- Existing coverage overlap.
- Create research/write tasks.

---

## Safety And Approval

### Must Require Approval

- Any redirect map.
- Any deletion/removal of source content.
- Any bulk generation of more than 50 indexable pages.
- Any sitemap publish for programmatic pages.
- Any canonical change.

### Can Run Automatically

- GSC sync.
- Inventory crawl.
- Similarity computation.
- Link graph scan.
- Query overlap computation.
- Draft strategy generation.
- Validation checks.

### Rollback Expectations

- Merge apply should snapshot edited files before changes.
- Redirect generation should produce a diff or patch before writing when possible.
- Programmatic rollout should support disabling an entire strategy/ticker universe from the sitemap.
- Every apply task should write a validation artifact.

---

## Implementation Plan

### Phase 1: Make The Audit Trustworthy

- Replace summary-only agent context with full structured cluster context.
- Implement deterministic clusters with total impressions/clicks/position.
- Add query overlap from GSC page-query data.
- Add link graph metrics to cluster pages.
- Add `hub_gaps.json` and `territory_analysis.json`.
- Update `cannibalization-strategy` skill input contract.
- Add tests for cluster creation, shared-query overlap, and missing GSC data behavior.

### Phase 2: Add Review And Approval

- Add typed strategy models.
- Persist approval state.
- Build cannibalization review UI.
- Stop auto-spawning destructive apply tasks directly from unapproved strategy output.
- Spawn draft tasks only after recommendations are approved.

### Phase 3: Consolidation Execution

- Add `consolidate_cluster` handler.
- Add merge-specific skill.
- Add deterministic preflight, patch application, redirect generation, and validation.
- Support `generic_csv` redirect export first.
- Add platform adapters incrementally.

### Phase 4: Hub System

- Register dedicated task types: `hub_gap_audit`, `create_hub_page`, and `refresh_hub_page`.
- Add `HubGapAuditHandler` and `HubPageHandler` workflow handlers. Do not route approved hub recommendations through generic `fix_*` tasks.
- Add `StepKind` variants and executors for:
  - `hub_load_recommendation` — deterministic; loads the approved hub recommendation and spoke set.
  - `hub_build_brief` — deterministic; gathers spoke metadata, GSC metrics, link graph data, headings, excerpts, existing hub candidates, calculators, and competitor notes when available.
  - `hub_outline` — agentic; returns a structured hub outline and linking strategy.
  - `hub_write` — agentic; returns an MDX draft or structured content patch.
  - `hub_apply_draft` — deterministic; writes the hub file, snapshots existing file if refreshing, and updates `articles.json` metadata.
  - `hub_apply_links` — deterministic; adds hub-to-spoke links and spoke-to-hub links from the approved mapping.
  - `hub_validate` — deterministic; validates frontmatter, H1/title, canonical slug, no duplicate hub, required links, and minimum content completeness.
- Add hub-specific skills:
  - `hub-outline` for information architecture, search intent, and spoke grouping.
  - `hub-write` for broad pillar prose that does not cannibalize spoke pages.
- Define typed artifacts:
  - `hub_gap_report.json` with candidate cluster, spoke IDs, total impressions, existing hub candidates, and reason.
  - `hub_brief.json` with selected hub URL/title, target broad keyword, spokes, excerpts, metrics, link requirements, and calculators.
  - `hub_outline.json` with sections, intended spoke links, excluded sub-intents, and rationale.
  - `hub_link_plan.json` with exact source files, target URLs, anchors, and insertion hints.
  - `hub_validation_report.json` with pass/fail checks and blocking issues.
- Review flow:
  - Approved `hub` recommendations create `create_hub_page` tasks.
  - Existing hub candidates create `refresh_hub_page` tasks instead of duplicate hubs.
  - Low-confidence or route-collision recommendations stay in review and cannot spawn apply tasks.
  - The review UI should show spoke list, impressions, current incoming/outgoing links, proposed URL/title, outline preview, and missing parent-link count.
- Quality gates:
  - Hub URL must be broader than the spokes and must not target a spoke keyword directly.
  - Hub must link to every approved spoke and relevant calculator/index page.
  - Every spoke must link back to the hub with a descriptive anchor unless the file is missing or no safe insertion point exists.
  - Hub must not merge away distinct sub-intents that should remain spokes.
  - Hub draft must pass MDX/frontmatter validation and cannot create duplicate H1/title collisions.
- Tests:
  - Unit-test hub gap scoring with clusters that have and lack existing hubs.
  - Unit-test route collision handling for `/hub/...`, `/guide/...`, and project-configured hub routes.
  - Unit-test link-plan generation and idempotent link application.
  - Add a workflow test proving an approved hub recommendation spawns `create_hub_page` and executes the hub handler, not the generic implementation fallback.

### Phase 5: Territory Research

- Add configurable territory taxonomy.
- Add keyword/competitor collection.
- Add agentic territory strategy step.
- Spawn research and writing tasks only from approved territory recommendations.

### Phase 6: Programmatic Calculators

- Define calculator data provider interface.
- Add strategy formulas and deterministic metric computation.
- Add agentic template strategy step.
- Generate sample pages first.
- Add deterministic quality gate and sitemap controls.
- Roll out in batches, starting with priority tickers.

---

## Acceptance Criteria

The feature is ready when:

- A cannibalization audit produces prioritized clusters with full GSC, link, and content evidence.
- The agent sees enough context to justify keep/redirect decisions with exact URLs.
- Low-confidence recommendations require review and cannot auto-apply.
- Approved merge recommendations can create `consolidate_cluster` tasks.
- Consolidation tasks produce merge patches and redirect maps without manual JSON editing.
- Hub recommendations can create hub page tasks with outlines, spokes, and validation.
- The internal link architecture can verify hub-to-spoke and spoke-to-hub links.
- Territory research distinguishes saturated themes from genuinely new opportunities.
- Calculator rollout can generate sample pages with real computed data and block thin pages.
- All generated strategy artifacts are typed enough for frontend review.

---

## Non-Goals For First Pass

- Fully automatic deletion of old content.
- Fully automatic publishing of 1,500 calculator pages.
- Ranking guarantees or traffic forecasts presented as certainty.
- Platform-specific redirect support for every hosting provider on day one.
- Replacing human approval for high-impact SEO changes.

---

## Open Questions

- Which production platforms must redirect generation support first?
- What market/options data provider should calculator pages use?
- Should calculator pages live in the user's repo as MDX/routes, or should PageSeeds generate data files consumed by the site framework?
- What threshold should define shared-query cannibalization: query overlap count, Jaccard over query sets, or weighted impression overlap?
- Should hubs be created as `/hub/...`, `/guide/...`, or project-configurable routes?
- How many programmatic pages can be safely submitted in the first sitemap batch?
