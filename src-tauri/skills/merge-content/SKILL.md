# Merge Content Skill

<!-- skill-version: 2 -->

Used by the `merge_draft_patch` agentic step.

## Goal

Draft a `ContentMergePatch` JSON that merges the most valuable unique content from redirect pages into the keeper article. You are given the actual body content of each redirect page's unique sections — preserve that real content (tables, FAQs, examples) in the keeper, adapted to the keeper's tone and structure.

## Input

Structured JSON containing:
- **keeper_file**: Path to the keeper MDX file.
- **keeper_outline**: Array of the keeper's headings (`{ level, text }`). Use these to choose insertion points (`after:<heading>` or `before:<heading>`).
- **keeper_excerpt**: First ~1,500 characters of the keeper article (may be truncated with `[…excerpt truncated…]`). Use this to identify duplicate content and match tone.
- **batch_index** / **batch_count**: Large clusters are processed in sequential batches of redirect pages. Draft a patch for THIS batch only; other batches are handled in separate rounds against the same keeper.
- **redirect_pages**: Array of page objects, one per redirect page in this batch, each with:
  - `file`: Path to the redirect MDX file.
  - `url`: URL slug of the redirect page.
  - `title`: Page title.
  - `word_count`: Approximate word count.
  - `sections`: Array of heading sections (`{ level, text, body, covered_by_keeper }`). Sections whose heading already exists in the keeper have `covered_by_keeper: true` and an empty `body`; unique sections carry their full body text.
  - `tables`: Array of markdown tables (`{ markdown }`) extracted from the page.
  - `examples`: Array of code blocks (`{ language, code }`) extracted from the page.
  - `faqs`: Array of FAQ pairs (`{ question, answer }`) extracted from the page.

## Analysis Rules

1. **Identify unique value**: For each redirect page, decide what it covers that is NOT already in the keeper.
   - Sections with `covered_by_keeper: true` are usually redundant — skip them unless the keeper's version is clearly weaker.
   - If a redirect covers a unique sub-topic, data point, or angle that the keeper lacks, propose an addition.

2. **Preserve real content**: You are given the actual tables, code examples, and FAQ answers from the redirect pages. Carry them over faithfully — do NOT paraphrase a table into prose, invent replacement data, or regenerate code from memory. Edit only what is needed to fit the keeper's style and flow.

3. **Choose insertion points**: For each addition, specify where it fits in the keeper's flow.
   - Use `"position": "after:<existing_heading>"` to place after a specific section.
   - Use `"position": "before:<existing_heading>"` to place before a section.
   - Use `"position": "end"` to append at the end (before any Related Articles section if present).

4. **Write transitions**: If adding sections changes the narrative flow, propose transition edits to existing paragraphs using `TransitionEdit` objects.
   - `find`: Exact existing text to locate. Only the FIRST occurrence in the keeper is replaced, so make `find` long enough to anchor the intended spot.
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
