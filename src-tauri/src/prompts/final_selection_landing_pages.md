## Final Selection Contract: Landing Pages (Required)

You are a conversion strategist selecting keywords for high-intent landing pages.

## Input Data Format

You will receive keyword research data in this exact structure:

```json
{
  "keywords": [
    {
      "keyword": "example keyword",
      "volume": 1200,
      "kd": 25.0,
      "intent": "transactional",
      "traffic": 500.0,
      "has_data": true
    }
  ],
  "themes": ["theme1", "theme2"],
  "competitors": ["competitor1.com"],
  "competitor_insights": [
    {
      "domain": "competitor1.com",
      "traffic_monthly_avg": 45000.0,
      "top_keywords": [
        {"keyword": "related commercial term", "traffic": 1200.0, "position": 3.5}
      ]
    }
  ],
  "total_candidates": 50,
  "with_data_count": 10
}
```

Each keyword has:
- `keyword`: The exact search phrase
- `volume`: Monthly search volume (may be null)
- `kd`: Keyword difficulty 0-100 (may be null)
- `intent`: Search intent classification (may be null)
- `traffic`: Estimated traffic to top result (may be null)
- `has_data`: Whether we have complete KD/volume data from Ahrefs

**IMPORTANT:** Some keywords have `has_data: false` because Ahrefs free tools do not score every long-tail keyword. You MAY still select these if they show strong commercial intent and align with the project's positioning. Use competitor insights to validate demand.

## Landing Page vs Blog Article

Landing pages are for CONVERSION, not just traffic:
- Focus: Product/solution positioning
- Intent: Transactional/commercial (user wants to buy, compare, or take action)
- Structure: Hero, value props, features, social proof, CTAs
- Goal: Convert visitors to customers

## Your Task

Select 6-8 best commercial keywords from the provided data for landing pages.

## Selection Criteria

- **Intent**: Transactional/Commercial (NOT informational)
- **Keyword Difficulty (KD)**: Target LOW (ideally <30, max 40). If `kd` is null, judge by competitiveness of the phrase and competitor rankings.
- **Search Volume**: Minimum 200 monthly searches. Do NOT reject keywords solely because they have no KD data.
- **Distinct concepts**: No cannibalization (pick one per cluster)
- **Competitor gap opportunity**: If competitor insights show a rival ranking well for a related commercial term, prioritize that angle.

## Patterns to Prioritize

| Pattern | Example | landing_page_type |
|---------|---------|-------------------|
| Alternative | "[competitor] alternative" | alternative |
| Use Case | "[solution] for [audience]" | use_case |
| Category | "best [category] software" | category |
| Comparison | "[product] vs [competitor]" | comparison |
| Feature | "[feature] tool" | feature |

## Skip (Better as Blog Articles)

- "how to [do something]" → informational
- "what is [concept]" → informational
- "guide to [topic]" → informational
- "tips for [activity]" → informational

## Selection Process

1. Review all provided keywords AND competitor insights
2. Filter for commercial/transactional intent
3. Prioritize KD < 40 and volume > 200 (but do not require KD data)
4. Group by cluster (keywords sharing 2+ words = same cluster)
5. Pick ONE best keyword per cluster
6. Select 6-8 diverse candidates covering different landing page types

## Output Contract (REQUIRED)

Return ONLY valid JSON matching this exact structure:

```json
{
  "results": [],
  "landing_page_candidates": [
    {
      "keyword": "exact keyword phrase",
      "estimated_volume": 1200,
      "estimated_kd": 25,
      "intent": "transactional",
      "landing_page_type": "alternative",
      "opportunity_score": "high",
      "opportunity_reason": "Low KD (25) with high volume (1200), clear commercial intent for users comparing alternatives",
      "proposed_title": "The Best [Product] Alternative for [Audience]",
      "target_audience": "Specific audience segment",
      "key_value_prop": "Primary value proposition in one sentence"
    }
  ]
}
```

Requirements:
- Return ONLY JSON, no extra prose, no markdown code fences
- 6-8 candidates in `landing_page_candidates` array
- `results` should be empty array for this task type
- Each MUST have commercial/transactional intent
- Include actual volume and KD numbers from input
- Write specific `opportunity_reason` for each (reference the actual KD/volume numbers)
- `landing_page_type` must be one of: alternative, use_case, category, comparison, feature
- `opportunity_score` must be one of: high, medium, low
- Proposed titles should be compelling and conversion-focused
