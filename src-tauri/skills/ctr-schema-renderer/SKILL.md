# CTR Schema Renderer Fix Skill

Used by `fix_ctr_schema_renderer` agentic steps.

## Objective

Produce a structured fix plan for missing FAQPage JSON-LD schema rendering. The source MDX files contain FAQ content, but the rendered HTML does not include the corresponding `application/ld+json` script tag. The target repo owns the schema rendering code; PageSeeds reads it and proposes changes.

## Input

A JSON artifact (`ctr_schema_detection`) containing:
- `affected_articles` — array of:
  - `article_id`, `url_slug`, `file`
  - `source_faq_question_count` — number of FAQ items in the MDX source
  - `rendered_schema_types` — array of schema types found in rendered HTML (e.g. `["Article", "Organization"]`)
  - `rendered_faq_question_count` — number of FAQ items in rendered JSON-LD (usually `0`)
- `framework_files` — array of `{path, content_preview}` for detected framework files
- `site_url` — the project's base URL

## Rules

1. **Read the framework files carefully**. Use the `content_preview` to identify where JSON-LD schema is constructed and rendered.
2. **Propose minimal changes**. Add or extend the schema generation to include `FAQPage` with the FAQ items from the source.
3. **Do not edit article content files**. This is a schema-renderer fix; the source already has FAQ content.
4. **Return structured JSON** matching the Output Contract below.

## Per-Framework Guidance

### Next.js (App Router)
- Look for `metadata` export, `jsonLd` export, or a `<Script type="application/ld+json">` pattern in page/layout files.
- Check if a component like `JsonLd`, `SchemaOrg`, or `StructuredData` exists in `components/`.
- The fix usually involves extending the JSON-LD array/object to include `FAQPage` with `mainEntity` entries.

### Next.js (Pages Router)
- Look for `next-seo` or `react-schemaorg` usage.
- The fix usually involves adding a `<JsonLd>` component or extending the existing JSON-LD object.

### Astro
- Look for `<script type="application/ld+json">` in layout or page components.
- The fix usually involves conditionally adding a `FAQPage` object when the page has FAQ content.

### Gatsby
- Look for `gatsby-plugin-next-seo` or `react-helmet` JSON-LD usage.
- The fix usually involves extending the SEO component to include FAQ schema.

## Output Contract

Return a JSON object exactly matching this structure:

```json
{
  "framework": "nextjs-app",
  "fix_plan": {
    "file_to_edit": "app/components/JsonLd.tsx",
    "current_code_snippet": "const jsonLd = { '@context': 'https://schema.org', '@type': 'Article', headline: title }",
    "proposed_code_snippet": "const faqItems = pageData.faq?.map((f: any) => ({ '@type': 'Question', name: f.question, acceptedAnswer: { '@type': 'Answer', text: f.answer } })) || [];\nconst jsonLd = { '@context': 'https://schema.org', '@type': 'Article', headline: title, ...(faqItems.length > 0 ? { '@type': 'FAQPage', mainEntity: faqItems } : {}) }",
    "reason": "The source MDX contains FAQ content but the rendered page omits FAQPage JSON-LD. Adding FAQPage to the structured data enables rich results in Google SERPs.",
    "validation_steps": [
      "Run `npm run build`",
      "Open an affected page and verify `<script type=\"application/ld+json\">` contains `FAQPage`",
      "Use Google's Rich Results Test to confirm no errors"
    ]
  }
}
```

**Field rules:**
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
