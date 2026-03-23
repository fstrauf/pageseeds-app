# GSC Workflows — AI Runbook

Purpose: define how GSC workflows must behave in PageSeeds App (Rust), matching CLI business behavior.

## Scope

This runbook covers three task families:
- `collect_gsc` (indexing diagnosis and task spawning)
- `investigate_gsc` (agentic analysis fallback)
- `content_review` step `gsc_sync_articles` (analytics sync only)

## Core Rule

Do not mix workflows:
- `collect_gsc` is about URL Inspection and indexing root causes.
- `gsc_sync_articles` is about clicks/impressions/CTR sync.

If these are conflated, task outcomes become wrong.

## Workflow 1: collect_gsc

Input:
- project `manifest.json`
- GSC auth token (from in-memory state if valid, else service account token)

Execution:
1. Resolve `site_url` from `manifest.gsc_site` or `manifest.url`.
2. Resolve sitemap URL from `manifest.sitemap` or `{site_url}/sitemap.xml`.
3. Fetch sitemap URLs (cap 200; support sitemap index one level deep).
4. Call URL Inspection API for each URL.
5. Classify each record into a stable `reason_code`.
6. Assign priority (`lower = more urgent`).
7. Write `automation/gsc_collection.json`.
8. Spawn implementation tasks from collection results.

Output artifact:
- `automation/gsc_collection.json`

Post-conditions:
- Success with issues: create fix tasks.
- Success with all indexed: create one `investigate_gsc` task.
- Failure: task resets to `todo` with error.

## Workflow 2: investigate_gsc

Trigger:
- Usually spawned by `collect_gsc` when all URLs are `indexed_pass`.

Execution:
1. Read `automation/gsc_collection.json`.
2. Run agentic investigation step with collection JSON in prompt context.
3. Return structured JSON findings in step output.

Hard requirement:
- If `gsc_collection.json` is missing, fail with explicit message to run `collect_gsc` first.

## Workflow 3: gsc_sync_articles (content_review)

Purpose:
- Sync Search Analytics metrics into `articles.json`.

Execution:
1. Resolve auth token (reuse in-memory token when available).
2. Fetch page rows for configured date window.
3. Match URLs to articles via normalized path/slug fallback.
4. Write `gsc` block per matched article.

Non-goal:
- This step does not classify indexing issues and does not create fix tasks.

## Reason Codes Contract

`collect_gsc` must use these codes:
- `robots_blocked`
- `noindex`
- `fetch_error`
- `canonical_mismatch`
- `not_indexed_crawled`
- `not_indexed_discovered`
- `not_indexed_other`
- `indexed_pass`
- `api_error` (when inspection fails for specific URLs)

Priority contract:
- `robots_blocked` / `noindex` / `fetch_error` -> 10
- `canonical_mismatch` -> 20
- `api_error` -> 30
- `not_indexed_crawled` -> 40
- `not_indexed_discovered` -> 50
- `not_indexed_other` -> 70
- `indexed_pass` -> 999

## Task Spawning Contract

From `gsc_collection.json` (up to top 20 actionable items):
- `robots_blocked`, `noindex`, `fetch_error`, `canonical_mismatch` -> `fix_technical`
- `not_indexed_crawled`, `not_indexed_discovered`, `not_indexed_other` -> `fix_indexing`
- grouped `api_error` -> one `fix_gsc_access`
- `indexed_pass` -> no fix task

Dedup rules:
- Skip duplicate `reason_code:url` pairs.
- Skip if similar open task already exists (`todo` or `in_progress`).

Required task fields:
- `phase = implementation`
- `execution_mode = manual`
- `title` contains reason + URL slug
- `description` includes URL, issue, action, verdict

## Data Contract: gsc_collection.json

Required top-level keys:
- `meta`
- `counts`
- `items`

`meta` minimum:
- `site_url`
- `sitemap_url`
- `collected_at`
- `total_urls`
- `issues_found`

`items[]` minimum:
- `url`
- `verdict`
- `coverage_state`
- `reason_code`
- `action`
- `priority`

Sorting:
- `items` sorted ascending by `priority`.

## Safety Guards

- Domain guard: if inspected URLs mostly do not match project `site_url`, fail fast.
- Auth guard: if no valid token can be resolved, fail with setup guidance.
- Artifact guard: `investigate_gsc` must not run without `gsc_collection.json`.

## Minimal Verification Checklist

- Running `collect_gsc` creates `gsc_collection.json`.
- Non-indexed pages create fix tasks.
- All indexed pages create `investigate_gsc` task.
- `content_review` still performs analytics sync only.
- No Python CLI subprocess is required for these GSC flows.
