# CTR Optimization Skill

<!-- skill-version: 1 -->

Used by the `ctr_analyze` agentic step.

## Input

Structured JSON containing per-article data:
- `article_id`, `url_slug`, `title`, `meta_description`, `first_paragraph`, `h1`
- `target_keyword`, `gsc_metrics` (impressions, clicks, ctr, position)
- Computed `clicks_lost` score and `target_ctr` (position-aware expected CTR)
- `top_queries` — array of top GSC queries for this page, each with `query`, `impressions`, `clicks`, `ctr`, `avg_position`, `intent`

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
- **CRITICAL — PRESERVATION RULE**: If the article already has frontmatter `faq:` YAML with valid question/answer pairs, do NOT recommend `faq_schema` fixes. Existing FAQ is preserved by default.
- Only recommend `faq_schema` fixes when:
  - No FAQ content exists at all (no frontmatter `faq:`, no inline JSON-LD, no visible FAQ section)
  - OR the existing FAQ is clearly empty/malformed (zero valid Q/A pairs)
  - OR the existing FAQ is obviously thin generic filler with no article-specific detail
- When generating FAQ is allowed, create 3–5 questions grounded in the article content with specific facts, prices, ranges, or named entities. Avoid generic restatements.
- Do not judge existing rich FAQ as "thin" just because you think you could write different questions.

### 4. Featured Snippet Readiness
- First paragraph should contain a 40–60 word direct answer.
- The paragraph must be a single contiguous text block (no blank lines) and must contain the `target_keyword` OR a question mark (`?`).
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
      "file": "content/042_best_stocks_csp.mdx",
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

## Field Rules

- `article_id` — echo from input (required)
- `url_slug` — echo from input (required)
- `file` — echo the `file` value from the input context **exactly** (required). Do not guess, construct, or modify the path.
- `target_keyword` — echo from input (required)
- `priority` — `high`, `medium`, or `low` based on clicks_lost magnitude
- `expected_ctr_improvement` — estimated range (e.g. `0.3-0.8%`)

## Constraints

- Limit to **top 20 pages** by `clicks_lost`.
- **Title rewrites**: keep under 55 characters (hard limit: 55), front-load keyword, remove duplication.
- **Meta descriptions**: 150–155 characters (aim for 150, hard max 155), pattern `[Keyword] + [benefit] + [soft CTA]`. Minimum accepted is 130.
- **FAQ**: 3–5 questions that reflect real search queries. When `top_queries` is provided, prefer high-impression question/comparison queries from that list. Must be JSON-LD FAQPage schema, not just markdown headings.
- **Snippet bait**: 40–60 word direct answer. Match article type to the query intent:
  - `question` intent → paragraph direct answer
  - `comparison` intent → paragraph comparison or table
  - `best_list` intent → numbered or bulleted list
  - `calculator_tool` / `generic` → paragraph
  When `top_queries` is available, target the highest-impression query with position 2–10.
- Be specific: name exact current titles and recommended replacements.
