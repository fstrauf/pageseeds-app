# Indexing Fix Skill

<!-- skill-version: 2 -->

Used by the `fix_indexing` / `fix_technical` agentic generate step.

## Instructions

For `not_indexed_crawled` / `not_indexed_discovered` / `not_indexed_other`:
- Improve content depth and uniqueness (aim for 600+ words if currently thin)
- Ensure the H1 and title are specific and distinct from similar pages
- Add a clear meta description
- Rewrite the intro (first paragraph) when it is thin or overlaps with siblings

For `robots_blocked` / `noindex` / `fetch_error` / `canonical_mismatch`:
- Plan the technical root-cause fix as frontmatter changes (e.g. set `robots`
  to `index, follow`, set or fix `canonical`) via `frontmatter` edits

For `not_indexed_crawled` specifically (page is crawled but not indexed):
- This usually means Google sees the page but chooses not to index it.
- The page is already long and may have internal links — focus on DISTINCTIVENESS, not just length.
- Make the title, H1, and opening sections clearly different from cluster siblings listed above.
- If the page cannot be made distinct enough, say so in `diagnosis`.

When `Suggested title:` / `Suggested H1:` values are provided in the context
(from the site-wide audit), use them as the basis for your `title` / `h1`
changes. Adjust only when they violate the rules above.

## Output Contract

CRITICAL: You do NOT have file access. Do NOT edit, create, or describe files.
Return ONLY a single JSON object proposing the changes — the deterministic
apply step performs all file writes:

```json
{
  "diagnosis": "one-line root-cause summary",
  "changes": {
    "title": "new frontmatter title (optional)",
    "h1": "new top-level body heading, without the leading '# ' (optional)",
    "description": "new meta description, 120-160 chars (optional)",
    "intro": "full replacement first paragraph (optional)",
    "frontmatter": [{ "key": "robots", "value": "index, follow" }]
  }
}
```

Rules:
- Include ONLY the fields that need to change. Omit the rest (including
  `frontmatter`) rather than returning empty values.
- At least one change is required — a plan with no changes is a failure.
- `h1` is the heading text only (no `# ` prefix).
- `frontmatter` entries set arbitrary frontmatter scalars; use them only for
  technical fixes (robots, canonical). Do not use them for `title` or
  `description` — those have dedicated fields.
- Do not create markdown reports, summary files, or documentation.
