# CTR Optimization Skill

Used by the `ctr_analyze` agentic step.

## Input

Structured JSON containing per-article data:
- `article_id`, `url_slug`, `title`, `meta_description`, `first_paragraph`, `h1`
- `target_keyword`, `gsc_metrics` (impressions, clicks, ctr, position)
- Computed `clicks_lost` score

## Analysis Rules

### 1. Title Analysis
For each article, evaluate the title for CTR issues:
- **Length**: Must be under 55–60 characters. Longer titles risk truncation in SERPs.
- **Brand duplication**: Flag if the brand or site name appears more than once.
- **Keyword position**: The target keyword should appear near the front (first 2–3 words).
- **Clarity**: Titles must clearly signal the article's promise; avoid vague or generic phrasing.
- **Truncation risk**: Titles ending with ellipses or cut-off benefit statements reduce CTR.

### 2. Meta Description Analysis
- **Presence**: Flag if missing entirely.
- **Length**: Ideal range is 140–155 characters.
- **Pattern compliance**: Should follow `[Keyword] + [benefit] + [soft CTA]`.
- **Uniqueness**: Duplicate or near-duplicate descriptions across articles reduce CTR.

### 3. FAQ Schema
- Check for FAQ schema presence.
- If present, verify 3–5 relevant questions that match actual search queries.
- If missing or thin, recommend adding 3–5 questions.

### 4. Featured Snippet Readiness
- First paragraph should contain a 40–60 word direct answer.
- Match the answer format to the article type:
  - **X vs Y** → paragraph comparison
  - **best X** → bulleted or numbered list
  - **comparison / multi-item** → table
- If the first paragraph is too short or off-format, recommend a rewrite.

### 5. Prioritization
- Rank all recommendations by `clicks_lost` in descending order (highest first).
- Limit output to the **top 20 pages** by `clicks_lost`.

### 6. Actionability
- Every recommendation must include a **specific, actionable fix**.
- Never output generic advice (e.g., "improve your title").
- Name the exact current title/description and the exact recommended replacement.

## Output Contract

Return JSON exactly matching this structure:

```json
{
  "recommendations": [
    {
      "article_id": 42,
      "url_slug": "best-stocks-csp",
      "priority": "high",
      "expected_ctr_improvement": "0.3-0.8%",
      "fixes": [
        {
          "type": "title_rewrite",
          "current": "Current Title | Brand | Brand -- Tagline",
          "recommended": "Optimized Title | Brand",
          "reason": "Title is 92 chars, brand duplicated, truncation risk"
        },
        {
          "type": "meta_description",
          "current": "",
          "recommended": "Learn the best stocks for cash-secured puts in 2026. Boost your income with our proven CSP strategy guide.",
          "reason": "Meta description missing - adding keyword + benefit + CTA"
        },
        {
          "type": "faq_schema",
          "recommended": ["What are cash-secured puts?", "How much capital do I need for CSPs?"],
          "reason": "No FAQ section found - adding 3-5 questions to expand SERP presence"
        },
        {
          "type": "snippet_bait",
          "recommended": "Cash-secured puts are an options strategy where you sell put contracts while holding enough cash to buy the underlying stock if assigned. This generates premium income while potentially acquiring stocks at a discount to current market price.",
          "reason": "First paragraph is only 28 words. Adding 40-60 word direct answer targets paragraph snippet."
        }
      ]
    }
  ]
}
```

## Constraints

- Limit to **top 20 pages** by `clicks_lost`.
- **Title rewrites**: keep under 55 characters, front-load keyword, remove duplication.
- **Meta descriptions**: 140–155 characters, pattern `[Keyword] + [benefit] + [soft CTA]`.
- **FAQ**: 3–5 questions that reflect real search queries.
- **Snippet bait**: match article type (`X vs Y` → paragraph, `best X` → list, comparison → table).
- Be specific: name exact current titles and recommended replacements.
