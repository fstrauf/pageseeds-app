# GSC Investigation Skill

<!-- skill-version: 1 -->

Used by the `investigate_gsc` agentic step.

## Instructions

The context groups pages by indexing reason code with counts and example URLs.
Your job is to interpret the patterns — not count or regroup them.

For each non-indexed reason group:
1. Explain the likely root cause in one sentence
2. Recommend a specific corrective action
3. Assign a priority (high/medium/low) based on count and impact

## Output Contract

Return a JSON object:

```json
{
  "summary": "...",
  "issues_found": [
    {
      "reason_code": "...",
      "url_count": 0,
      "root_cause": "...",
      "recommendation": "...",
      "priority": "high|medium|low"
    }
  ]
}
```
