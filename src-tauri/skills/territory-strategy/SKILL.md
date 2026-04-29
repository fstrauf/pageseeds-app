# Territory Strategy

Research a content territory and produce a strategy for filling gaps without cannibalizing existing coverage.

## Input

You receive a JSON `TerritoryContext` containing:
- `theme`: The territory theme to research
- `priority`: High / medium / low
- `existing_articles`: Articles already covering this theme, each with:
  - `article_id`, `title`, `url_slug`, `target_keyword`, `excerpt`
- `demand_evidence`: Evidence strings from the audit (e.g. impression counts, query overlap)

## Output Contract

Return JSON exactly matching this structure:

```json
{
  "theme": "coffee-health",
  "priority": "high",
  "target_keywords": [
    "is coffee healthy",
    "coffee health benefits",
    "caffeine and heart health",
    "decaf health effects",
    "coffee acid reflux"
  ],
  "competitor_gaps": [
    "No dedicated coverage of cardiovascular or metabolic health benefits despite high search volume",
    "Missing evidence-based breakdown of caffeine vs decaf health impacts",
    "No content addressing acid reflux, GERD, or stomach sensitivity from coffee"
  ],
  "content_recommendations": [
    {
      "title": "Is Coffee Good for Your Heart? What the Research Says",
      "url_slug": "is-coffee-good-for-your-heart",
      "intent": "informational",
      "rationale": "High search volume for cardiovascular health + caffeine; no existing article covers this specifically"
    },
    {
      "title": "Caffeine vs Decaf: Health Impacts Compared",
      "url_slug": "caffeine-vs-decaf-health-impacts",
      "intent": "informational",
      "rationale": "Decaf process article has impressions but zero clicks, indicating unmet demand for health-focused comparison"
    }
  ],
  "existing_coverage": [
    {
      "article_id": 233,
      "title": "How Much Caffeine Is in Coffee?",
      "url_slug": "how-much-caffeine-in-coffee",
      "overlap": "Covers caffeine content but not health effects or benefits"
    }
  ]
}
```

## Field Requirements

- `theme` (required string): The territory theme, lowercase with hyphens
- `priority` (required string): `"high"`, `"medium"`, or `"low"` — same as input priority
- `target_keywords` (array of 3–5 strings): Specific keywords this territory should target
- `competitor_gaps` (array of strings): What's missing compared to ideal coverage; be specific about search demand
- `content_recommendations` (array of objects): Each MUST have all four fields:
  - `title` (string): Descriptive, search-friendly article title
  - `url_slug` (string): Short slug with hyphens, no leading/trailing slashes
  - `intent` (string): `"informational"`, `"commercial"`, or `"transactional"`
  - `rationale` (string): Why this article fills a gap and what demand evidence supports it
- `existing_coverage` (array of objects): Summarize articles already covering this theme; each MUST have all four fields:
  - `article_id` (number): The article ID from the input context
  - `title` (string): The article title
  - `url_slug` (string): The article URL slug
  - `overlap` (string): Brief description of how this article overlaps with the territory

## Constraints

- Do NOT recommend articles that would cannibalize existing coverage.
- Each recommended URL slug must be unique and not overlap with existing slugs.
- Focus on gaps: what sub-intents are missing that competitors likely cover?
- Return ONLY valid JSON. No markdown prose, no explanations outside the JSON.
