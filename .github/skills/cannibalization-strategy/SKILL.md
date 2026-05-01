# Cannibalization Strategy Skill

Used by the `can_analyze` agentic step.

## Input

Structured JSON containing:
- **Articles**: `id`, `url_slug`, `title`, `h1`, `target_keyword`, `first_200_words`, `gsc_metrics` (impressions, clicks, ctr, position)
- **Similarity pairs**: articles with Jaccard similarity > 0.3
- **Keyword groups**: articles sharing identical `target_keywords`

## Analysis Rules

### 1. Identify True Cannibalization
- Analyze similarity clusters and distinguish **true cannibalization** from mere topical similarity.
- True cannibalization occurs when two or more articles target the **same search intent** for the **same keyword** and compete against each other in SERPs.
- Topical overlap alone (e.g., two articles mentioning the same broad topic) is not sufficient.

### 2. Merge Recommendations
For each cannibalized cluster, recommend which article to **KEEP** and which to **redirect**:

- **Keeper selection criteria** (evaluate all, then decide):
  - **Impressions**: Higher impressions = stronger authority signal (primary proxy when backlink data is unavailable).
  - **URL quality**: Shorter, cleaner, more keyword-aligned URLs are preferred.
  - **Content depth**: Longer, more thorough, better-structured content wins.
  - **Publish date**: More recent content is preferred if depth and authority are comparable.
- The keeper should be the **strongest overall article** in the cluster.
- Redirect targets should be merged into the keeper **before** applying 301s: preserve unique examples, data points, or angles.

### 3. Hub / Pillar Page Identification
- For any cluster with **3+ articles**, evaluate whether a dedicated hub/pillar page is missing.
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
      "cluster_theme": "cash-secured-puts",
      "keep_url": "/blog/best-stocks-csp",
      "redirect_urls": ["/blog/cash-secured-puts-strategy-explained", "/blog/cash-secured-puts-playbook"],
      "merge_instructions": "Merge unique content from redirect targets into the keeper before applying 301 redirects. Preserve any unique examples or data points.",
      "reason": "best-stocks-csp has 45K impressions vs 1.2K for the others. Cleaner URL. More recent content."
    }
  ],
  "hub_recommendations": [
    {
      "topic": "cash-secured-puts",
      "suggested_url": "/hub/cash-secured-puts",
      "suggested_title": "Cash-Secured Puts: The Complete Guide",
      "spoke_pages": [42, 43, 44, 45],
      "outline_suggestion": "Introduction to CSPs -> How they work -> Best practices -> Risk management -> Comparison to other strategies -> FAQ"
    }
  ],
  "territory_recommendations": [
    {
      "theme": "broker-reviews",
      "opportunity": "Zero articles cover broker-specific guides. High search demand for 'best broker for options trading'.",
      "suggested_articles": ["Best Brokers for Options Trading in 2026", "Interactive Brokers vs Tastytrade for CSPs"],
      "priority": "high"
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
