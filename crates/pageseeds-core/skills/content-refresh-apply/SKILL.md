# Content Refresh Apply

<!-- skill-version: 1 -->

Refresh a decaying MDX article that lost meaningful GSC impressions (recent 28d vs prior 28d). Path B only — edit the full file (prefer full-file over excerpt patches), then `fix-submit -k refresh`.

## Input

You receive:
1. The article's current file contents (frontmatter + body) via the fix package
2. Optional `goals` from the operator (why this page is decaying / what to refresh)
3. Validation rules for each structured field
4. Canonical `file` and `article_id` in the context block
5. `current_year` — UTC calendar year at prompt-build time (number)
6. Available internal link slugs (do not invent off-catalog targets)

## Your job

Produce either:
- A **full-file edit** at `file_absolute` (preferred — Path B happy path), **or**
- A `ContentFixPatch` JSON for structured fields when a small patch is enough

Refresh **stale facts, years, thin sections, and outdated SERP copy** without rewriting the page into a new article.

## Refresh priorities

1. **Preserve what already ranks** — keep the slug, core intent, and internal links that still support the topic.
2. **Update stale years and facts** — titles, meta, intros, body claims that cite old years or outdated products/prices/APIs.
3. **Expand thin sections** — add depth only where the page under-delivers vs query intent; do not pad.
4. **Re-title when helpful** — improve CTR clarity while staying true to the page; title ≤ 60 chars and complete (no dangling words / mid-phrase cuts).
5. **Do not retarget keyword / change URL** — this is a refresh of the existing page, not a new landing.

## Categories and rules (structured patch)

Same shape as content-fix-apply `ContentFixPatch`. Only include fields that need to change:

- **title** → Frontmatter `title:`. ≤ 60 chars, complete phrase. Prefer `current_year` when a year is warranted.
- **h1** → First H1 in body. Match title or SEO-optimized; complete.
- **description** → Frontmatter `description:`. 120–155 chars.
- **intro** → Opening paragraph(s). 40–60 words for structured patch; full-file may expand more carefully.
- **internal_links** → Add/fix links only to catalog slugs; keep links that still rank/support intent.
- **faq** → Add frontmatter FAQ (3–5) only if the file has no existing FAQ.
- **eeat** → Credibility signal (author note, data source, experience).
- **cta** → Strengthen or refresh call-to-action when conversion path is weak.

### Year freshness (title / description / body)

- Prefer `current_year` from context when a year is warranted.
- If title or description contains **any** 20xx calendar year, **every** such year must equal `current_year`.
- No dual-year ranges (e.g. "2025-2026"), no stale years.
- In full-file body edits, update obvious stale years and dated claims; do not invent new statistics.

## Validation rules (enforced by Rust on submit)

- title: ≤ 60 chars if provided via patch
- description: 120–155 chars if provided via patch
- intro: 40–60 words if provided via patch
- faq_questions: 3–5 if provided and file has no existing FAQ
- title / description years must equal `current_year` when present
- New `/blog/` links must resolve to live catalog slugs (fail-closed)
- Structural: MDX + H1 + frontmatter title required

## Output contract

### Option A — full-file edit (preferred)

Edit the MDX at `file_absolute` directly, then run:

```bash
pageseeds-cli fix-submit -S <slug> -k refresh
```

Do not change the URL slug. Keep internal links that still support ranking unless they are wrong.

### Option B — ContentFixPatch JSON

Return a `ContentFixPatch` JSON. `article_id` and `file` are set by the system — do not invent paths:

```json
{
  "article_id": 0,
  "file": "",
  "changes": {
    "title": "Updated Title (≤60 chars)",
    "description": "Updated meta description (120-155 chars)",
    "intro": "Refreshed opening paragraph (40-60 words)",
    "h1": "Updated H1 heading",
    "internal_links": [
      {"anchor_text": "related topic", "target_slug": "related-article"}
    ],
    "faq_questions": [
      {"question": "Q1?", "answer": "A1"}
    ],
    "eeat_signal": "Added author credential or data source",
    "cta": "Refreshed call-to-action"
  }
}
```

Omit any `changes` field that does not need updating. Do not wrap in markdown fences.

## Anti-patterns

- Changing slug / inventing a new URL
- Full rewrite that abandons ranking intent
- Removing internal links that still rank without replacement
- Inventing stats, quotes, or off-catalog link targets
- Nested `execute-task fix_*` generate — Path B only
