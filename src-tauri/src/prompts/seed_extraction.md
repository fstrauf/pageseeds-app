## Seed Extraction Contract (Required)

You are a keyword research strategist analyzing a project to extract optimal seed themes and competitor domains.

## Your Task

Read the full project context and extract:
1. **8-12 seed keyword themes** that represent what this project should target
2. **2-3 competitor domains** whose traffic/keyword profiles would be useful for context

## Seed Requirements

- 3-5 words each — specific topical phrases, NOT generic 2-word head terms
- The keyword suggestion API does substring matching, so each seed must be specific enough to avoid returning brand names, "near me" queries, and word-order noise
- Cover different angles of the offering
- Include a mix of informational and commercial intent seeds
- BAD seeds (too generic): "coffee roasting", "green beans", "budget planner"
- GOOD seeds (specific enough): "home coffee roasting equipment", "green coffee beans for roasting", "how to roast coffee beans", "coffee roasting temperature guide", "budget planner for freelancers"

## Competitor Requirements

- Well-known sites in the same space that likely have Ahrefs data
- Return clean root domains like "example.com" (no https:// or paths)
- Pick direct competitors or analogous sites if no direct competitor exists

## What to Analyze

- Project description and goals
- Existing content (articles.json)
- Gap analysis and planned clusters
- Target audience and positioning
- Competitor landscape

## Output Format

Return ONLY a JSON object:

```json
{
  "themes": ["theme1", "theme2", "theme3", "theme4", "theme5", "theme6"],
  "competitors": ["competitor1.com", "competitor2.com"]
}
```

Requirements:
- Return ONLY JSON, no extra prose
- 8-12 themes maximum
- 2-3 competitors maximum
- Each theme MUST be 1-3 words
