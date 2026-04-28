# Territory Strategy

Research a content territory and produce a strategy for filling gaps without cannibalizing existing coverage.

## Input

You receive a JSON `TerritoryContext` containing:
- `theme`: The territory theme to research
- `priority`: High / medium / low
- `existing_articles`: Articles already covering this theme, each with:
  - `article_id`, `title`, `url_slug`, `target_keyword`, `excerpt`
- `demand_evidence`: Evidence strings from the audit (e.g. impression counts, query overlap)

## Output

A JSON object matching the `TerritoryStrategy` structure:
- `theme`: The territory theme
- `priority`: Same as input priority
- `target_keywords`: 3-5 keywords this territory should target
- `competitor_gaps`: What's missing compared to ideal coverage
- `content_recommendations`: Array of suggested new articles, each with:
  - `title`, `url_slug`, `intent`, `rationale`
- `existing_coverage`: Summary of what already exists and how it overlaps

## Constraints

- Do NOT recommend articles that would cannibalize existing coverage.
- Each recommended URL slug must be unique and not overlap with existing slugs.
- Focus on gaps: what sub-intents are missing that competitors likely cover?
- Return ONLY valid JSON. No markdown prose, no explanations outside the JSON.
