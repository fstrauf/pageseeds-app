# Content Fix Apply

Apply SEO content improvements to a single MDX article based on structured recommendations.

## Input

You receive:
1. The article's current file contents (frontmatter + body)
2. A list of structured recommendations (category, current text, proposed text, reason)
3. Validation rules for each field

## Your job

Produce a `ContentFixPatch` JSON that addresses every recommendation **unless the current file already satisfies the requirement**.

Only include fields that need to change. Do not include a field if the fix is already satisfied.

## Categories and rules

- **title** → Update frontmatter `title:` field. Must be ≤ 60 chars.
- **h1** → Update the first H1 heading in the body. Must match title or be optimized for SEO.
- **description** → Update frontmatter `description:` field. Must be 120-155 chars.
- **intro** → Rewrite the opening paragraph(s). Must be 40-80 words.
- **internal_links** → Add suggested links at appropriate places in the body.
- **faq** → Add frontmatter FAQ questions (3-5 questions max). Only if file has no existing FAQ.
- **eeat** → Add credibility signal (author note, data source, or experience statement).
- **cta** → Add or strengthen call-to-action.
- **date** → Update frontmatter `date:` field.

## Validation rules (enforced by Rust)

- title: ≤ 60 chars if provided
- description: 120-155 chars if provided
- intro: 40-80 words if provided
- faq_questions: 3-5 questions if provided and file has no existing FAQ
- Every proposed change must differ from the current text

## Output contract

Return a `ContentFixPatch` JSON with these fields:

```json
{
  "article_id": 123,
  "file": "content/001_article.mdx",
  "changes": {
    "title": "New Title (≤60 chars)",
    "description": "New meta description (120-155 chars)",
    "intro": "New opening paragraph (40-80 words)",
    "h1": "New H1 heading",
    "internal_links": [
      {"anchor_text": "related topic", "target_slug": "related-article"}
    ],
    "faq_questions": [
      {"question": "Q1?", "answer": "A1"}
    ],
    "eeat_signal": "Added author credential or data source",
    "cta": "New or improved call-to-action",
    "date": "2026-05-12"
  }
}
```

Omit any `changes` field that does not need updating. Do not wrap in markdown fences.
