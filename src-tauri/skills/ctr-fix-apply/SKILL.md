# CTR Fix Apply Skill

<!-- skill-version: 3 -->

Used by `fix_ctr_article` agentic steps.

## Objective

Return a structured patch plan with replacement values for CTR fixes. **Do not edit the file directly** — the app applies changes deterministically. Your job is to produce the correct prose; the app handles frontmatter, body structure, file writing, and file existence checks.

## Input

A JSON artifact (`ctr_recommendations`) containing a single article recommendation object:
- `article_id` — numeric ID from articles.json
- `url_slug` — URL slug of the article
- `file` — relative path to the content file (echo this value back unchanged)
- `target_keyword` — the keyword this article targets
- `fixes` — array of fixes to apply, each with:
  - `type`: `title_rewrite`, `meta_description`, `faq_schema`, or `snippet_bait`
  - `current` — the current value (for title/meta)
  - `recommended` — the new value to write
  - `reason` — why the change is needed

## Rules

1. **Produce replacement values only**. Do not output raw MDX, frontmatter, or `---` delimiters.
2. **Apply only the fixes listed**. Do not change unrelated fields.
3. **Return structured JSON** matching the Output Contract below.

## Per-Fix-Type Instructions

### `title_rewrite`
- Return the new title text in `changes.title`.
- Prefer keeping the recommended strategic phrasing (year freshness, head-term inclusion, intent framing) from `recommended`.
- Do **not** "normalize away" strategic years or head-query phrasing solely to hit 55 characters if that would drop the head query or remove a warranted current-year refresh.
- Soft length target: ~55 characters. Hard ceiling for apply is still ~55, but if `recommended` is strategically correct and under ~60, use it as given rather than shortening away recovery elements.
- If the current title is already good and no strategic recovery is needed, return `null` for `title`.

### `meta_description`
- Return the new meta description text in `changes.description`.
- **Hard limits: 120–155 characters.** Minimum 120, maximum 155.
- **Aim for 145–150 characters.** Undershooting 120 is a verification failure.
- Count characters carefully. Do not return descriptions under 120 chars.
- If the current description is already good, return `null` for `description`.

### `faq_schema`
- Return 3–5 question/answer pairs in `changes.faq_questions`.
- Each item: `{ "question": "...?", "answer": "..." }`.
- **CRITICAL — PRESERVATION RULE**: If the input context shows the article already has frontmatter `faq:` YAML with valid Q/A pairs, return `null` for `faq_questions`. Do not overwrite existing FAQ even if `faq_schema` is listed in the fixes.
- Only generate FAQ when the article has no structured FAQ source at all, or when explicitly told the existing FAQ is empty/malformed.

### `snippet_bait`
- Return the new first paragraph text in `changes.first_paragraph`.
- Hard limits: 40–60 words (minimum 40, maximum 60).
- Must contain the `target_keyword` OR a question mark (`?`).
- Must be a single contiguous block of text (no blank lines inside it).

## Output Contract

Return a JSON object exactly matching this structure:

```json
{
  "article_id": 42,
  "file": "content/042_article_slug.mdx",
  "changes": {
    "title": "New Title Here | Brand",
    "description": "New meta description here...",
    "first_paragraph": "New 45-word first paragraph here...",
    "faq_questions": [
      {"question": "What is X?", "answer": "X is..."},
      {"question": "How does Y work?", "answer": "Y works by..."}
    ]
  }
}
```

**Field rules:**
- `article_id` and `file` — echo from input
- `changes.title` — new title text, or `null` to skip
- `changes.description` — new meta description text, or `null` to skip
- `changes.first_paragraph` — new first paragraph text, or `null` to skip
- `changes.faq_questions` — array of `{question, answer}` objects, or `null` to skip
- Omit a field entirely if no change is needed

## Constraints

- Do not include raw MDX, frontmatter delimiters (`---`), or YAML formatting in output values.
- Do not include markdown `#` headings in `title` or `description` values.
- Do not write files — only return the structured JSON.
- Do not verify file existence or read from disk — use the `current` values provided in the input.
