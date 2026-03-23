# AI Workflow Reference (Short)

Purpose: quick, implementation-level contract for agents working in PageSeeds App.

## Runtime Model

- Entry: task execution starts in executor.
- Planner: workflow handler returns ordered steps.
- Runner: each step kind maps to one executor function.
- Artifacts: write machine-readable JSON to automation dir and attach to task artifacts.
- Completion:
- research tasks end in review.
- most other successful tasks end in done.
- failed tasks reset to todo.

Step kinds used by these workflows:
- deterministic: pure Rust logic.
- agentic: model call with task context.
- normalizer: parse model output into structured JSON.
- manual: no execution, user action required.

## Workflow: research_keywords

Goal: generate new candidate keywords with difficulty and volume.

Path:
1. Handler emits step kind keyword_research_cli.
2. Executor runs native keyword pipeline.
3. Themes source:
- task description themes list when present.
- fallback auto-derived themes from project context.
4. Pipeline:
- fetch keyword ideas (contains volume).
- dedupe against existing articles.json keywords.
- analyze top N keyword difficulty.
- merge into one result payload.
5. Output artifact includes:
- themes
- total_candidates
- new_keywords
- difficulty.results with keyword, difficulty, volume, serp metadata
6. Task status moves to review for user keyword selection.

## Workflow: collect_gsc

Goal: inspect indexability for sitemap URLs and spawn concrete fix tasks.

Path:
1. Handler emits step kind collect_gsc_inspect.
2. Executor resolves site URL + sitemap URL from manifest.
3. Token source order:
- cached UI token when available.
- otherwise service-account auth via env resolver.
4. Run:
- fetch sitemap URLs (supports sitemapindex one level deep).
- inspect URLs via GSC URL Inspection API.
- classify each record into stable reason_code + action + priority.
5. Write automation artifact gsc_collection.json containing:
- meta
- counts by reason_code
- items sorted by ascending priority.
6. Post-step task spawning:
- reason codes map to fix_technical, fix_indexing, or fix_gsc_access.
- skip duplicates and existing open equivalents.
- cap per-run spawned URL tasks.
- if all pages are indexed, create investigate_gsc.

## Workflow: investigate_gsc

Goal: produce structured investigation recommendations from collection output.

Path:
1. Handler emits agentic step with artifact param gsc_collection.json.
2. Agent runner loads artifact content into prompt context.
3. If artifact missing, step fails with collect-first message.
4. Agent returns structured analysis JSON for follow-up work.

## Workflow: content_review (hook only)

On successful content_review/content_audit:
- executor reads recommendations.json.
- if recommendations exist and no active apply task exists, create one content_review_apply task.

## Invariants (Do Not Break)

- Keep business logic in Rust modules, not command wrappers.
- Keep command wrappers thin and typed.
- Keep handler step graphs declarative; execution belongs in executor.
- Keep output artifacts deterministic and machine-parseable.
- Prefer stable reason codes over free-text states for downstream task creation.
