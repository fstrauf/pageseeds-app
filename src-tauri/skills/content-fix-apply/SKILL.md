# Content Fix Apply

<!-- skill-version: 3 -->

Apply SEO content improvements to a single MDX article based on structured recommendations.

## Input

You receive:
1. The article's current file contents (frontmatter + body)
2. A list of structured recommendations (category, current text, proposed text, reason)
3. Validation rules for each field
4. Canonical `file` and `article_id` in the context block

## Your job

Produce a `ContentFixPatch` JSON that addresses every recommendation **unless the current file already satisfies the requirement**.

Only include fields that need to change. Do not include a field if the fix is already satisfied.

**Identity is filled by the system from context.** Do not invent `file` paths or `article_id` values. Prefer omitting them or copying the exact values from the context block — never use placeholders like `content/001_article.mdx`.

## Categories and rules

- **title** → Update frontmatter `title:` field. Must be ≤ 60 chars and a complete, grammatically correct phrase. It must NOT end mid-sentence or with dangling words (`a`, `an`, `the`, `and`, `or`, `to`, `for`, `of`, `in`, `on`, `with`, `by`, `from`, `as`, `is`, `are`, `what`, `how`, `when`, `where`, `why`, `which`, `complete`, `guide`, `income`, `without`, `track`, `close`, `compared`) or trailing punctuation (`:`, `,`, `-`). Rewrite rather than truncate.
- **h1** → Update the first H1 heading in the body. Must match title or be optimized for SEO, and must also be complete.
- **description** → Update frontmatter `description:` field. Must be 120-155 chars.
- **intro** → Rewrite the opening paragraph(s). Must be 40-60 words.
- **internal_links** → Add suggested links at appropriate places in the body.
- **faq** → Add frontmatter FAQ questions (3-5 questions max). Only if file has no existing FAQ.
- **eeat** → Add credibility signal (author note, data source, or experience statement).
- **cta** → Add or strengthen call-to-action.


## Validation rules (enforced by Rust)

- title: ≤ 60 chars if provided
- description: 120-155 chars if provided
- intro: 40-60 words if provided
- faq_questions: 3-5 questions if provided and file has no existing FAQ
- Every proposed change must differ from the current text
- Empty `changes: {}` is **not** success when open suggestions still fail content health (title length, meta length, intro word count, missing H1/FAQ)


## Output contract

Return a `ContentFixPatch` JSON with these fields. `article_id` and `file` are set by the system from context — copy context values if required by the schema; do not invent paths:

```json
{
  "article_id": 0,
  "file": "",
  "changes": {
    "title": "New Title (≤60 chars)",
    "description": "New meta description (120-155 chars)",
    "intro": "New opening paragraph (40-60 words)",
    "h1": "New H1 heading",
    "internal_links": [
      {"anchor_text": "related topic", "target_slug": "related-article"}
    ],
    "faq_questions": [
      {"question": "Q1?", "answer": "A1"}
    ],
    "eeat_signal": "Added author credential or data source",
    "cta": "New or improved call-to-action"
  }
}
```

Omit any `changes` field that does not need updating. Do not wrap in markdown fences.
