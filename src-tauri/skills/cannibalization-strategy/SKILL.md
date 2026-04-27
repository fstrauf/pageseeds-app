# Cannibalization Strategy Skill

Used by the `can_analyze` agentic step.

## Input

Structured JSON containing:
- **site_summary**: `total_pages`, `total_impressions`, `period_days`
- **clusters**: Array of cluster objects, each with:
  - `cluster_id`: Machine-readable cluster identifier
  - `theme`: Common theme / target keyword
  - `candidate_intent`: Inferred search intent for the cluster
  - `total_impressions`, `total_clicks`, `avg_position`: Aggregated GSC metrics
  - `shared_query_count`: Number of distinct target keywords in cluster (proxy for query overlap)
  - `hub_exists`: Whether a hub/guide page already exists in this cluster
  - `pages`: Full per-page metadata including `url`, `title`, `h1`, `target_keyword`, `impressions`, `clicks`, `ctr`, `avg_position`, `word_count`, `incoming_internal_links`, `outgoing_internal_links`, `published_date`, `first_200_words`
  - `top_shared_queries`: Most common queries shared across pages
- **hub_gaps**: Clusters with 3+ articles that lack a broad parent hub page
- **territory_analysis**: `saturated_themes` (>5 articles) and `open_territories` (0-1 articles with demand evidence)

## Analysis Rules

### 1. Identify True Cannibalization
- Analyze similarity clusters and distinguish **true cannibalization** from mere topical similarity.
- True cannibalization occurs when two or more articles target the **same search intent** for the **same keyword** and compete against each other in SERPs.
- Topical overlap alone (e.g., two articles mentioning the same broad topic) is not sufficient.
- Use `shared_query_count`, similarity evidence, and per-page metrics to decide.

### 2. Merge Recommendations
For each cannibalized cluster, recommend which article to **KEEP** and which to **redirect**:

- **Keeper selection criteria** (evaluate all, then decide):
  - **Impressions**: Higher impressions = stronger authority signal (primary proxy when backlink data is unavailable).
  - **Internal links**: Higher `incoming_internal_links` indicates stronger site integration.
  - **URL quality**: Shorter, cleaner, more keyword-aligned URLs are preferred.
  - **Content depth**: Higher `word_count` and better-structured content wins.
  - **Publish date**: More recent content is preferred if depth and authority are comparable.
  - **Position**: Lower `avg_position` (closer to 1) is better.
- The keeper should be the **strongest overall article** in the cluster.
- Redirect targets should be merged into the keeper **before** applying 301s: preserve unique examples, data points, or angles.
- Set `confidence` to `high`, `medium`, or `low` based on evidence strength.
- Low-confidence recommendations should never auto-apply.

### 3. Hub / Pillar Page Identification
- For any cluster with **3+ articles**, evaluate whether a dedicated hub/pillar page is missing.
- Check `hub_exists` and `hub_gaps` to avoid recommending duplicate hubs.
- Hub pages should target a **broader keyword** than the individual cluster articles.
- The hub should logically link to all cluster articles.

### 4. Territory Analysis
- Identify **saturated themes**: > 5 articles on the same narrow topic with diminishing returns.
- Identify **open territories**: themes with 0–1 existing articles but **related demand** (implied by adjacent keywords, search trends, or gaps in the content map).
- Recommend new topical territories based on real search demand.

### 5. Prioritization
- Prioritize clusters with the **highest total impressions** first.
- Use impressions as the primary authority proxy when backlink data is unavailable.

## Output Contract

Return JSON exactly matching this structure:

```json
{
  "merge_recommendations": [
    {
      "cluster_id": "cash_secured_puts_best_stocks",
      "confidence": "high",
      "keep_url": "/blog/best-stocks-csp",
      "redirect_urls": ["/blog/cash-secured-puts-strategy-explained", "/blog/cash-secured-puts-playbook"],
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
      "topic": "cash-secured-puts",
      "suggested_url": "/hub/cash-secured-puts",
      "suggested_title": "Cash-Secured Puts: Complete Guide",
      "intent": "broad pillar",
      "source_pages": [1, 2, 4],
      "spoke_pages": [1, 2, 4, 5],
      "outline": ["What CSPs are", "Best stocks", "Strike selection", "Risks", "Calculators"]
    }
  ],
  "calculator_recommendations": [],
  "territory_recommendations": [
    {
      "theme": "broker-reviews",
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

## Constraints

- Be specific: name **exact URLs** and **article titles**.
- Use **impressions as primary authority proxy** when backlink data is unavailable.
- The keeper must have the **cleanest URL**, **highest impressions**, and **best content depth**.
- Hub pages must target **broader keywords** than the cluster articles they link to.
- New territories must have **0–1 existing articles** and evidence of **real search demand**.
- Prioritize clusters with the **highest total impressions** first.
- Every merge recommendation must name a keeper URL and at least one redirect URL.
- Every keeper and redirect URL must exist in the provided cluster pages.
- Every redirect URL must be different from the keeper.
