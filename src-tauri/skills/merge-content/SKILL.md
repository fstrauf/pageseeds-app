# Merge Content Skill

<!-- skill-version: 3 -->

Used by:
- **Path B (CLI happy path):** session agent after `merge-context` — write full merged MDX to `keeper_file`, then `merge-submit`
- **Desktop nested path:** `merge_draft_patch` agentic step (ContentMergePatch JSON only)

## Goal

Produce a single authoritative keeper article that absorbs unique value from redirect pages (tables, FAQs, examples, unique sections) without diluting the keeper's structure or tone.

---

## Path B — CLI session agent (preferred)

The session agent receives a **MergePackage** from `pageseeds-cli merge-context` (deterministic — no nested LLM).

### Package contents

- `keep` / `redirects`: full MDX bodies (frontmatter + body), outlines, soft GSC metrics
- `keeper_file` / `keeper_path`: absolute (and relative) path to **overwrite** with the merged article
- `skill_content`: this skill body
- `constraints.min_keeper_words` (default 400)
- `requires_human_confirm`: true when keep has high traffic (clicks ≥ 50 or impressions ≥ 1000 in the GSC window)
- `instructions`: Path B steps

### Path B steps

```text
evidence shortlist / approved keep+redirects
  → merge-context (full packages for keep + sources)
  → session agent writes merged MDX to keeper_file
  → merge-submit (validate, apply redirects, rewrite links, sync)
```

1. Read **full** keep + redirect MDX in the package (not excerpts only).
2. Write a **complete** merged MDX file to `keeper_file` (overwrite keeper).
   - Preserve/improve frontmatter (`title`, `description`, `slug`, `date`, `status: published`).
   - Fold unique tables, FAQs, examples, and sections from redirect pages — do **not** invent data.
   - Match the keeper's voice; prefer under-merge over dilution.
3. Meet `constraints.min_keeper_words` and valid MDX structure.
4. If `requires_human_confirm` is true, obtain human confirmation before submit.
5. Call `pageseeds-cli merge-submit` (with `-y` / `--confirm` when required).
   - `ok:false` → fix the keeper file and resubmit (nothing applied until gates pass).
   - `ok:true` → redirects.csv written, inbound links rewritten, sources depublished, sync done.

**Do not** call nested `extract_structured` / ContentMergePatch draft for Path B.  
**Do not** `execute-task consolidate_cluster` on the CLI happy path (weak host nested draft).

### CLI

```bash
# From consolidate task (loads plan from strategy artifact)
pageseeds-cli merge-context -i <id> -p <path> -I <consolidate-task-id>

# Or explicit URLs / article ids
pageseeds-cli merge-context -i <id> -p <path> \
  -K /blog/keep-slug -R /blog/src-a,/blog/src-b

# After writing merged MDX to keeper_file
pageseeds-cli merge-submit -i <id> -p <path> \
  -I <consolidate-task-id>   # or -K + -R
  [-y/--confirm]             # required when package.requires_human_confirm
```

---

## Nested path — ContentMergePatch (desktop `consolidate_cluster` only)

Used by the `merge_draft_patch` agentic step inside the app executor. Return **JSON patch only** — do not rewrite the whole file.

### Input (nested)

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

### Analysis Rules (nested)

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

### Output Contract (nested)

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

### Nested constraints

- Do NOT rewrite the entire keeper article. Only propose targeted additions and transitions.
- Do NOT add content that is already present in the keeper.
- The `position` field MUST reference an existing heading in the keeper (case-insensitive match).
- If no good insertion point exists, use `"position": "end"`.
- `find` text in transitions MUST be exact substrings from the keeper content.
- Return ONLY the JSON object. No markdown, no prose, no explanations.
