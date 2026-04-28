# CTR Site Template Fix Skill

Used by `fix_ctr_site_template` agentic steps.

## Objective

Produce a structured fix plan for site-wide title template duplication. The target repo owns the template rendering code; PageSeeds reads it and proposes changes, but the user must apply them in the target repo.

## Input

A JSON artifact (`ctr_template_detection`) containing:
- `pattern` — the detected repeated suffix/prefix string (e.g. `" | SiteName"`)
- `affected_articles` — array of `{article_id, url_slug, current_title}`
- `framework_files` — array of `{path, content_preview}` for detected framework files
- `site_url` — the project's base URL

## Rules

1. **Read the framework files carefully**. Use the `content_preview` to identify which file controls the `<title>` tag rendering.
2. **Propose minimal changes**. Only modify the template logic that injects the repeated pattern.
3. **Do not edit article content files**. This is a site-template fix, not per-article.
4. **Return structured JSON** matching the Output Contract below.

## Per-Framework Guidance

### Next.js (App Router)
- Look for `metadata.title.template` or `metadata.title.default` in `layout.tsx` / `app/layout.tsx`.
- The fix usually involves removing or shortening the `template` string.

### Next.js (Pages Router)
- Look for `<Head><title>...` in `_app.tsx` or page components.
- The fix usually involves changing the title composition logic.

### Astro
- Look for `<title>` in `src/layouts/*.astro` or `src/components/Head.astro`.
- The fix usually involves changing the `{title}` interpolation.

### Gatsby
- Look for `react-helmet` or `gatsby-plugin-react-helmet` usage in `src/components/seo.js` or `gatsby-ssr.js`.
- The fix usually involves changing the `titleTemplate` prop.

## Output Contract

Return a JSON object exactly matching this structure:

```json
{
  "pattern": " | SiteName",
  "framework": "nextjs-app",
  "fix_plan": {
    "file_to_edit": "app/layout.tsx",
    "current_code_snippet": "export const metadata = { title: { template: '%s | SiteName' } }",
    "proposed_code_snippet": "export const metadata = { title: { template: '%s' } }",
    "reason": "The brand suffix adds 12 characters to every title, causing truncation on mobile SERPs. Removing it from the template and adding it manually only to homepage/brand pages fixes the issue.",
    "validation_steps": [
      "Run `npm run build`",
      "Check that article titles no longer contain ' | SiteName'",
      "Verify homepage title still includes brand"
    ]
  }
}
```

**Field rules:**
- `pattern` — echo the detected pattern from input
- `framework` — one of: `nextjs-app`, `nextjs-pages`, `astro`, `gatsby`, `unknown`
- `fix_plan.file_to_edit` — relative path to the file the user should edit
- `fix_plan.current_code_snippet` — the exact current code to find (for search/replace)
- `fix_plan.proposed_code_snippet` — the new code to replace it with
- `fix_plan.reason` — human-readable justification for the change
- `fix_plan.validation_steps` — array of strings, steps the user should take after applying

## Constraints

- Do not write files — only return the structured JSON.
- Do not assume file paths beyond what is in `framework_files`.
- If no fix is possible, set `fix_plan` to `null` and explain why in a `notes` field.
