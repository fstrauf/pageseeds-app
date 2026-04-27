# CTR Recovery System Spec

Status: Proposed  
Created: 2026-04-28  
Scope: CTR audit, rendered metadata detection, target-repo fixes, query-grounded snippets, schema, and CTR outcome tracking.

## Summary

The current CTR workflow is a useful article-level repair loop. It can find articles with weak title/meta/snippet/FAQ signals, rank them by estimated lost clicks, create per-article fix tasks, apply structured patches, and verify basic thresholds.

It does not yet fully solve the original CTR problem: low click-through from high-impression pages caused by rendered title template duplication, missing visible rich-result markup, weak query-specific snippet structure, and lack of weekly GSC outcome tracking.

The remaining work is partly in PageSeeds and partly in the target website repository. PageSeeds should detect, prioritize, generate tasks, apply safe content patches, and verify rendered output. The target repo must own site-level title templates, schema rendering, layout components, and any framework-specific SEO behavior.

## Problem

The original diagnosis was not just "some articles need better metadata." It was:

- High impressions and low clicks: about 252K impressions and 118 clicks across the top pages.
- CTR around 0.10% while average positions around 5-8 should produce materially higher CTR.
- Rendered title tags repeat the brand/template and truncate the useful part.
- FAQ/schema and snippet formatting are not taking up SERP real estate.
- Query-specific opportunities are known in GSC but are not being used to shape answers.
- Progress needs to be measured weekly in GSC, not only by local file health.

The current implementation covers part of that, but it mostly operates on local article files. The biggest missing category is rendered-site behavior.

## Ownership Model

| Area | Owner | Reason |
|---|---|---|
| Detect low-CTR pages from GSC | PageSeeds | Data collection and prioritization are app workflows. |
| Read local MDX frontmatter/body | PageSeeds | Content-file parsing and patching are already implemented in Rust. |
| Rendered `<title>` template | Target repo | Usually lives in Next/Astro/React metadata/layout code, not in article frontmatter. |
| Rendered JSON-LD schema | Target repo + PageSeeds verification | The repo must render it; PageSeeds should verify that Google-visible output contains it. |
| Snippet-bait prose | PageSeeds | Article content can be patched safely from structured agent output. |
| Snippet markup patterns (`<ol>`, tables, H2 question headings) | PageSeeds + target repo | Content patches can add markup; framework components may be repo-specific. |
| Hub/internal link architecture | Separate PageSeeds workflows | Related to CTR, but broader than CTR audit. |
| Backlink/calculator outreach | Separate growth workflows | Not a CTR audit fix; should be tracked separately. |
| Outcome tracking | PageSeeds | GSC before/after reporting belongs in the app. |

## Current Implementation Baseline

Already implemented or mostly implemented:

- `ctr_audit` syncs page-level GSC data before building context.
- `ctr_build_context` reads article metadata, extracts title/meta/first paragraph/FAQ state, computes `clicks_lost`, and sends the top 20 problematic pages to the agent.
- `ctr_analyze` loads the CTR optimization skill and returns JSON recommendations.
- `fix_ctr_article` creates one task per article and uses a structured patch contract.
- Rust applies patches deterministically with safe top-level frontmatter scalar replacement.
- Verification checks title length, meta description length, snippet word count, keyword/question presence, and FAQ presence.
- Healthy unchanged articles are skipped on later audits.
- Failed per-article fixes land in review instead of being blindly requeued.
- A CTR health panel exists for local article health counts.

Known gaps in the baseline:

- Rendered site title tags are not compared against frontmatter titles.
- Site-wide title-template duplication is not detected or fixed.
- Query-level GSC data is not included in CTR context.
- Agent recommendations may rely on optional `file` and `target_keyword` fields that should be guaranteed by Rust.
- Snippet verification does not check H2 question headings, ordered lists, tables, or exact query match.
- FAQ detection can accept markdown/frontmatter FAQ even when the rendered page may not emit JSON-LD.
- Health summary fields for improvements/regressions/last audit are still shallow.
- Outcome tracking does not measure weekly CTR/click deltas or featured snippet count.

## Goals

- Make PageSeeds distinguish content-file issues from target-repo rendering/template issues.
- Detect rendered title duplication and create a repo-level fix plan for the target site.
- Ground FAQ and snippet recommendations in actual page-query GSC data.
- Apply article-level fixes only through structured patches and deterministic writers.
- Verify both source-file health and rendered-page health.
- Track whether CTR, clicks, and rich-result eligibility improve after deployment.
- Keep the workflow simple enough that failed fixes move to review with clear reasons.

## Non-Goals

- Fully automatic framework-specific rewrites for every possible target repo.
- Guaranteeing Google will show FAQ snippets or featured snippets.
- Combining backlink outreach, HARO, guest posts, and calculator promotion into CTR audit.
- Replacing the separate cannibalization, hub, internal-linking, or backlink workflows.
- Making PageSeeds responsible for production deployment.

## Desired Workflow

```text
ctr_recovery_audit
  -> ctr_gsc_sync_pages             deterministic
  -> ctr_gsc_sync_queries           deterministic
  -> ctr_rendered_serp_audit        deterministic
  -> ctr_build_context              deterministic
  -> ctr_analyze                    agentic
  -> ctr_normalize_recommendations  deterministic
  -> ctr_create_fix_tasks           deterministic

fix_ctr_article
  -> fix_ctr_article_generate       agentic
  -> fix_ctr_article_apply          deterministic
  -> fix_ctr_article_verify_source  deterministic
  -> fix_ctr_article_verify_render  deterministic, optional until preview/live URL is available

fix_ctr_site_template
  -> ctr_template_detect            deterministic
  -> ctr_template_plan              agentic/manual-review
  -> ctr_template_apply             agentic or manual depending on repo framework
  -> ctr_template_verify_render     deterministic

ctr_outcome_review
  -> ctr_fetch_after_metrics        deterministic
  -> ctr_compare_periods            deterministic
  -> ctr_report_outcomes            deterministic
```

## Feature 1: Rendered SERP Audit

### Problem

The current audit reads source frontmatter. The original title issue is likely rendered by the target site template. If the rendered HTML adds `| Days to Expiry | Days to Expiry — Option Selling Analyzer`, local frontmatter edits alone will not fix the SERP title.

### Requirements

- Fetch or render each target URL and extract:
  - rendered `<title>`
  - rendered meta description
  - canonical URL
  - H1
  - JSON-LD blocks and schema types
  - first visible answer block after H1 or target H2
  - presence of ordered lists/tables near snippet target sections
- Compare rendered values with source-file values.
- Classify issue source:
  - `content_file`: source title/meta/body is weak.
  - `site_template`: rendered title/meta/schema differs because of layout or framework logic.
  - `missing_rendered_schema`: source has FAQ data but rendered HTML has no FAQPage JSON-LD.
  - `unknown`: cannot resolve URL or rendering failed.

### Output Contract

```json
{
  "pages": [
    {
      "article_id": 42,
      "url": "https://example.com/blog/best-stocks-csp",
      "file": "content/042_best_stocks_csp.mdx",
      "source_title": "Best Cash-Secured Put Stocks (2026)",
      "rendered_title": "Best Cash-Secured Put Stocks (2026) | Days to Expiry | Days to Expiry — Option Selling Analyzer",
      "rendered_title_length": 103,
      "title_issue_source": "site_template",
      "rendered_meta_description": "...",
      "schema_types": ["Article"],
      "has_rendered_faq_page": false,
      "snippet_markup": {
        "has_question_h2": false,
        "has_ordered_list": false,
        "has_table": false
      },
      "issues": ["rendered_title_too_long", "brand_duplicate", "missing_rendered_faq_page"]
    }
  ]
}
```

### Acceptance Criteria

- PageSeeds can show when the local frontmatter title is fine but rendered HTML is broken.
- Rendered duplicate-brand titles are never classified as solved by local article rewrites alone.
- Missing rendered JSON-LD is surfaced even if markdown FAQ headings exist.

## Feature 2: Site Title Template Fix Task

### Problem

The most important title issue belongs in the target repo: metadata/layout code creates the final `<title>`. PageSeeds needs a task that identifies this and guides/fixes the target repo, not another article rewrite.

### Requirements

- Add a task type: `fix_ctr_site_template`.
- Trigger it when `ctr_rendered_serp_audit` detects a repeated pattern across multiple pages.
- Detect likely framework files, but keep this conservative:
  - Next.js: `app/layout.*`, `app/**/page.*`, `generateMetadata`, `metadata`, `_document`, `_app`.
  - Astro: `src/layouts/**`, `src/pages/**`, `Astro.props`, `SEO` components.
  - Gatsby/React: `Helmet`, `Seo`, `Layout`, `gatsby-config` metadata.
  - Generic: search for repeated brand suffix strings.
- Produce a reviewable plan with file candidates, current pattern, desired pattern, and validation command.
- Desired default title format:
  - Article pages: `{Article Title} | {Brand}`
  - Home page: `{Brand} — {Primary Product/Offer}` or project-configured equivalent
  - No duplicate brand suffix.
- Verification must render/fetch sample pages and confirm the duplicate suffix is gone.

### Output Contract

```json
{
  "detected_pattern": "{title} | Days to Expiry | Days to Expiry — Option Selling Analyzer",
  "desired_pattern": "{title} | Days to Expiry",
  "affected_pages": 37,
  "candidate_files": ["src/components/SEO.tsx", "src/app/layout.tsx"],
  "confidence": "high",
  "requires_manual_review": true,
  "verification_urls": ["/blog/best-stocks-csp", "/blog/theta-decay-dte-guide"]
}
```

### Acceptance Criteria

- A site-wide duplicated title pattern creates one site-template task, not 37 article tasks.
- The task cannot mark itself done until rendered sample pages pass title checks.
- If framework detection is uncertain, the task goes to review with exact evidence.

## Feature 3: Query-Level GSC Context

### Problem

The original FAQ and snippet recommendations are query-specific. Current context has page-level metrics only, so the agent cannot reliably know which questions actually produce impressions.

### Requirements

- For the top CTR candidates, fetch top GSC queries per page.
- Store query metrics with:
  - query
  - impressions
  - clicks
  - CTR
  - average position
  - detected intent: question, comparison, best/list, tax/legal, calculator/tool, generic
- Include top queries in CTR context.
- Use query terms to generate FAQ questions and snippet answer targets.
- Prefer high-impression, position 2-10 question/comparison queries for snippet-bait tasks.

### Target CTR Model

Replace the fixed `0.5%` target with a conservative position-aware curve.

Example default curve:

| Position Range | Target CTR |
|---|---:|
| 1-2 | 8.0% |
| 3-4 | 4.0% |
| 5-7 | 1.5% |
| 8-10 | 0.8% |
| 11-20 | 0.3% |

Use `clicks_lost = impressions * max(0, target_ctr_for_position - actual_ctr)`.

### Acceptance Criteria

- Top page recommendations cite the specific query driving the fix.
- FAQ questions come from or closely map to actual queries.
- Lost-click estimates are closer to the original business case than the current 0.5% ceiling.

## Feature 4: Stronger CTR Recommendation Contract

### Problem

The recommendation model allows `file` and `target_keyword` to be optional. The fix task requires both. This lets a syntactically valid agent response create a broken follow-up task.

### Requirements

- Rust must enrich recommendations with canonical article fields after agent output:
  - `article_id`
  - `url_slug`
  - `file`
  - `target_keyword`
  - source/rendered health facts
  - top queries
- Agent output should identify the article and recommended fixes; Rust should fill trusted repo fields from context.
- Reject recommendations for unknown article IDs.
- Reject or review recommendations with no applicable fix.
- Keep one `fix_ctr_article` task per article.

### Acceptance Criteria

- A valid recommendation can never produce a fix task with an empty file path.
- Agent output is schema-validated before task creation.
- The per-article artifact contains all data needed by `ctr-fix-apply` without asking the agent to infer file paths.

## Feature 5: Featured Snippet Patch Types

### Problem

The current snippet fix only replaces the first paragraph. The original plan requires different formats for different query types.

### Requirements

Extend the patch model beyond `first_paragraph`:

```json
{
  "snippet_patch": {
    "target_query": "cash secured put vs naked put",
    "format": "comparison_paragraph",
    "heading": "Cash-Secured Put vs Naked Put: What's the Difference?",
    "answer_paragraph": "A cash-secured put requires...",
    "ordered_list": null,
    "comparison_table": null
  }
}
```

Supported formats:

- `direct_answer_paragraph`
- `comparison_paragraph`
- `best_list_ordered`
- `comparison_table`
- `definition_with_steps`

Deterministic apply rules:

- Insert or replace a target H2 near the top of the article.
- Insert a 40-60 word direct answer below the H2.
- For `best_list_ordered`, insert an `<ol>` or markdown ordered list.
- For `comparison_table`, insert a markdown table or approved MDX table component.
- Keep modifications limited to the target intro/snippet section.

Verification rules:

- H2 contains the target query or close variant.
- Direct answer is 40-60 words.
- Required structure exists for the selected format.
- The section appears before the first deep content section where possible.

### Acceptance Criteria

- "best X" queries can create ordered-list snippet targets.
- "X vs Y" queries can create comparison paragraph/table targets.
- Verification fails with a clear reason if the structure was not created.

## Feature 6: Rendered Schema Verification

### Problem

Local markdown/frontmatter FAQ does not guarantee Google sees FAQPage JSON-LD.

### Requirements

- Separate source FAQ health from rendered schema health:
  - `source_has_faq_content`
  - `source_has_faq_frontmatter`
  - `rendered_has_faq_page_json_ld`
  - `rendered_faq_question_count`
- Article is rich-result-ready only if rendered JSON-LD includes `FAQPage` with 3-5 questions.
- If source has FAQ but rendered JSON-LD is missing, create a site-template/schema rendering task instead of adding duplicate article FAQ content.

### Acceptance Criteria

- PageSeeds does not mark FAQ rich-result health as passing unless rendered JSON-LD exists or the target repo explicitly configures a schema renderer mapping that can be validated.
- The workflow can distinguish "write FAQ content" from "fix schema renderer."

## Feature 7: CTR Outcome Tracking

### Problem

Current health checks prove local rules pass. They do not prove clicks improved.

### Requirements

- Store baseline metrics when recommendations are created:
  - period start/end
  - impressions
  - clicks
  - CTR
  - average position
  - query-level metrics for top queries
- Store deployment/verification timestamp when fixes pass.
- Add outcome review task that compares before/after periods.
- Report:
  - daily clicks before/after
  - overall CTR before/after
  - per-page CTR delta
  - per-query CTR delta
  - impressions-weighted click gain
  - pages with no improvement
  - pages that regressed
- Do not judge outcomes until enough post-change data exists, default 14 days.

### Acceptance Criteria

- The UI can answer whether the CTR sprint moved from ~7 clicks/day toward ~20 clicks/day.
- Regressions create review tasks instead of being hidden by local health checks.
- Reports distinguish ranking/impression changes from true CTR changes.

## Feature 8: Health Summary Backed By Audit State

### Problem

The health summary currently recomputes local health but does not use the stored audit lifecycle deeply enough.

### Requirements

- Populate:
  - `last_audit_at`
  - `last_audited_at`
  - `last_audit_issues`
  - `resolved_issues`
  - `improved_count`
  - `regressed_count`
- Include source vs rendered issue categories.
- Track pending, failed, review, verified counts for CTR fix tasks.

### Acceptance Criteria

- The CTR panel shows actual workflow state, not only current local health.
- A user can tell which pages improved, which still need work, and which are blocked by target-repo template issues.

## Feature 9: Target Repo Readiness Checks

### Problem

Many fixes depend on whether PageSeeds can run or inspect the target site.

### Requirements

- Detect available target repo commands:
  - install command
  - typecheck/build command
  - dev/preview command
  - static export output, if any
- Prefer existing package scripts.
- If the site can be built/served locally, verify rendered metadata against local preview URLs before asking the user to deploy.
- If local rendering is not available, fall back to live URL checks and mark fixes as requiring deployment verification.

### Acceptance Criteria

- PageSeeds can explain why rendered checks are unavailable.
- Site-template fixes include a concrete verification path.

## Feature 10: Sprint Plan And UI

### Requirements

Add a CTR Recovery view with four sections:

1. **Smoking Gun Summary**
   - Top pages by lost clicks.
   - Current impressions/clicks/CTR/position.
   - Estimated clicks lost using position-aware target CTR.

2. **Root Cause Breakdown**
   - Rendered title/template issues.
   - Missing rendered schema.
   - Snippet format gaps.
   - Meta description issues.
   - Source-file vs target-repo issue counts.

3. **Action Plan**
   - Site-template tasks.
   - Per-article fix tasks.
   - Schema-rendering tasks.
   - Snippet-format tasks.
   - Outcome-review tasks.

4. **Outcome Tracking**
   - 7/14/30/90-day CTR and clicks.
   - Pages improved/regressed.
   - Query-level winners/losers.

### Acceptance Criteria

- The user sees that the site-template issue must be fixed in the target repo.
- The user can run the article patch backlog separately from rendered-template tasks.
- The user can track whether the 30-day sprint target was reached.

## Data Model Additions

### `ctr_rendered_page_audits`

Stores rendered HTML observations per page and audit run.

Important fields:

- `project_id`
- `article_id`
- `url`
- `file`
- `source_title`
- `rendered_title`
- `source_description`
- `rendered_description`
- `schema_types_json`
- `has_rendered_faq_page`
- `snippet_markup_json`
- `issues_json`
- `checked_at`

### `ctr_query_metrics`

Stores query-level GSC rows for CTR candidates.

Important fields:

- `project_id`
- `article_id`
- `page_url`
- `query`
- `impressions`
- `clicks`
- `ctr`
- `avg_position`
- `period_start`
- `period_end`
- `intent`

### `ctr_outcomes`

Stores before/after measurements.

Important fields:

- `project_id`
- `article_id`
- `fix_task_id`
- `baseline_start`
- `baseline_end`
- `after_start`
- `after_end`
- `baseline_clicks`
- `after_clicks`
- `baseline_ctr`
- `after_ctr`
- `position_delta`
- `outcome_status`

## Required Tests

- Rendered title duplication is detected when source title is clean.
- Rendered title duplication creates a site-template task, not article rewrite tasks.
- A duplicated title template across multiple pages produces one grouped task.
- Query-level GSC rows are attached to the CTR context for top pages.
- Recommendations missing file/target keyword are enriched or rejected before spawning fix tasks.
- FAQ markdown/frontmatter does not pass rendered FAQPage verification unless JSON-LD exists.
- Snippet format verification fails if a `best_list_ordered` patch creates no list.
- CTR audit skip logic includes FAQ/schema state in the content/rendered hash.
- Outcome review waits for the configured post-change window.
- Health summary displays improved/regressed/resolved issue counts from stored state.

## Rollout Plan

### Phase 1: Close Current Contract Gaps

- Make `file` and `target_keyword` guaranteed in recommendation artifacts.
- Include FAQ/schema state in the audit hash.
- Populate health summary lifecycle fields from audit state.
- Align `ctr-optimization` and `ctr-fix-apply` contracts with actual Rust thresholds.

### Phase 2: Add Query-Grounded CTR Context

- Fetch page-query GSC rows for top candidates.
- Add intent classification for query rows.
- Replace fixed 0.5% lost-click target with position-aware target CTR.
- Update skills to use top queries for FAQ/snippet fixes.

### Phase 3: Add Rendered SERP Audit

- Fetch/render target pages.
- Compare source vs rendered title/meta/schema/snippet markup.
- Classify issue source.
- Add rendered audit artifacts and UI counts.

### Phase 4: Add Target-Repo Template Tasks

- Add `fix_ctr_site_template` task type.
- Detect repeated title suffix patterns.
- Generate framework-aware review plans.
- Verify rendered sample pages after fix.

### Phase 5: Upgrade Snippet And Schema Fixes

- Add structured snippet patch types.
- Add rendered FAQPage verification.
- Distinguish FAQ content tasks from schema-renderer tasks.

### Phase 6: Outcome Tracking

- Store baseline metrics at recommendation time.
- Add after-period GSC comparison.
- Add CTR recovery report and UI.

## Success Criteria

- PageSeeds can explain whether each CTR issue belongs to source content or target repo rendering code.
- A duplicated rendered title template is detected and fixed once at the repo-template level.
- Top CTR recommendations use real page-query data.
- Per-article fixes can never be spawned without a valid file path and target keyword.
- FAQ health means rendered JSON-LD exists, not merely that an FAQ heading exists.
- Featured snippet fixes can create and verify query-specific paragraphs, lists, and tables.
- The UI shows whether clicks and CTR improved after deployment.
- The workflow supports the original 30-day sprint: title template fix, top-page title/meta rewrites, FAQ/schema rollout, snippet-bait sections, and weekly GSC tracking.