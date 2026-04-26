# CTR Fix Apply Skill

Used by `fix_ctr_article` agentic steps.

## Objective

Read the target MDX file and return a structured patch plan with replacement values for CTR fixes. **Do not edit the file directly** — the app applies changes deterministically. Your job is to produce the correct prose; the app handles frontmatter, body structure, and file writing.

## Input

A JSON artifact (`ctr_recommendations`) containing a `recommendations` array with one article. Each item has:
- `article_id` — numeric ID from articles.json
- `url_slug` — URL slug of the article
- `file` — **relative path to the MDX file** (e.g. `content/042_article_slug.mdx`)
- `target_keyword` — the keyword this article targets
- `fixes` — array of fixes to apply, each with:
  - `type`: `title_rewrite`, `meta_description`, `faq_schema`, or `snippet_bait`
  - `current` — the current value (for title/meta)
  - `recommended` — the new value to write
  - `reason` — why the change is needed

## Rules

1. **Read the file** at the given `file` path to verify current content.
2. **Produce replacement values only**. Do not output raw MDX, frontmatter, or `---` delimiters.
3. **Apply only the fixes listed**. Do not change unrelated fields.
4. **Return structured JSON** matching the Output Contract below.

## Per-Fix-Type Instructions

### `title_rewrite`
- Return the new title text in `changes.title`.
- Hard limit: 55 characters max.
- If the current title is already good, return `null` for `title`.

### `meta_description`
- Return the new meta description text in `changes.description`.
- Aim for 150 characters, hard max 155. Minimum accepted is 130.
- If the current description is already good, return `null` for `description`.

### `faq_schema`
- Return 3–5 question/answer pairs in `changes.faq_questions`.
- Each item: `{ "question": "...?", "answer": "..." }`.
- If no FAQ changes are needed, return `null` or omit the field.

### `snippet_bait`
- Return the new first paragraph text in `changes.first_paragraph`.
- Hard limits: 40–60 words (min 40, max 60).
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
- If the file does not exist, return an empty `changes` object and set `error: "file not found"`.
