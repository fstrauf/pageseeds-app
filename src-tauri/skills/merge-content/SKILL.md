# Merge Content Skill

Used by the `merge_draft_patch` agentic step.

## Goal

Draft a `ContentMergePatch` JSON that merges the most valuable unique content from redirect pages into the keeper article. The patch must be structured so deterministic code can apply it safely.

## Input

Structured JSON containing:
- **keeper_file**: Path to the keeper MDX file.
- **keeper_content**: Full text of the keeper article.
- **redirect_inventories**: Array of `SectionInventory` objects, one per redirect page, each with:
  - `file`: Path to the redirect MDX file.
  - `headings`: Array of `{ level, text, body }` — headings and their body content.
  - `tables`: Array of `{ caption, markdown }` — markdown tables.
  - `examples`: Array of `{ caption, code, language }` — code blocks.
  - `faqs`: Array of `{ question, answer }` — FAQ items.

## Analysis Rules

1. **Identify unique value**: For each section in the redirect inventories, decide whether it adds information that is NOT already covered in the keeper.
   - If the keeper already has a section with the same heading and similar content, skip it.
   - If the redirect page has a unique table, example, or data point that the keeper lacks, include it.

2. **Choose insertion points**: For each addition, specify where it fits in the keeper's flow.
   - Use `"position": "after:<existing_heading>"` to place after a specific section.
   - Use `"position": "before:<existing_heading>"` to place before a section.
   - Use `"position": "end"` to append at the end (before any Related Articles section if present).

3. **Write transitions**: If adding sections changes the narrative flow, propose transition edits to existing paragraphs using `TransitionEdit` objects.
   - `find`: Exact existing text to locate.
   - `replace`: New text that improves flow.

4. **Preserve tone and style**: The added content should match the keeper's writing style. Do not change the keeper's voice.

5. **Be conservative**: Only add content that clearly adds value. It is better to under-merge than to duplicate or dilute the keeper.

## Output Contract

Return JSON exactly matching this structure:

```json
{
  "keeper_file": "content/001_best_stocks_csp.mdx",
  "additions": [
    {
      "heading": "Risk Management Table",
      "content": "| Risk | Mitigation |\n|------|------------|\n| Assignment | Maintain cash reserve |",
      "position": "after:Criteria",
      "source_file": "content/002_csp_strategy.mdx"
    }
  ],
  "transitions": [
    {
      "find": "We look for stable blue chip stocks with weekly options.",
      "replace": "We look for stable blue chip stocks with weekly options. The table below summarizes common risks and how to mitigate them."
    }
  ],
  "notes": [
    "The risk management table from the redirect page adds unique value not present in the keeper."
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
