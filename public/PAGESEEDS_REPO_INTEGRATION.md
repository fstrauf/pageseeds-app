# PageSeeds SEO Content Rendering Contract

This repository receives or maintains content created by PageSeeds. PageSeeds may create and update blog posts, MDX/Markdown files, and structured frontmatter fields. The repository is responsible for rendering that content correctly.

## Core Principle

PageSeeds owns content data.
This repo owns rendering.

The goal is SEO through complete page rendering, not metadata storage. Structured frontmatter must become:

- Visible page content where appropriate, especially FAQ, HowTo, citations, dates, author, category, tags, and reading time.
- Head metadata, including title, description, canonical URL, Open Graph, Twitter card, and article dates.
- JSON-LD structured data generated from the same parsed source.

Do not solve PageSeeds metadata by inserting raw `<script type="application/ld+json">` blocks into individual articles unless the repo has no layout/schema system at all. Prefer a centralized renderer that reads frontmatter/content data and emits visible content, metadata, Open Graph tags, canonical URLs, and JSON-LD.

If an implementation only parses frontmatter and emits JSON-LD, it is incomplete.

## Required Behavior

When a blog post contains structured frontmatter, the app must parse and use it. Do not ignore nested frontmatter fields.

At minimum, support these fields when present:

```yaml
title: "Article title"
description: "Meta description"
slug: article-slug
date: "2026-04-29"
lastModified: "2026-04-29"
author: "Site or author name"
category: "Category"
tags: ["tag one", "tag two"]
image: "/path/to/social-image.png"
canonicalUrl: "https://example.com/canonical-path"
readingTime: 8

faq:
  - question: "Question?"
    answer: "Answer."

howTo:
  name: "How to do X"
  description: "Short summary"
  steps:
    - name: "Step one"
      text: "Instruction text"

citations:
  - source: "Source name"
    url: "https://example.com"
    date: "2026-04-29"
```

## Frontmatter Parsing

Use a real YAML/frontmatter parser. Do not parse frontmatter by splitting each line on `:` because that fails for nested arrays like `faq`, `howTo`, and `citations`.

Good options:
- `gray-matter`
- `yaml`
- framework-native content collections
- Astro content collections
- Nuxt/Content
- Contentlayer
- unified/remark frontmatter tooling

The parsed blog post model should expose structured fields, for example:

```ts
type BlogPost = {
  title: string
  slug: string
  description?: string
  date?: string
  lastModified?: string
  author?: string
  category?: string
  tags?: string[]
  image?: string
  canonicalUrl?: string
  readingTime?: number
  faq?: { question: string; answer: string }[]
  howTo?: {
    name?: string
    description?: string
    steps?: { name: string; text: string; url?: string }[]
  }
  citations?: { source: string; url?: string; date?: string }[]
  content: string
}
```

## End-to-End Rendering Requirement

PageSeeds compatibility means the structured fields are carried all the way from file data to the rendered page. The work is not complete after parsing.

Every implementation must handle this full path:

1. Parse frontmatter with a real parser.
2. Preserve nested structured fields in the blog post model.
3. Pass the parsed data into the blog/article layout.
4. Render visible article content from the parsed data when the field is content-like.
5. Render head metadata from the parsed data when the field is metadata-like.
6. Render JSON-LD from the parsed data when the field has a schema.org representation.
7. Verify the final rendered HTML, not only unit-level parsed objects.

Content-like fields include `faq`, `howTo`, and `citations`. These must be visible to readers unless the same content already appears in the Markdown body and the implementation can prove it is synchronized.

Metadata-like fields include `title`, `description`, `canonicalUrl`, `date`, `lastModified`, `author`, `category`, `tags`, `image`, and `readingTime`. These should appear in the appropriate page chrome, head metadata, or article metadata area.

## Metadata Rendering

Every article page should render:

- `<title>` from `title`
- meta description from `description`
- canonical URL from `canonicalUrl`, `canonical`, or the route, in that precedence order
- Open Graph title, description, image, URL, and type
- Twitter card metadata
- article published and modified dates when available
- article tags/category when available
- author when available
- reading time when available

Use the canonical URL consistently. If a post provides `canonicalUrl` or `canonical`, the canonical link, Open Graph URL, Twitter URL if supported, `Article.url`, and `mainEntityOfPage.@id` should not disagree without an explicit reason.

Metadata should be rendered by the framework's head system, not by hand inside article content.

Examples:
- Vue: `@unhead/vue`, `useHead`
- Nuxt: `useSeoMeta`, `useHead`
- Next.js: `generateMetadata`
- Astro: layout frontmatter/head component
- SvelteKit: `<svelte:head>`

## Structured Data Rendering

Render JSON-LD centrally from parsed post data.

For every article, render `Article` or `BlogPosting` schema using:

- `title`
- `description`
- `date`
- `lastModified`
- `author`
- `image`
- canonical URL
- category/tags
- word count if available

If `faq` exists and has at least one item, render `FAQPage` schema from it:

```json
{
  "@context": "https://schema.org",
  "@type": "FAQPage",
  "mainEntity": [
    {
      "@type": "Question",
      "name": "Question?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "Answer."
      }
    }
  ]
}
```

If `howTo` exists and has steps, render `HowTo` schema from it.

If breadcrumbs are available, render `BreadcrumbList`.

Do not maintain separate hard-coded FAQ schema in page templates when the article already has `faq` data. The frontmatter should be the source of truth.

Structured data must describe visible page content. Do not emit FAQPage or HowTo JSON-LD from frontmatter while hiding the corresponding questions, answers, or steps from users.

## Visible Content Rendering

For SEO, PageSeeds frontmatter is not only machine metadata. Some fields are page content and must be rendered in the article layout.

When `faq` exists and has at least one valid item:

- Render a visible FAQ section in or near the article body.
- Render every valid question and answer from the same parsed `faq` array used for JSON-LD.
- Do not require PageSeeds to duplicate the FAQ in Markdown just to make it visible.

When `howTo` exists and has steps:

- Render a visible HowTo or Steps section unless equivalent step content already exists in the article body.
- Render the same step names and text used for HowTo JSON-LD.

When `citations` exists:

- Render a visible Sources, References, or Citations section.
- Link citation URLs when present.
- Preserve source names and dates when present.

When `readingTime`, `author`, `category`, `tags`, `date`, or `lastModified` exists:

- Render them in the article header, byline, article metadata row, footer metadata, or another consistent article UI.

The article page should not silently discard supported PageSeeds fields after parsing them.

## FAQ Content Rule

FAQ data should have one canonical source.

Preferred order:

1. Structured frontmatter `faq`
2. A dedicated structured content block supported by the repo
3. Markdown FAQ section parsed into structured data
4. Raw inline JSON-LD only as a temporary fallback

Do not allow these to drift:
- frontmatter FAQ
- visible FAQ section
- JSON-LD FAQ schema

If visible FAQ content is rendered on the page, JSON-LD should describe the same questions and answers.

If FAQ frontmatter exists but no visible FAQ section exists after rendering, the implementation is not PageSeeds-compatible.

## PageSeeds Compatibility Rule

When PageSeeds adds or updates frontmatter fields, the repo should render those fields without needing PageSeeds to insert framework-specific code into each article.

PageSeeds may create:

- improved titles
- improved meta descriptions
- first-paragraph/snippet improvements
- FAQ question/answer data
- HowTo data
- citation data
- canonical metadata
- freshness metadata

The repo must convert that data into the final rendered page.

## What Not To Do

Do not put raw JSON-LD scripts at the bottom of MDX files as the default solution.

Do not duplicate the same FAQ in frontmatter, markdown body, and inline JSON-LD unless the build process keeps them synchronized.

Do not ignore nested frontmatter fields.

Do not parse frontmatter with fragile line-based string splitting.

Do not let article-level agents decide framework rendering behavior. Rendering belongs in layout/components/templates.

Do not stop after adding parser tests if the rendered page still hides the content.

Do not treat JSON-LD as a substitute for user-visible content.

## Implementation Checklist

A repo is PageSeeds-compatible when:

- Blog content uses a real frontmatter parser.
- Nested `faq`, `howTo`, and `citations` fields are preserved.
- The blog post type includes PageSeeds-supported metadata fields.
- The article layout passes parsed metadata into the head/SEO component.
- The article layout renders visible FAQ content from `faq` frontmatter.
- The article layout renders visible HowTo steps from `howTo.steps` frontmatter, unless the same steps are already visibly rendered from the body content.
- The article layout renders visible citations/sources from `citations` frontmatter.
- Article metadata UI renders relevant supported fields such as author, category, tags, reading time, date, and last modified date when present.
- JSON-LD is generated centrally from parsed data.
- FAQPage schema is rendered when `faq` exists.
- HowTo schema is rendered when `howTo.steps` exists.
- FAQPage and HowTo schema describe visible content on the page.
- Canonical, Open Graph, Twitter, article date, and modified date tags render correctly.
- Canonical URL, Open Graph URL, and Article JSON-LD URL are consistent when a canonical value is provided.
- Existing articles with PageSeeds frontmatter build without errors.
- A rendered page source check confirms visible FAQ/HowTo/citation content appears outside JSON-LD scripts.
- A rendered page source check confirms the schema appears in the final HTML/head.
- No duplicate or conflicting FAQPage schema is emitted.

## Minimum Acceptance Tests

Add or update tests that prove the complete behavior, not only parsing:

- Parser test: nested `faq`, `howTo`, and `citations` survive frontmatter parsing.
- Layout/render test: a post with `faq` renders visible questions and answers.
- Layout/render test: a post with `howTo.steps` renders visible step names and text.
- Layout/render test: a post with `citations` renders visible source names and links.
- Metadata test: `canonicalUrl`/`canonical`, dates, description, image, and tags reach the framework head metadata.
- Structured-data test: Article, FAQPage, HowTo, and BreadcrumbList JSON-LD are generated from the parsed post data.
- Drift test or assertion: FAQ JSON-LD questions match the visible FAQ questions.

At least one validation step must inspect rendered HTML or server output and confirm that FAQ text appears outside `<script type="application/ld+json">` blocks.

## Validation Prompt For Agents

When updating this repo for PageSeeds compatibility, do the following:

1. Find the blog/content loading pipeline.
2. Replace fragile frontmatter parsing with a real YAML/frontmatter parser if needed.
3. Extend the post/content type to include PageSeeds fields.
4. Find the article layout or SEO/head component.
5. Render visible FAQ, HowTo, citations, and article metadata from parsed post data.
6. Render metadata and JSON-LD from the same parsed post data.
7. Ensure `faq` frontmatter produces both a visible FAQ section and FAQPage JSON-LD.
8. Ensure `howTo` frontmatter produces both visible steps and HowTo JSON-LD.
9. Ensure `citations` frontmatter produces visible citations or references.
10. Ensure canonical, Open Graph, Twitter, and Article JSON-LD URLs are consistent.
11. Add or update tests/fixtures for nested frontmatter and rendered article output.
12. Run the repo's typecheck, tests, and build.
13. Inspect rendered output for at least one article with FAQ data and confirm the FAQ text appears outside JSON-LD scripts.
14. Inspect rendered output for duplicate/conflicting FAQPage schema.

The final implementation should preserve the rule: PageSeeds creates structured content, this repo renders it.
