# SEO Improvement Workflow Spec

**Status:** Proposed  
**Date:** 2026-04-24  
**Reference:** `daystoexpiry_SEO_Summary.md`

---

## Problem Statement

The SEO summary for daystoexpiry.com reveals two classic patterns that apply to most content sites:

| Problem | Symptom | Root Cause |
|---------|---------|------------|
| **Clicks stuck** | 575K impressions -> 581 clicks (0.10% CTR) | Titles truncated, no rich snippets, no featured snippet targeting, missing meta descriptions |
| **Impressions stuck** | New posts do not grow total visibility | Content cannibalization -- 120 posts compete for the same ~200 queries |

The fixes are well-categorized in the summary: **CTR fixes** (no new content needed) and **impression fixes** (structural/content changes). This spec translates each fix into a deterministic/agentic workflow step using the existing PageSeeds engine.

---

## Core Principle: Deterministic-First, Agentic-Second

Per AGENTS.md and WORKFLOW_ENGINE.md:

> 1. Deterministic step: collect data, compute metrics, filter, rank, group
> 2. Agentic step: interpret, recommend, write prose using structured output from step 1

**External API calls are deterministic.** Calling GSC, crawling pages, counting words -- deterministic. The step that interprets what to do about the results is agentic.

---

## Existing Capabilities Inventory

Before proposing new work, here is what already exists that maps to these problems:

| Capability | File | Maps To SEO Problem |
|---|---|---|
| Content audit (15 checks) | `engine/exec/content_audit.rs` | Title keyword, H1 keyword, meta desc presence/length, readability, passive voice |
| GSC sync (90-day metrics) | `engine/exec/gsc.rs` | Impressions, clicks, CTR, position per page |
| Content review recommend | `engine/exec/content.rs` | Selects priority articles, agent generates recommendations |
| Coverage cluster analysis | `engine/exec/coverage.rs` | Article clustering by semantic similarity |
| Internal link scan | `content/linking.rs` | Outgoing/incoming links, orphan detection |
| Quality rater | `engine/exec/quality_rater.rs` | Content quality scoring |
| Readability analysis | `content/readability.rs` | Flesch, SMOG, passive voice, cliches |
| Keyword research | `engine/exec/keywords.rs` | Keyword ideas, difficulty, volume |
| Competitor analysis | `content/competitor.rs` | Word count comparison, heading extraction |
| Opportunity scoring | `seo/scoring.rs` | Multi-factor keyword scoring |

---

## Gap Analysis: What is Missing

Despite the rich existing toolkit, several gaps prevent a fully automated version of the SEO summary analysis:

| Gap | Why It Blocks Automation | Severity |
|---|---|---|
| **Title length + brand duplication detection** | Audit checks title contains keyword but not length (< 60 chars) or duplicated brand suffixes | High |
| **Meta description quality scoring** | Audit checks presence and 50-155 char length, but not quality pattern | High |
| **FAQ schema detection** | No structured data scanning at all | High |
| **Featured snippet readiness** | No check for 40-60 word direct answer at article top | High |
| **Content cannibalization detection** | Coverage clusters articles, but does not flag multiple articles targeting the same keyword with actionable merge recommendations | High |
| **Content similarity scoring** | No TF-IDF or embedding-based similarity between articles to quantify overlap | Medium |
| **Hub page gap detection** | Coverage analysis clusters but does not identify missing pillar pages | Medium |
| **Topical territory saturation** | No analysis of how many articles already cover X topic vs what topics have 0 coverage | Medium |
| **CTR-at-risk scoring** | No dedicated scoring formula for high impressions, low CTR pages | Medium |
| **SERP feature detection** | No tracking of which pages own featured snippets | Low |

---

## Proposed Workflows

Three new workflow handlers + extensions to existing ones. Each workflow is designed to be runnable on any project (workspace or live-site) and produces actionable fix tasks.


### Workflow 1: ctr_audit -- Fix Clicks (CTR)

**Task type:** ctr_audit  
**Phase:** investigation  
**Execution mode:** automatic (runs without user intervention, spawns fix tasks)

**Purpose:** Systematically identify and queue fixes for all CTR issues: titles, meta descriptions, FAQ schema, featured snippet targeting.

**Handler plan (6 steps):**

Step 1 (deterministic): Sync latest GSC data.  
Why deterministic: API call, repeatable, no judgment. Uses existing GscSyncArticles step kind.

Step 2 (deterministic): Run title analysis.  
Checks: length > 60 chars (truncation risk), brand suffix duplication, keyword presence, stop-word bloat. Writes title_analysis.json.  
Why deterministic: all checks are computable rules. Requires new step kind: TitleMetaAnalysis.

Step 3 (deterministic): Run meta description + snippet readiness analysis.  
Checks: meta desc presence/length/quality pattern, FAQ schema detection, featured snippet readiness (40-60 word direct answer in first 100 words). Writes snippet_analysis.json.  
Why deterministic: regex + length + structure checks. Requires new step kind: SnippetAnalysis.

Step 4 (deterministic): Compute CTR-at-risk score for every page.  
Formula: impressions > 200 AND ctr < 0.5% -> high priority. Ranks pages by clicks lost = impressions * (target_ctr - actual_ctr). Writes ctr_priority.json.  
Why deterministic: pure math on GSC data.

Step 5 (agentic): Generate prioritized recommendations.  
Input contract: title_analysis.json + snippet_analysis.json + ctr_priority.json  
Output contract: JSON with recommendations array per article, each with fixes list  
Why agentic: deciding WHICH fixes to apply to WHICH pages requires judgment about tradeoffs. Cannot be rule-based: impact depends on query intent, competition, and page purpose.  
Uses skill: ctr-optimization

Step 6 (normalizer): Enforce output contract. Writes ctr_recommendations artifact.

**Auto-spawned follow-up tasks:**

On completion, the handler reads ctr_recommendations.json and spawns:

| Task Type | Title | Idempotency Key |
|---|---|---|
| fix_title_meta | Fix titles and meta descriptions for N pages | ctr_fix:title_meta:{project_id} |
| fix_faq_schema | Add FAQ schema to N pages | ctr_fix:faq:{project_id} |
| fix_snippet_bait | Add snippet-bait openings to N pages | ctr_fix:snippet:{project_id} |

Each fix task gets the relevant subset of recommendations as an artifact.

---

### Workflow 2: cannibalization_audit -- Fix Impressions (Break the Plateau)

**Task type:** cannibalization_audit  
**Phase:** investigation  
**Execution mode:** automatic

**Purpose:** Detect content cannibalization, identify merge candidates, find hub page gaps, and recommend new topical territories.

**Handler plan (7 steps):**

Step 1 (deterministic): Load all articles + GSC data. Uses existing GscSyncArticles and CoverageLoadArticles step kinds.

Step 2 (deterministic): Compute content similarity matrix. Uses TF-IDF on article titles + target_keywords + H1s. Produces similarity pairs > 0.7 threshold. Why deterministic: TF-IDF is pure math.

Step 3 (deterministic): Detect cannibalization clusters. Groups articles by: (a) same target_keyword, OR (b) similarity > 0.7. Ranks clusters by total impressions (higher = more urgent to fix). Writes cannibalization_clusters.json. Why deterministic: grouping by shared keyword or similarity threshold is computable.

Step 4 (deterministic): Detect hub page gaps. For each cluster with >= 3 articles, checks if a hub/pillar page exists. A hub page is identified by: broader keyword, links to >= 3 cluster articles, or URL path containing /hub/ or /guide/. Writes hub_gaps.json. Why deterministic: structural check against existing inventory.

Step 5 (deterministic): Detect topical territory saturation. Counts articles per theme (from target_keyword prefixes + title analysis). Flags themes with > 10 articles as saturated. Flags themes with 0 articles as open territory. Writes territory_analysis.json. Why deterministic: counting + thresholding.

Step 6 (agentic): Generate merge strategy + expansion plan.  
Input contract: cannibalization_clusters.json + hub_gaps.json + territory_analysis.json  
Output contract: merge_recommendations, hub_recommendations, territory_recommendations  
Why agentic: deciding which article is the keeper in a merge requires judgment about authority (backlinks, internal links, traffic history), content quality, and brand alignment. Cannot be reduced to a single metric.  
Uses skill: cannibalization-strategy

Step 7 (normalizer): Enforce output contract. Writes cannibalization_strategy artifact.

**Auto-spawned follow-up tasks:**

| Task Type | Title | Idempotency Key |
|---|---|---|
| fix_content_merge | Merge N cannibalized page clusters | can_fix:merge:{project_id} |
| fix_hub_page | Create N hub pages for topic clusters | can_fix:hub:{project_id} |
| research_territory | Research new topical territories | can_fix:territory:{project_id} |

---

### Workflow 3: site_health_snapshot -- One-Command Full Diagnostic

**Task type:** site_health_snapshot  
**Phase:** investigation  
**Execution mode:** batchable

**Purpose:** Run ALL existing audits + new CTR/cannibalization audits in one workflow. Produces a unified report comparable to the daystoexpiry SEO Summary.

**Handler plan:**

Step 1-3 (deterministic, optional): Existing audits  
- snapshot_gsc_sync (GscSyncArticles)  
- snapshot_content_audit (ContentAudit)  
- snapshot_coverage (CoverageClusterAnalysis)

Step 4-6 (deterministic, optional): New audits  
- snapshot_title_analysis (TitleMetaAnalysis)  
- snapshot_snippet_analysis (SnippetAnalysis)  
- snapshot_cannibalization (deterministic similarity + clustering)

Step 7 (agentic): Synthesize everything into a human-readable SEO summary.  
Input: all audit artifacts  
Output: markdown report matching the daystoexpiry summary format  
Why agentic: interpreting the interaction between metrics (e.g., high impressions + low CTR + title truncation = THIS specific recommendation) requires holistic judgment. A rule engine would need combinatorial explosion of rules.  
Uses skill: seo-summary-synthesis

Step 8 (normalizer): Enforce output contract. Writes seo_summary artifact.

**Output:** seo_summary.md artifact with sections matching the reference:
- The Two Problems (symptom + root cause table)
- Part 1 -- Fix Clicks (CTR) with specific fixes + pages to apply
- Part 2 -- Fix Impressions with merge list + hub recommendations
- 30-Day Sprint schedule
- Key Numbers to Track Weekly

---

## New Step Kinds Required

| Step Kind | String | Used By |
|---|---|---|
| TitleMetaAnalysis | title_meta_analysis | ctr_audit, site_health_snapshot |
| SnippetAnalysis | snippet_analysis | ctr_audit, site_health_snapshot |
| CannibalizationDetect | cannibalization_detect | cannibalization_audit |
| HubGapDetect | hub_gap_detect | cannibalization_audit |
| TerritoryAnalysis | territory_analysis | cannibalization_audit |
| SeoSummarySynthesize | seo_summary_synthesize | site_health_snapshot |

---


## New Execution Modules Required

### 1. engine/exec/title_meta.rs (~400 lines)

**Deterministic.** Analyzes every article's title and meta description.

**Checks:**

| Check | Rule | Output Field |
|---|---|---|
| Title length | len <= 60 chars | title_too_long: bool |
| Brand suffix duplication | Regex detects repeated brand phrases (e.g. "Days to Expiry | Days to Expiry") | brand_duplicated: bool |
| Title truncation risk | len > 55 (Google typically truncates at 55-60) | truncation_risk: bool |
| Title keyword position | Keyword appears in first 30 chars | keyword_front_loaded: bool |
| Meta description CTA | Contains soft CTA words: learn, discover, find out, read, see | has_cta: bool |
| Meta description benefit | Contains benefit words: boost, increase, improve, save, reduce | has_benefit: bool |
| Meta description pattern score | 0-3 scale: has keyword (1) + has benefit (1) + has CTA (1) | pattern_score: u8 |

**Output JSON:** title_analysis.json

Example structure:
```json
{
  "generated_at": "2026-04-24T...",
  "total_articles": 120,
  "issues": {
    "titles_too_long": 45,
    "brand_duplicated": 23,
    "truncation_risk": 52,
    "meta_desc_no_cta": 67,
    "meta_desc_no_benefit": 78
  },
  "articles": [
    {
      "id": 1,
      "url_slug": "naked-puts-vs-csp",
      "title": "Naked Puts vs Cash-Secured Puts | Days to Expiry | Days to Expiry -- Option Selling Analyzer",
      "title_length": 92,
      "title_too_long": true,
      "brand_duplicated": true,
      "truncation_risk": true,
      "meta_description": "...",
      "meta_desc_length": 0,
      "meta_desc_pattern_score": 0,
      "has_cta": false,
      "has_benefit": false
    }
  ]
}
```

---

### 2. engine/exec/snippet.rs (~300 lines)

**Deterministic.** Analyzes snippet readiness: FAQ schema and featured snippet targeting.

**Checks:**

| Check | Rule | Output Field |
|---|---|---|
| FAQ schema present | Body contains ## FAQ or ## Frequently Asked Questions with >= 3 Q/A pairs | faq_present: bool, faq_count: u8 |
| FAQ structured data | Frontmatter has faqSchema: true or body contains JSON-LD script tag | faq_structured: bool |
| Featured snippet bait | First non-heading paragraph is 40-60 words AND contains a direct answer pattern | snippet_bait_present: bool |
| Direct answer pattern | Starts with "[Keyword] is...", "Yes,", "No,", "The best...", numbered list, or table | snippet_pattern_type: String |
| Article type match | "X vs Y" -> paragraph snippet, "best X" -> list snippet, comparison -> table snippet | recommended_snippet_type: String |

**Output JSON:** snippet_analysis.json

Example structure:
```json
{
  "generated_at": "2026-04-24T...",
  "total_articles": 120,
  "issues": {
    "no_faq": 98,
    "no_snippet_bait": 87,
    "wrong_snippet_type": 23
  },
  "articles": [
    {
      "id": 1,
      "url_slug": "naked-puts-vs-csp",
      "faq_present": false,
      "faq_count": 0,
      "faq_structured": false,
      "snippet_bait_present": false,
      "first_para_word_count": 28,
      "recommended_snippet_type": "paragraph",
      "snippet_pattern_type": "none"
    }
  ]
}
```

---

### 3. engine/exec/cannibalization.rs (~500 lines)

**Deterministic.** Detects overlapping content and quantifies similarity.

**Algorithm:**

1. TF-IDF vectorization on [title, h1, target_keyword, first_200_words] per article
2. Cosine similarity between all pairs
3. Clustering: Articles with similarity > 0.7 OR identical target_keyword form a cluster
4. Ranking: Clusters sorted by total impressions (from GSC data)

**Output JSON:** cannibalization_clusters.json

Example structure:
```json
{
  "generated_at": "2026-04-24T...",
  "total_articles": 120,
  "cluster_count": 8,
  "articles_in_clusters": 34,
  "clusters": [
    {
      "cluster_id": 1,
      "theme": "cash-secured-puts",
      "article_count": 11,
      "total_impressions": 125000,
      "articles": [
        { "id": 42, "url_slug": "best-stocks-csp", "title": "...", "impressions": 45000, "similarity_to_winner": 1.0 },
        { "id": 43, "url_slug": "cash-secured-puts-strategy-explained", "title": "...", "impressions": 1200, "similarity_to_winner": 0.82 }
      ]
    }
  ]
}
```

---

### 4. engine/exec/hub_gap.rs (~200 lines)

**Deterministic.** Identifies missing hub/pillar pages.

**Algorithm:**

For each cluster with >= 3 articles:
1. Check if any article has broader keyword (shorter, fewer qualifiers)
2. Check if any article URL contains /hub/ or /guide/ or /pillar/
3. Check if any article links to >= 50% of cluster members
4. If none match -> gap detected

**Output JSON:** hub_gaps.json

Example structure:
```json
{
  "hub_gaps": [
    {
      "theme": "cash-secured-puts",
      "cluster_size": 11,
      "hub_exists": false,
      "suggested_url": "/hub/cash-secured-puts",
      "suggested_title": "Cash-Secured Puts: The Complete Guide",
      "articles_to_link": [42, 43, 44, 45]
    }
  ]
}
```

---

### 5. engine/exec/territory.rs (~250 lines)

**Deterministic.** Maps topical coverage saturation.

**Algorithm:**

1. Extract themes from target_keywords using prefix clustering:
   - "cash-secured-puts strategy" -> theme: "cash-secured-puts"
   - "best stocks for CSP" -> theme: "cash-secured-puts"
2. Count articles per theme
3. Themes with > 10 articles = saturated
4. Themes with 0 articles in a parent category = open territory

**Output JSON:** territory_analysis.json

Example structure:
```json
{
  "themes": [
    { "theme": "cash-secured-puts", "article_count": 11, "status": "saturated" },
    { "theme": "covered-calls", "article_count": 9, "status": "saturated" },
    { "theme": "broker-reviews", "article_count": 0, "status": "open" },
    { "theme": "options-taxes", "article_count": 1, "status": "open" }
  ],
  "saturated_count": 4,
  "open_count": 12
}
```

---


## New SKILL.md Prompts Required

### ctr-optimization SKILL.md

Role: You are an SEO specialist focused on improving click-through rate from search results.

Input: Structured JSON containing title_analysis.json, snippet_analysis.json, and ctr_priority.json.

Output Contract: Return JSON with recommendations array. Each recommendation contains article_id, url_slug, priority, expected_ctr_improvement, and fixes array. Each fix has type (title_rewrite, meta_description, faq_schema, snippet_bait), current, recommended, and reason.

Rules:
1. Prioritize pages with highest "clicks lost" first.
2. For title rewrites: keep under 55 chars, front-load keyword, remove brand duplication.
3. For meta descriptions: use pattern [Keyword] + [specific benefit] + [soft CTA], 140-155 chars.
4. For FAQ schema: suggest 3-5 questions that match actual search queries.
5. For snippet bait: match the article type (X vs Y -> paragraph, best X -> list, comparison -> table).
6. Limit to top 20 pages by priority.

### cannibalization-strategy SKILL.md

Role: You are a content strategist deciding which pages to keep, merge, or redirect.

Input: Structured JSON containing cannibalization_clusters.json, hub_gaps.json, and territory_analysis.json.

Output Contract: Return JSON with three arrays:
- merge_recommendations: each with cluster_theme, keep_url, redirect_urls, merge_instructions, reason
- hub_recommendations: each with topic, suggested_url, suggested_title, articles_to_link, outline_suggestion
- territory_recommendations: each with theme, opportunity, suggested_articles, priority

Rules:
1. KEEP the article with: highest impressions, most backlinks (if known), best URL, most recent publish date.
2. REDIRECT all others to the keeper with 301.
3. MERGE unique content from redirect targets into the keeper before redirecting.
4. HUB pages should link to all cluster articles and target broader keywords.
5. NEW TERRITORIES should be themes with 0-1 existing articles and real search demand.

### seo-summary-synthesis SKILL.md

Role: You are a senior SEO analyst producing a client-ready summary report.

Input: ALL audit artifacts from a site health snapshot.

Output Contract: Return a markdown document with these exact sections:
1. ## The Two Problems (table: Problem | Symptom | Root Cause)
2. ## Part 1 -- Fix Clicks (CTR) (table: Fix | What to do | Pages to apply it to)
3. ## Part 2 -- Fix Impressions (Break the Plateau) (table: Fix | What to do | Why it works)
4. ## The Merge List (Do This Week) (table: Keep (winner) | 301 Redirect)
5. ## 30-Day Sprint (table: Week | Actions | Focus)
6. ## Key Numbers to Track Weekly (table: Metric | Current | 30-day target | 90-day target)
7. ## One-Sentence Strategy

Rules:
1. Use ONLY data from the provided artifacts. Do not invent metrics.
2. Format numbers with K/M suffixes (e.g., 575K, 1.2M).
3. Be specific: name exact URLs and article titles, not generics.
4. Targets should be ambitious but realistic based on current trajectory.

---

## Files to Create / Modify

### New Rust files
| File | Lines | Purpose |
|---|---|---|
| engine/exec/title_meta.rs | ~400 | Title/meta description analysis |
| engine/exec/snippet.rs | ~300 | FAQ schema + snippet readiness detection |
| engine/exec/cannibalization.rs | ~500 | Content similarity + cannibalization clustering |
| engine/exec/hub_gap.rs | ~200 | Missing pillar page detection |
| engine/exec/territory.rs | ~250 | Topical saturation analysis |
| engine/exec/ctr_priority.rs | ~150 | CTR-at-risk scoring |

### Modified Rust files
| File | Change |
|---|---|
| engine/workflows/step_kind.rs | Add 6 new StepKind variants |
| engine/workflows/handlers.rs | Add 3 new handlers (CtrAudit, CannibalizationAudit, SiteHealthSnapshot) |
| engine/executor.rs | Add match arms for new step kinds in run_step() |
| engine/exec/mod.rs | Re-export new modules |
| config/mod.rs | Add ctr_audit, cannibalization_audit, site_health_snapshot to TASK_TYPES |
| commands/mod.rs | Register new commands |
| lib.rs | Register new commands in generate_handler! |
| engine/spawner.rs | Add auto-spawn logic for new follow-up task types |

### New SKILL.md files
| File | Purpose |
|---|---|
| .github/automation/skills/ctr-optimization.md | CTR optimization recommendations |
| .github/automation/skills/cannibalization-strategy.md | Merge strategy + expansion plan |
| .github/automation/skills/seo-summary-synthesis.md | Site health summary report |

### Frontend files
| File | Change |
|---|---|
| src/lib/types.ts | Add types for new audit outputs |
| src/lib/tauri.ts | Add invoke wrappers for new commands |
| src/components/tasks/TaskCreate.tsx | Add new task types to creation UI |

---

## Implementation Order

| Phase | What | Est. Effort | Ships Value |
|---|---|---|---|
| 1 | title_meta.rs + snippet.rs + ctr_priority.rs + CtrAuditHandler | 2 days | CTR issue detection + fix task spawning |
| 2 | cannibalization.rs + hub_gap.rs + territory.rs + CannibalizationAuditHandler | 3 days | Cannibalization detection + merge/hub task spawning |
| 3 | SiteHealthSnapshotHandler + seo-summary-synthesis skill | 1 day | One-command full diagnostic report |
| 4 | Frontend: new task types in UI, display audit results | 1 day | User can trigger workflows from UI |

**Total: ~7 days** for full implementation.

---

## Verification: Can We Reproduce the daystoexpiry Summary?

Running site_health_snapshot on daystoexpiry.com should produce a report containing:

| Section from Reference | Produced By | Status |
|---|---|---|
| "The Two Problems" table | seo_summary_synthesize agentic step | NEW |
| "Fix Clicks" -- shorten titles | title_meta_analysis deterministic step | NEW |
| "Fix Clicks" -- add FAQ schema | snippet_analysis deterministic step | NEW |
| "Fix Clicks" -- target featured snippets | snippet_analysis deterministic step | NEW |
| "Fix Clicks" -- write meta descriptions | title_meta_analysis deterministic step | NEW |
| "Fix Impressions" -- merge cannibal pages | cannibalization_detect deterministic + can_strategy_recommend agentic | NEW |
| "Fix Impressions" -- build 4 hub pages | hub_gap_detect deterministic + agentic | NEW |
| "Fix Impressions" -- programmatic calculators | territory_analysis deterministic + agentic | NEW |
| "Fix Impressions" -- open new territories | territory_analysis deterministic + agentic | NEW |
| "The Merge List" | can_strategy_recommend agentic step | NEW |
| "30-Day Sprint" | seo_summary_synthesize agentic step | NEW |
| "Key Numbers to Track" | seo_summary_synthesize agentic step | NEW |

**Verdict:** ~80% of the reference summary can be produced deterministically. The 20% that requires agentic steps is: merge strategy (which page to keep), hub page topics, territory recommendations, and the synthesized narrative.

---

## What This Spec Does NOT Cover

- **Programmatic page generation** (e.g., /calculator/cash-secured-put/AAPL) -- this is a separate feature requiring template + data feed integration
- **301 redirect implementation** -- the workflow recommends redirects but does not execute them (requires CMS/platform-specific integration)
- **Real-time SERP feature tracking** -- detecting whether a page CURRENTLY owns a featured snippet requires SERP API polling, not implemented
- **Backlink-aware merge decisions** -- the cannibalization strategy uses impressions as a proxy for authority; true backlink data would improve decisions
- **GA4 / engagement data** -- the summary mentions session data; GA4 integration is a separate scope

---

## Summary

This spec turns the daystoexpiry SEO Summary from a one-off manual analysis into a **reproducible, automated workflow** that can be run on any project in the PageSeeds app. Every problem in the reference document maps to:

1. A **deterministic step** that collects structured data (titles, snippets, similarities, clusters)
2. An **agentic step** that interprets the data and produces actionable recommendations
3. **Auto-spawned fix tasks** that break the work into implementable units

The result: any site can get a comparable deep-dive analysis with one click, and the app systematically queues the fixes for execution.
