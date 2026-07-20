# Hub Outline Architect

<!-- skill-version: 1 -->

Design a structured information architecture for a pillar / hub page based on a cluster of spoke articles.

## Input

You receive a JSON `HubBrief` containing:
- `topic`: The broad topic this hub covers
- `suggested_url`: Target URL slug for the hub
- `suggested_title`: Proposed H1 / title
- `intent`: Primary search intent
- `target_keyword`: Primary keyword to target
- `spokes`: Array of spoke articles, each with:
  - `article_id`, `title`, `url_slug`, `file`, `impressions`, `excerpt`

## Output

A JSON object matching the `HubOutline` structure:
- `title`: Final hub title (may refine the suggested title)
- `slug`: Final URL slug
- `sections`: Array of outline sections, each with:
  - `heading`: Section H2 heading
  - `intent`: What the reader wants in this section
  - `spoke_ids`: Which spoke article IDs this section covers (can be empty for intro/conclusion)
  - `notes`: Guidance for the writer on what to include/exclude
- `link_strategy`: Brief description of how spokes should be linked
- `excluded_sub_intents`: Sub-topics that should remain as separate spokes and NOT be merged into the hub

## Constraints

- Group related spokes into logical sections. Do not create one section per spoke unless they are truly independent.
- The hub should cover the BROAD intent. Spokes cover specific sub-intents.
- List any sub-intents that should stay as spokes in `excluded_sub_intents`.
- Each spoke must appear in at least one section's `spoke_ids`.
- Return ONLY valid JSON. No markdown prose, no explanations outside the JSON.
