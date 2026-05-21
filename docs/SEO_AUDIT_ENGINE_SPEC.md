# SEO Audit Engine — Disaggregated Feature Spec

## Verdict

The original monolithic `seo_audit` task type is rejected. It duplicates existing infrastructure and violates the "prefer skill, not handler" rule from AGENTS.md.

**What exists already:**
- `content_audit` — 17-check deterministic audit of MDX frontmatter + body
- `cannibalization_audit` — TF-IDF + cosine similarity + Union-Find clustering (2,646-line execution module)
- `ctr_audit` — template pattern detection, rendered SERP audit, CTR scoring
- `indexing_diagnostics` — sitemap parsing, GSC comparison, drift detection
- `consolidate_cluster` — content merging + redirect rule generation

**What is genuinely new:**
- Temporal URL detection in slugs
- Page bloat proxies from source
- SSR fallback detection from source (missing routes, orphaned slugs)
- Literal template variable detection in frontmatter
- Feature spec generation for developer-actionable fixes
- Generic diff reporting across task artifacts

This spec describes **extensions to existing tasks** and **new reusable utilities**, not a new task type.

---

## Genuinely New: Additions to Existing Tasks

### 1. Extend `content_audit` with 4 New Checks

The existing `content_audit.rs` runs 17 deterministic checks per article via `audit_one_article()`. We add 4:

| New Check | What It Does | Why It Matters |
|-----------|-------------|----------------|
| `temporal_url` | Flags slugs containing month names, years, seasons, or relative times (`today`, `next-week`) | daystoexpiry.com had 8 temporal URLs; signals temporary content to Google |
| `page_bloat_proxy` | Flags articles with MDX file > 50KB, > 10 images, > 5 tables, or > 5 code blocks | daystoexpiry.com median was 170KB rendered; proxies for likely bloat |
| `literal_template_variable` | Detects literal strings like `\| Brand \|`, `{Brand}`, `{{title}}` in frontmatter title | daystoexpiry.com had `\| Brand \|` in titles; indicates template bug |
| `title_token_duplication` | Detects repeated tokens in title (e.g. brand name appears twice) | daystoexpiry.com: 150/150 pages had duplicated brand |

#### Implementation

In `engine/exec/content_audit.rs`, extend `audit_one_article()`:

```rust
// Add to the check pipeline inside audit_one_article()
let temporal_check = check_temporal_url(&article.url_slug);
let bloat_check = check_page_bloat_proxy(&file_content, &body);
let literal_check = check_literal_template_variable(&title);
let dup_token_check = check_title_token_duplication(&title);
```

Each returns a `serde_json::Value` appended to the per-article result. No new task type, handler, or step kind required.

#### Temporal URL Regex Patterns

```rust
const TEMPORAL_PATTERNS: &[&str] = &[
    r"\b(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*[-_]?\d{4}\b",
    r"\b\d{4}\b",
    r"\b(spring|summer|fall|autumn|winter)[-_]?\d{4}\b",
    r"\b(today|now|right-now|this-week|next-week|this-month|next-month)\b",
];
```

---

### 2. Extend `content_audit` with Exact Duplicate Detection

Add an `md5_body_hash` field to each article's audit record in `content_audit.json`.

In `audit_one_article()`:

```rust
let normalized_body = normalize_body_for_hash(body); // strip markdown syntax, whitespace
let body_hash = format!("{:x}", md5::compute(normalized_body));
```

The post-processing step (after all articles are audited) groups articles by `body_hash` and flags groups with ≥2 members.

This is cheaper than `CanExactKeywordDupes` (which requires GSC data) and catches true content duplicates, not just keyword duplicates.

---

### 3. Extend `ctr_audit` Template Detection for Literal Variables

The existing `CtrTemplateDetect` step already groups pages by common title suffix and identifies framework files. We extend it to detect:

| New Detection | How |
|--------------|-----|
| `literal_variable_in_template` | Searches `layout.tsx` / `_app.tsx` for strings like `{Brand}`, `{{brand}}`, `<%= brand %>` |
| `missing_dynamic_title` | Checks if the title template is static (e.g. `<title>Site Name</title>`) rather than dynamic (`<title>{pageTitle} | {brand}</title>`) |

This extends `engine/exec/ctr_audit/template.rs` — the same file that already detects brand duplication and framework patterns.

---

### 4. New: SSR Fallback Detection from Source

**Where it lives:** New check in `content::ops::sync_and_validate()` or as a standalone helper called by `indexing_diagnostics`.

**What it does:**
1. List all MDX files in the content directory
2. List all route files in the framework (e.g. `app/blog/[slug]/page.tsx`, `pages/blog/[slug].tsx`)
3. Find MDX files with no matching route (would 404)
4. Find route parameters that don't match any MDX file (would 404 with generic title)

**Why this is better than crawling:** Crawling detects `__next_error__` after the fact. Source analysis detects the missing route *before* Google ever sees it.

```rust
pub fn detect_orphaned_slugs(content_dir: &Path, routes_dir: &Path) -> Vec<OrphanedSlug> {
    let mdx_slugs: HashSet<String> = collect_mdx_slugs(content_dir);
    let route_patterns: Vec<RoutePattern> = collect_route_patterns(routes_dir);
    
    mdx_slugs.into_iter()
        .filter(|slug| !route_patterns.iter().any(|p| p.matches(slug)))
        .map(|slug| OrphanedSlug {
            slug,
            file_path: content_dir.join(format!("{}.mdx", slug)),
            issue: "No route renders this slug".to_string(),
        })
        .collect()
}
```

---

### 5. New Reusable Utility: Feature Spec Generation

**What it is:** A helper function (not a task type) that writes a markdown feature specification to `.github/automation/seo_feature_spec.md`.

**Where it is called:** From `post_actions.rs` after any task that detects code-level issues:
- After `content_audit` — for temporal URLs, title token duplication, literal variables
- After `ctr_audit` — for template bugs
- After `indexing_diagnostics` — for sitemap gaps, missing redirects
- After `cannibalization_audit` — for merge recommendations + redirect rules

**Interface:**

```rust
pub fn generate_feature_spec(
    project_path: &str,
    issues: Vec<DeveloperActionableIssue>,
) -> Result<PathBuf, String> {
    let spec = format_feature_spec(issues);
    let path = ProjectPaths::from_path(project_path)
        .in_automation("seo_feature_spec.md");
    std::fs::write(&path, spec)?;
    Ok(path)
}
```

**Output format:**
```markdown
# SEO Feature Specification

Generated by PageSeeds on 2026-05-21

## Issue 1: Title Template Duplicates Brand Name

**Severity:** Critical | **Impact:** All pages have truncated SERP titles
**Detected by:** ctr_audit / CtrTemplateDetect
**File to edit:** `app/layout.tsx`
**Current:** `{title} | {brand} | {brand}`
**Fixed:** `{title} | {brand}`

## Issue 2: Missing 301 Redirects

**Severity:** Critical | **Impact:** 7 URLs return 404
**Detected by:** indexing_diagnostics
**File to edit:** `next.config.js`
```javascript
{ source: '/blog/wheel-strategy', destination: '/blog/wheel-strategy-guide', permanent: true },
```
```

---

### 6. New Reusable Utility: Diff Reporting

**What it is:** A generic artifact-comparison helper that compares the current task artifact against a previous snapshot.

**Where it lives:** `engine/exec/utils/diff.rs` — framework-agnostic, works with any JSON artifact.

```rust
pub fn diff_artifacts<T: serde::Serialize + serde::de::DeserializeOwned + HashableIssue>(
    current: &T,
    previous: Option<&T>,
) -> ArtifactDiff {
    // Compare by hashing each issue's (category, url, check_type) tuple
}
```

**Used by:**
- `content_audit` — diff `content_audit.json` against previous run
- `ctr_audit` — diff `ctr_audit_context.json` against previous run
- `cannibalization_audit` — diff `cannibalization_strategy.json` against previous run

**Frontend:** The `SeoAuditDashboard` reads diff data from each artifact's metadata and renders a unified "+3 new, -2 resolved" banner.

---

## What Stays Exactly As-Is

| Feature | Existing Implementation | No Changes Needed |
|---------|------------------------|-------------------|
| Cannibalization clustering | `cannibalization_audit` task with 7-step pipeline | ✅ Reuse as-is |
| Sitemap orphan detection | `indexing_diagnostics` + `gsc_diagnostics` | ✅ Reuse as-is |
| Redirect rule generation | `consolidate_cluster` → `MergeGenerateRedirects` step | ✅ Reuse as-is |
| Title template detection | `ctr_audit` → `CtrTemplateDetect` step | ✅ Extend, don't replace |
| Content quality scoring | `content_audit` → 17 existing checks | ✅ Extend, don't replace |
| Follow-up task spawning | `post_actions.rs` + `TaskSpawner` | ✅ Reuse as-is |

---

## Frontend: SEO Audit Dashboard (Composition, Not Monolith)

The dashboard does **not** depend on a single `seo_audit` task. It is a composition layer that reads from multiple existing task artifacts:

```typescript
// SeoAuditDashboard.tsx
async function loadSeoData(projectId: number) {
  const [contentAudit, cannibalization, ctrAudit, indexing] = await Promise.all([
    getContentAuditReport(projectId),      // content_audit.json
    getCannibalizationStrategy(projectId),  // cannibalization_strategy.json
    getCtrAuditContext(projectId),          // ctr_audit_context.json
    getIndexingDiagnostics(projectId),      // indexing_diagnostics output
  ]);
  
  return {
    metaHealth: extractMetaIssues(contentAudit),
    cannibalizationClusters: cannibalization?.clusters ?? [],
    templateIssues: extractTemplateIssues(ctrAudit),
    temporalUrls: extractTemporalUrls(contentAudit),
    bloatIssues: extractBloatIssues(contentAudit),
    sitemapOrphans: indexing?.orphans ?? [],
    missingRedirects: indexing?.missing_redirects ?? [],
    featureSpec: await getSeoFeatureSpec(projectId), // seo_feature_spec.md
    diff: computeUnifiedDiff(contentAudit, cannibalization, ctrAudit, indexing),
  };
}
```

This is a **read-only composition** — no new backend task required.

---

## Files to Modify (Minimal)

| File | Change | Lines |
|------|--------|-------|
| `engine/exec/content_audit.rs` | Add 4 checks to `audit_one_article()` | ~80 |
| `engine/exec/content_audit.rs` | Add `md5_body_hash` + duplicate grouping | ~40 |
| `engine/exec/ctr_audit/template.rs` | Add literal variable + missing dynamic title detection | ~60 |
| `content/ops.rs` or `indexing_diagnostics` | Add `detect_orphaned_slugs()` helper | ~50 |
| `engine/exec/utils/` (new `diff.rs`) | Generic artifact diff helper | ~60 |
| `engine/post_actions.rs` | Add `generate_feature_spec()` calls after relevant tasks | ~30 |
| `src/components/seo/SeoAuditDashboard.tsx` | Composition UI reading multiple artifacts | ~200 |
| `src/components/seo/SeoAuditFeatureSpec.tsx` | Markdown preview of `seo_feature_spec.md` | ~80 |

**Total new code: ~600 lines. Total new task types: 0. Total new handlers: 0. Total new step kinds: 0.**

---

## Scheduling

The new checks run automatically whenever their parent task runs:

| Trigger | What Runs | New Checks Included |
|---------|-----------|---------------------|
| Weekly scheduled | `content_audit` | temporal_url, page_bloat_proxy, literal_template_variable, title_token_duplication, exact_duplicate |
| Weekly scheduled | `ctr_audit` | template literal variable, missing dynamic title |
| Weekly scheduled | `indexing_diagnostics` | sitemap orphans, missing redirects |
| Post-publish | `content_audit` | All 4 new checks for the published article |
| Post-write | `content_audit` | temporal_url, title_token_duplication (validate new article) |

---

## daystoexpiry.com Coverage

| Finding | Detection Path | Fix Path |
|---------|---------------|----------|
| 7 zombie 404 pages | `indexing_diagnostics` + new `detect_orphaned_slugs()` | `fix_404s` task + feature spec with redirect rules |
| 6 exact duplicate URLs | `content_audit` new `md5_body_hash` check | `consolidate_cluster` task + feature spec with redirects |
| 150 pages with title stuffing | `ctr_audit` `CtrTemplateDetect` + `content_audit` `title_token_duplication` | `fix_ctr_site_template` task + feature spec with template fix |
| Literal `\| Brand \|` in titles | `content_audit` new `literal_template_variable` check | Feature spec with template variable fix |
| Cannibalization clusters | Existing `cannibalization_audit` | Existing `consolidate_cluster` task |
| 8 temporal URLs | `content_audit` new `temporal_url` check | `content_cleanup` task (rewrite slugs) + feature spec (redirects) |
| Page bloat | `content_audit` new `page_bloat_proxy` check | Feature spec with optimization notes |
| 64 sitemap orphans | Existing `indexing_diagnostics` | Feature spec with sitemap fix |

---

## Related Docs

- `AGENT_DEVELOPMENT_PLAYBOOK.md` — "prefer skill, not handler" rule
- `content_audit.rs` — existing 17-check audit to extend
- `ctr_audit/template.rs` — existing template detection to extend
- `cannibalization_audit.rs` — existing clustering pipeline to reuse
- `indexing_diagnostics.rs` — existing sitemap/GSC analysis to reuse
