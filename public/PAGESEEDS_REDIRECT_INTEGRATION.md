# PageSeeds Redirect Integration Contract

This repository receives redirect rules created by PageSeeds during content consolidation (merge) workflows. When PageSeeds detects cannibalized or duplicate content, it merges valuable content into a single keeper article and generates 301 redirect rules for the deprecated URLs.

## Core Principle

PageSeeds owns the redirect decision (which URLs to redirect where).
This repo owns the redirect implementation (how the web server responds to those URLs).

## What PageSeeds Generates

After approving a merge recommendation, PageSeeds writes or updates:

`.github/automation/redirects.csv`

```csv
source,destination,status
/blog/old-article-slug,/blog/keeper-article-slug,301
/blog/another-deprecated-url,/blog/canonical-article-slug,301
```

This file is cumulative. Each merge task appends new rules without duplicating existing ones.

PageSeeds also:
- Modifies the keeper MDX file to include merged content
- Leaves old redirect-target MDX files in place (the repo decides when to remove them)
- Syncs article metadata to `articles.json`

## What the Repo Must Do

The repository must ensure that URLs listed in `redirects.csv` actually return HTTP 301 redirects to their destinations. Without this, Google continues to index the old URLs as duplicate content, defeating the purpose of the merge.

## Implementation Options

### Option A: Next.js `redirects()` (Recommended)

If this is a Next.js app, read `.github/automation/redirects.csv` at build time and return the rules from the `redirects()` config function.

Example:

```typescript
// next.config.ts
import fs from 'fs';
import path from 'path';

function loadPageSeedsRedirects(): Array<{ source: string; destination: string; permanent: boolean }> {
  const csvPath = path.join(process.cwd(), '.github', 'automation', 'redirects.csv');
  if (!fs.existsSync(csvPath)) {
    return [];
  }
  const csv = fs.readFileSync(csvPath, 'utf-8');
  const lines = csv.split('\n').slice(1); // skip header
  return lines
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const [source, destination, status] = line.split(',');
      return {
        source: source.trim(),
        destination: destination.trim(),
        permanent: status.trim() === '301',
      };
    });
}

export default {
  async redirects() {
    const pageSeedsRedirects = loadPageSeedsRedirects();
    const existingRedirects = [
      // ... existing hardcoded redirects ...
    ];
    return [...existingRedirects, ...pageSeedsRedirects];
  },
  // ... rest of config
};
```

### Option B: Vercel `vercel.json`

If hosting on Vercel without Next.js redirects, generate `vercel.json` from the CSV:

```json
{
  "redirects": [
    {
      "source": "/blog/old-article-slug",
      "destination": "/blog/keeper-article-slug",
      "statusCode": 301
    }
  ]
}
```

### Option C: Static Site + CDN Rules

For static generators (Astro, Hugo, etc.), configure redirects at the CDN layer (Cloudflare, Netlify, Vercel) or generate server config files (e.g., `_redirects` for Netlify, `_config.yml` redirects for Jekyll).

## Required Behavior

1. **Read the CSV at build time.** Do not import it as a client-side module.
2. **Return 301 status** for permanent redirects. Do not use 302.
3. **Preserve query strings** if the platform supports it (`destination` with `:path*` or equivalent).
4. **Do not redirect to 404s.** The destination URL must resolve to a real page.
5. **Remove or depublish old pages.** After redirects are live, delete the old MDX files or move them out of the content directory. Keeping them risks Google indexing both the redirect and the old content.

## What Not To Do

- Do not ignore `redirects.csv` and leave old URLs returning 200.
- Do not implement redirects only in client-side JS (React Router, etc.). Search engines need HTTP-level redirects.
- Do not leave old MDX files in `content/blog/` after their redirects are active.
- Do not manually hardcode PageSeeds redirects without reading the CSV. The CSV is the source of truth and updates after each merge.

## Implementation Checklist

A repo is PageSeeds redirect-compatible when:

- [ ] `.github/automation/redirects.csv` is read at build time.
- [ ] Each rule produces an HTTP 301 (or configured status) redirect.
- [ ] Redirects work for both trailing and non-trailing slash variants if the platform does not normalize them automatically.
- [ ] Old redirect-target MDX files are removed from the content directory after deploy.
- [ ] The build succeeds with an empty or missing `redirects.csv` (no hard dependency).
- [ ] A deploy-time check confirms at least one redirect from the CSV responds with 301.

## Validation Prompt For Agents

When wiring up PageSeeds redirects in this repo:

1. Find the framework's redirect mechanism (Next.js `redirects()`, Vercel config, Netlify `_redirects`, etc.).
2. Add a build-time reader for `.github/automation/redirects.csv`.
3. Merge PageSeeds redirects with any existing hardcoded redirects.
4. Ensure 301 status is used for permanent redirects.
5. Update the build/deploy pipeline to delete old MDX files after redirects are confirmed working.
6. Run the build and verify no errors occur when `redirects.csv` is missing.
7. Add a test or smoke check that at least one CSV redirect resolves to 301.

The final implementation should preserve the rule: PageSeeds decides what to redirect, this repo makes the redirect real.
