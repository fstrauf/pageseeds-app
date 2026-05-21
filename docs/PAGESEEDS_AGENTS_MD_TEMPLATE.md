# PageSeeds Automation Directory

This directory is managed by PageSeeds, a desktop SEO tool that analyzes your site
and generates actionable fixes.

## What's Here

| File | Purpose | When to act on it |
|------|---------|-------------------|
| `seo_feature_spec.md` | Code-level fixes: template bugs, redirect rules, sitemap gaps | **Apply immediately** — exact file paths and code provided |
| `content_audit.json` | Per-article health scores and check results | Reference when fixing individual articles |
| `articles.json` | Article metadata synced with MDX files | Managed by PageSeeds; don't edit manually |
| `investigations/` | Agentic investigation results | Review findings, then apply fixes |
| `cannibalization_strategy.json` | Content cannibalization clusters | Review before consolidating articles |
| `task_list.json` | PageSeeds task definitions | Read-only; managed by the app |

## What to Do When You See a Feature Spec

1. Read `.github/automation/seo_feature_spec.md`
2. Apply each `## Issue` section in order — the file path, current code, and fixed code are provided
3. After applying a fix, verify it works (build the site, check the affected pages)
4. Delete the spec file or the sections you've applied

## PageSeeds Content Conventions

- All content lives in MDX files (`.mdx`) with YAML frontmatter
- Article IDs are assigned by PageSeeds (numeric prefix in filenames like `042_slug.mdx`)
- Don't rename files or change IDs without running PageSeeds' content sync
- Frontmatter fields: `title`, `description`, `date`, `status`, `target_keyword`

## Running PageSeeds Tasks

If you need to trigger a PageSeeds task (content audit, cannibalization analysis, etc.),
open the PageSeeds desktop app and use the Health dashboard or Overview quick actions.
PageSeeds writes output to this directory.
