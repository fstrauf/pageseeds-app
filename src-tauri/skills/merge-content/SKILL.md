# Merge Content Skill

Used by the `merge_draft_patch` agentic step.

## Goal

Draft a `ContentMergePatch` JSON that merges the most valuable unique content from redirect pages into the keeper article. You are given compact summaries of each redirect page — write new content that captures the unique value from those summaries, adapted to the keeper's tone and structure.

## Input

Structured JSON containing:
- **keeper_file**: Path to the keeper MDX file.
- **keeper_outline**: Array of the keeper's headings (`{ level, text, body }`). Use these to choose insertion points (`after:<heading>` or `before:<heading>`).
- **keeper_excerpt**: First ~2,500 characters of the keeper article (may be truncated with `[…excerpt truncated…]`). Use this to identify duplicate content and match tone.
- **redirect_summaries**: Array of compact summary objects, one per redirect page, each with:
  - `file`: Path to the redirect MDX file.
  - `url`: URL slug of the redirect page.
  - `title`: Page title.
  - `word_count`: Approximate word count.
  - `excerpt`: First ~500 characters of the article body.
  - `headings`: Array of heading titles (no body content).
  - `has_tables`: Boolean — true if the article contains markdown tables.
  - `has_examples`: Boolean — true if the article contains code blocks.
  - `has_faqs`: Boolean — true if the article contains FAQ sections.

## Analysis Rules

1. **Identify unique value**: For each redirect summary, decide whether it covers information that is NOT already in the keeper.
   - If the keeper already has a section with the same heading and similar content, skip it.
   - If a redirect covers a unique sub-topic, data point, or angle that the keeper lacks, propose an addition.
   - Use `has_tables`, `has_examples`, and `has_faqs` as hints for what kind of content may be worth preserving.

2. **Write original content**: You are given summaries, not full source text. Write fresh content that captures the unique value from each redirect summary. Do not try to copy exact tables or code blocks — write a natural prose summary, explanation, or adapted version that fits the keeper's style.

3. **Choose insertion points**: For each addition, specify where it fits in the keeper's flow.
   - Use `"position": "after:<existing_heading>"` to place after a specific section.
   - Use `"position": "before:<existing_heading>"` to place before a section.
   - Use `"position": "end"` to append at the end (before any Related Articles section if present).

4. **Write transitions**: If adding sections changes the narrative flow, propose transition edits to existing paragraphs using `TransitionEdit` objects.
   - `find`: Exact existing text to locate.
   - `replace`: New text that improves flow.

5. **Preserve tone and style**: The added content should match the keeper's writing style. Do not change the keeper's voice.

6. **Be conservative**: Only add content that clearly adds value. It is better to under-merge than to duplicate or dilute the keeper.

## Output Contract

Return JSON exactly matching this structure:

```json
{
  "keeper_file": "content/001_best_stocks_csp.mdx",
  "additions": [
    {
      "heading": "Risk Management Considerations",
      "content": "When selling cash-secured puts, it's important to understand the risks involved. The primary risk is assignment — if the stock price falls below the strike price, you may be obligated to purchase the shares. To mitigate this, maintain a cash reserve and choose strike prices below strong support levels.",
      "position": "after:Criteria",
      "source_file": "content/002_csp_strategy.mdx"
    }
  ],
  "transitions": [
    {
      "find": "We look for stable blue chip stocks with weekly options.",
      "replace": "We look for stable blue chip stocks with weekly options. The section below summarizes common risks and how to mitigate them."
    }
  ],
  "notes": [
    "The redirect page added a unique risk management angle not present in the keeper."
  ]
}
```

## Constraints

- Do NOT rewrite the entire keeper article. Only propose targeted additions and transitions.
- Do NOT add content that is already present in the keeper.
- The `position` field MUST reference an existing heading in the keeper (case-insensitive match).
- If no good insertion point exists, use `"position": "end"`.
- `find` text in transitions MUST be exact substrings from the keeper content.
- Return ONLY the JSON object. No markdown, no prose, no explanations.
