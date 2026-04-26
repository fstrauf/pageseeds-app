# CTR Fix Apply Skill

Used by `fix_ctr_article` agentic steps.

## Objective

Apply specific CTR optimization recommendations directly to MDX files in the repository. Do not describe changes — make them. Every recommendation must result in an edited file on disk.

## Input

A JSON artifact (`ctr_recommendations`) containing a `recommendations` array. Each item has:
- `article_id` — numeric ID from articles.json
- `url_slug` — URL slug of the article
- `file` — **relative path to the MDX file** (e.g. `content/042_article_slug.mdx`)
- `fixes` — array of fixes to apply, each with:
  - `type`: `title_rewrite`, `meta_description`, `faq_schema`, or `snippet_bait`
  - `current` — the current value (for title/meta)
  - `recommended` — the new value to write
  - `reason` — why the change is needed

## Rules

1. **Read each file before editing**. Do not assume the current content matches `current` exactly — verify.
2. **Apply only the fixes listed**. Do not add extra changes (e.g. don't rewrite the whole article).
3. **Preserve MDX frontmatter format**. Only change the specific fields mentioned.
4. **Write files back** using the exact same path from the `file` field.
5. **Report what you changed** in your output.

## Per-Fix-Type Instructions

### `title_rewrite`
- Update the frontmatter `title:` field to the `recommended` value.
- Also update the markdown `# H1` heading to match (if the H1 currently matches the old title).
- Hard limit: 55 characters max.

### `meta_description`
- Update the frontmatter `description:` or `meta_description:` field.
- If neither field exists, add `description:` to frontmatter.
- Hard limits: 140–155 characters (min 140, max 155).

### `faq_schema`
- Add a JSON-LD FAQPage schema block near the end of the article (before the closing content but after the main text).
- Use this exact format:

```html
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "FAQPage",
  "mainEntity": [
    {
      "@type": "Question",
      "name": "Question text here?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "Answer text here."
      }
    }
  ]
}
</script>
```

- Include 3–5 questions from the `recommended` array.
- If an FAQ schema already exists, merge new questions into `mainEntity` (avoid duplicates).

### `snippet_bait`
- Rewrite the first paragraph (the text immediately after the H1 heading) to the `recommended` value.
- Hard limits: 40–60 words (min 40, max 60). This is a direct answer targeting featured snippets.
- Preserve any markdown formatting around it.

## Output Contract

Return a JSON object:

```json
{
  "applied": [
    {
      "article_id": 42,
      "file": "content/042_article_slug.mdx",
      "changes": [
        {"type": "title_rewrite", "old": "...", "new": "..."},
        {"type": "meta_description", "old": "...", "new": "..."}
      ]
    }
  ],
  "skipped": [
    {"article_id": 43, "reason": "file not found"}
  ],
  "summary": "Applied 5 changes across 3 files. Skipped 1 file (not found)."
}
```

## Constraints

- Do not create new files — only edit existing ones.
- Do not change `date`, `url_slug`, `id`, or other frontmatter fields unless part of the fix.
- If a file does not exist at the given `file` path, skip it and report in `skipped`.
