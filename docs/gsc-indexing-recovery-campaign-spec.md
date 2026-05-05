# GSC Indexing Recovery Campaign Spec

Status: Proposed
Created: 2026-05-05
Scope: Search Console Drift, sitemap/GSC freshness, internal link recovery, task orchestration, verification, and outcome tracking.

## Decision Summary

Build a backend-owned indexing recovery campaign instead of making the Drift tab create link tasks directly.

The first implementation should add a parent `gsc_indexing_recovery` task and a child `fix_indexing_internal_links` task. The parent refreshes stale GSC/link data, computes Drift, writes a structured target plan, and backend post-actions spawn one child task per eligible URL. Each child task receives a target artifact, uses a deterministic source shortlist, asks Rig for a structured link plan, applies conservative Related Articles links, and verifies that the target gained inbound links.

## Summary

The current Search Console Drift tab identifies useful indexing signals, but the fix path is too blunt. The UI computes a drift report, counts priority URLs with no inbound internal links, and the "Fix links" button creates one generic `cluster_and_link` task. That task is site-wide, capped at 20 link recommendations, and does not use the Drift candidate list as its target set. For a site with 56 orphan or zero-incoming URLs, one task cannot reliably produce 56 targeted fixes.

The clean solution is a backend-owned `gsc_indexing_recovery` campaign. A user starts the campaign from Drift. The backend refreshes stale GSC/link data, computes a deterministic target plan, then creates one focused link-fix task per eligible target URL. The queue runs those tasks serially and each task verifies that the target article gained at least one inbound internal link.

## Current Implementation Baseline

Already implemented or mostly implemented:

- `GscDrift` renders the Search Console Drift tab and calls `gsc_compute_drift(projectId)`.
- `gsc_compute_drift` compares live sitemap URLs against cached `gsc_collection.json`, cached/fresh `link_scan.json`, and article metadata.
- The Drift report surfaces `indexed`, `not_indexed`, `in_sitemap_not_in_gsc`, `in_gsc_not_in_sitemap`, `resubmit_priority`, `gsc_data_age_hours`, and `link_scan_age_hours`.
- `collect_gsc` fetches sitemap URLs, calls the URL Inspection API, writes `gsc_collection.json`, and syncs page-level GSC analytics into article data.
- `collect_gsc` post-actions can spawn GSC fix tasks from `gsc_collection.json`.
- `indexing_diagnostics` stores per-URL inspection history in SQLite and can spawn fix tasks for new or unresolved indexing issues.
- `content::linking::scan_links()` builds `orphan_ids`, `zero_incoming_ids`, incoming/outgoing link profiles, and unresolved link data.
- `cluster_and_link` scans links, asks an agent for up to 20 recommended links, applies Related Articles links, and rescans the graph.
- The backend queue persists queue runs/items, executes tasks serially, and auto-enqueues follow-up tasks whose `run_policy` is `auto_enqueue`.

Known gaps:

- The Drift tab creates one generic `cluster_and_link` task from the frontend instead of using a backend lifecycle path.
- The generic `cluster_and_link` task is not targeted to Drift candidates and is capped at 20 recommendations.
- Existing GSC-generated `interlinking` tasks currently route to the same generic `cluster_and_link` workflow, so one URL-specific issue does not get a URL-specific link repair.
- Freshness is advisory in the Drift UI. If GSC data is stale, clicking "Fix links" does not first refresh it.
- `collect_gsc` currently fetches only up to 200 sitemap URLs, while Drift fetches up to 5000 sitemap entries. That mismatch can make recovery planning incomplete on larger sites.
- Child fix tasks do not carry structured artifacts such as target article ID, target URL, reason code, source candidates, or baseline incoming count.
- Immediate success is not measured per target. A task can finish after adding links somewhere without proving that the intended target URL gained an inbound link.
- Outcome tracking is missing. The app does not schedule a later GSC re-check to see whether linking helped indexing.

## Current Flow Trace

The current code path is useful for diagnosis but not sufficient for repair:

```text
Search Console tab
  -> src/components/gsc/GSC.tsx renders <GscDrift projectId={projectId} />

Drift display
  -> src/components/gsc/GscDrift.tsx calls gscComputeDrift(projectId)
  -> src-tauri/src/commands/gsc.rs exposes gsc_compute_drift
  -> src-tauri/src/engine/exec/gsc/drift.rs fetches sitemap entries, loads gsc_collection.json,
     loads or refreshes link_scan.json, then builds GscDriftReport

Current repair button
  -> GscDrift.handleCreateLinkTask calls createTask(projectId, "cluster_and_link", ...)
  -> task is manually enqueued through useQueueStore().enqueue(...)
  -> cluster_and_link runs a site-wide scan/strategy/apply workflow
```

Why this fails the product intent:

- The button has no target contract. It does not pass Drift candidates, URLs, article IDs, reason codes, or incoming-link baselines into the task.
- `cluster_and_link` asks for the top 20 links across the site, so it is structurally unable to guarantee 56 target fixes.
- `interlinking` already exists as a GSC follow-up type, but it currently routes to the same generic workflow and has the same targeting problem.
- The frontend uses `createTask` directly. That bypasses the stronger backend patterns in `TaskSpawner`, task artifacts, post-actions, idempotency keys, and auto-enqueued follow-ups.
- Freshness is only displayed. It is not enforced before repair planning.

The clean boundary is: Drift renders and starts one campaign task; the backend plans and spawns all repair work.

## Problem

The product promise of the Drift tab is: "These pages are missing from Google's index or not healthy in GSC; let's do the most likely fixes and improve indexing odds."

The current implementation actually does: "Create one broad internal-link task and hope it touches some of the right pages."

That breaks down when there are many candidates because the agent receives a site-wide link graph, not a target contract. It may pick only the top 20 opportunities, miss most orphan URLs, or add links that improve the graph generally without fixing the specific pages the user was looking at.

## Goals

- Make "Fix links" create and run a full indexing recovery campaign, not a single generic task.
- Always use fresh enough GSC and link data before planning fixes.
- Create one focused task per eligible target URL, or a small explicit batch when configured.
- Pass structured target context to each task so the agent does not rediscover the problem from prose.
- Prefer deterministic filtering, scoring, and verification before agentic judgment.
- Use Rig structured extraction for agentic link-placement decisions where possible.
- Keep task creation and queueing backend-owned and consistent with the task lifecycle contract.
- Verify immediately that each target gained inbound internal links.
- Track delayed GSC outcomes so the workflow can learn whether fixes worked.

## Non-Goals

- Guaranteeing Google will index every fixed page.
- Automatically submitting URLs to Google. The URL Inspection API supports inspection, not reliable bulk submission.
- Solving robots, noindex, canonical, fetch, or sitemap-membership problems with internal links.
- Replacing `cluster_and_link` as the general site-wide internal linking workflow.
- Building a large one-prompt site strategy for all recovery targets.
- Running multiple file-editing tasks in parallel in V1.

## Lifecycle Contract

| Lane | Decision |
|---|---|
| User starts work | Drift creates one `gsc_indexing_recovery` task and enqueues it through the backend queue. |
| System creates downstream work | `gsc_indexing_recovery` creates follow-up tasks after successful planning. |
| Follow-up creation owner | `engine/post_actions.rs` calls a GSC recovery domain helper that uses `TaskSpawner::spawn`. |
| Queue behavior | Follow-ups use `run_policy = auto_enqueue`, so the backend queue appends and runs them. |
| Review behavior | Default campaign mode is automatic and uses `review_surface = none`. A later preview mode can use `artifact_review`. |
| Idempotency | Campaign and child tasks use deterministic idempotency keys scoped by project, URL, reason, and campaign date. |

## Proposed Task Types

### `gsc_indexing_recovery`

Purpose: user-started campaign parent that refreshes inputs, computes Drift, builds a target plan, and spawns focused follow-up tasks.

Task definition:

```text
task_type: gsc_indexing_recovery
phase: implementation
run_policy: user_enqueue
review_surface: none
follow_up_policy: backend_auto
handler_family: implementation
agent_policy: none
```

Workflow:

```text
gsc_indexing_recovery
  -> gsc_recovery_prepare          deterministic
  -> gsc_recovery_drift            deterministic
  -> gsc_recovery_plan             deterministic
```

Execution mode notes:

- `gsc_recovery_prepare` can use the existing Tokio pattern for async HTTP inside backend workflow execution. It should call extracted shared helpers from `collect.rs` rather than reimplementing token, sitemap, and URL Inspection logic.
- `gsc_recovery_drift` should reuse the existing Drift computation and should not perform hidden repair work.
- `gsc_recovery_plan` is deterministic because it filters, scores, and serializes known candidates. It should not ask an LLM to decide which URLs are eligible.

Post-task action:

```text
gsc_indexing_recovery success
  -> read gsc_recovery_plan artifact
  -> spawn fix_indexing_internal_links tasks
  -> return created IDs as follow-ups
  -> backend queue auto-enqueues them
```

### `fix_indexing_internal_links`

Purpose: one target URL/article gets one or more new inbound links from strong, relevant source pages.

Task definition:

```text
task_type: fix_indexing_internal_links
phase: implementation
run_policy: auto_enqueue
review_surface: none
follow_up_policy: backend_auto
handler_family: implementation
agent_policy: required
```

Workflow:

```text
fix_indexing_internal_links
  -> indexing_link_context         deterministic
  -> indexing_link_plan            agentic, structured Rig extraction
  -> indexing_link_apply           deterministic
  -> indexing_link_verify          deterministic
```

Execution mode notes:

- `indexing_link_context` reads the target artifact, current link scan, article metadata, and source files. It produces a compact per-target context so the model never needs the full site graph.
- `indexing_link_plan` is agentic because choosing a relevant source and anchor requires topical judgment. It should use a typed Rust struct with `serde` and `schemars::JsonSchema` and Rig structured extraction.
- `indexing_link_apply` should support `related_section` first. Contextual paragraph insertion can wait until there is deterministic validation and rollback.
- `indexing_link_verify` is deterministic and should fail the task or move it to review when the target did not gain an inbound link.

Optional follow-up:

```text
fix_indexing_internal_links success
  -> spawn gsc_indexing_outcome_review with user_enqueue or scheduler-controlled timing
```

### `gsc_indexing_outcome_review`

Purpose: delayed verification that the target URL improved in GSC after links were added and deployed.

Task definition:

```text
task_type: gsc_indexing_outcome_review
phase: verification
run_policy: user_enqueue
review_surface: artifact_review
follow_up_policy: none
handler_family: implementation
agent_policy: none
```

V1 can create this as a reviewable task with a `not_before` artifact. A later scheduler can auto-enqueue it after 7-14 days.

## Data Freshness Rules

The recovery campaign must not plan from stale data.

Freshness sources:

- GSC inspection data: prefer `gsc_collection.json.meta.collected_at`; fallback to file modified time.
- Link graph data: use `link_scan.json` file modified time.
- Sitemap data: fetch live during the campaign.
- Article metrics: use analytics synced by the same collection pass when GSC is refreshed.

Defaults:

```json
{
  "max_gsc_age_hours": 24,
  "max_link_scan_age_hours": 24,
  "min_inbound_links_after_fix": 1,
  "max_targets": null,
  "target_reason_codes": ["not_indexed_other", "not_indexed_discovered", "not_indexed_crawled", "not_in_gsc"]
}
```

If GSC data is stale or missing, `gsc_recovery_prepare` runs the same underlying collection logic used by `collect_gsc`. This should be implemented by extracting shared collection functions from `engine/exec/gsc/collect.rs`, not by duplicating URL Inspection API code.

If link data is stale or missing, `gsc_recovery_prepare` runs `content::linking::scan_links()` and writes `link_scan.json`.

The collection limit must be unified. Drift currently fetches up to 5000 sitemap entries while `collect_gsc` fetches 200 sitemap URLs. Recovery should use one configurable limit and report when inspection is partial because of API quota or a user cap.

## Target Eligibility

A Drift candidate is eligible for internal-link recovery when all are true:

- URL is in the sitemap, or the issue is `not_in_gsc` from `in_sitemap_not_in_gsc`.
- A matching MDX content file exists.
- Reason code is not a technical blocker.
- Existing incoming internal link count is below threshold, default `< 1`.
- No active recovery task already exists for the same project, URL, and reason.

Technical blockers that should not create link tasks:

```text
robots_blocked
noindex
fetch_error
canonical_mismatch
api_error
```

Those should continue to create `fix_technical`, `fix_gsc_access`, or `fix_indexing` tasks through existing GSC paths.

Ranking signals:

- zero incoming links
- URL in sitemap but absent from GSC
- not indexed reason severity
- older published date
- prior GSC impressions
- target keyword present
- content file exists and passes basic frontmatter checks
- source pages available with traffic or impressions

## Source Candidate Selection

Each target task should receive a deterministic shortlist of source pages before the agent runs.

Source pages should be:

- indexed or known-good in the latest GSC data when possible
- not already linking to the target
- not the target itself
- thematically related by title, target keyword, slug, headings, or future embeddings
- strong enough to help discovery, using impressions/clicks/position where available
- safe to edit as local MDX files

V1 deterministic scoring can use lightweight signals already available in the app:

```text
source_score = topical_overlap + gsc_impressions_bonus + indexed_bonus + hub_like_bonus - already_links_penalty
```

The agentic step should choose from the shortlist, not from the entire site.

## Artifact Contracts

### `gsc_recovery_plan`

Written by `gsc_recovery_plan` and consumed by post-actions.

```json
{
  "generated_at": "2026-05-05T12:00:00Z",
  "project_id": "project-123",
  "data_freshness": {
    "gsc_collected_at": "2026-05-05T11:55:00Z",
    "gsc_data_age_hours": 0,
    "link_scan_age_hours": 0,
    "sitemap_fetched_at": "2026-05-05T11:56:00Z",
    "partial_gsc_collection": false
  },
  "summary": {
    "sitemap_total": 356,
    "gsc_total": 356,
    "eligible_targets": 56,
    "skipped_targets": 12
  },
  "targets": [
    {
      "url": "https://example.com/blog/example-page",
      "slug": "example-page",
      "article_id": 42,
      "file": "042_example_page.mdx",
      "reason_code": "not_in_gsc",
      "priority_score": 135,
      "priority_reason": "in sitemap but never inspected by GSC, zero internal incoming links",
      "incoming_link_count_before": 0,
      "target_keyword": "example keyword",
      "published_date": "2026-03-01",
      "source_candidates": [
        {
          "article_id": 7,
          "file": "007_related_topic.mdx",
          "title": "Related Topic",
          "slug": "related-topic",
          "score": 91,
          "gsc_impressions": 1240,
          "reason": "keyword overlap and indexed source page"
        }
      ]
    }
  ],
  "skipped": [
    {
      "url": "https://example.com/blog/noindex-page",
      "reason_code": "noindex",
      "skip_reason": "technical blocker; internal links are not the right fix"
    }
  ]
}
```

### Child Task Artifact: `indexing_link_target`

Each `fix_indexing_internal_links` task gets one target object from the plan.

```json
{
  "campaign_task_id": "task-parent",
  "target": {
    "url": "https://example.com/blog/example-page",
    "slug": "example-page",
    "article_id": 42,
    "file": "042_example_page.mdx",
    "reason_code": "not_in_gsc",
    "incoming_link_count_before": 0,
    "target_keyword": "example keyword",
    "source_candidates": []
  }
}
```

### `indexing_link_context`

Written by the deterministic context step.

```json
{
  "target": {
    "article_id": 42,
    "title": "Example Page",
    "slug": "example-page",
    "url": "https://example.com/blog/example-page",
    "target_keyword": "example keyword",
    "current_incoming_ids": [],
    "current_outgoing_ids": [10, 11]
  },
  "sources": [
    {
      "article_id": 7,
      "title": "Related Topic",
      "slug": "related-topic",
      "file": "007_related_topic.mdx",
      "headings": ["Best Related Tools", "How to Choose"],
      "excerpt": "Short excerpt around likely placement areas...",
      "gsc_impressions": 1240,
      "already_links_to_target": false
    }
  ]
}
```

### `indexing_link_plan`

Produced by a Rig structured extraction step.

```json
{
  "target_article_id": 42,
  "links_to_add": [
    {
      "source_article_id": 7,
      "target_article_id": 42,
      "anchor_text": "example keyword guide",
      "target_slug": "example-page",
      "placement": "related_section",
      "reason": "The source page discusses the same decision process and has GSC impressions."
    }
  ]
}
```

Allowed placements in V1:

```text
related_section
contextual_paragraph
```

`related_section` is safer for V1 because the current apply logic already handles Related Articles sections. `contextual_paragraph` should only be enabled once the deterministic apply step can patch source paragraphs with validation and rollback.

### `indexing_link_verify`

Written by the verification step.

```json
{
  "target_article_id": 42,
  "target_slug": "example-page",
  "incoming_link_count_before": 0,
  "incoming_link_count_after": 2,
  "links_added": 2,
  "source_files_modified": ["007_related_topic.mdx"],
  "passed": true
}
```

## Desired End-to-End Workflow

```text
User opens Search Console > Drift
  -> Drift computes current report for display
  -> UI shows freshness and eligible link-recovery count

User clicks Fix links
  -> frontend calls create_gsc_indexing_recovery_task(project_id, options)
  -> frontend enqueues the returned parent task through queueStore/taskQueueActions

Queue runs gsc_indexing_recovery
  -> prepare refreshes stale GSC/link data
  -> drift computes current sitemap/GSC/link report
  -> plan writes gsc_recovery_plan
  -> post_actions creates one fix_indexing_internal_links task per eligible target

Queue auto-enqueues child tasks
  -> each child loads target artifact
  -> context step builds source shortlist
  -> agent chooses source links from shortlist
  -> apply step edits source MDX files
  -> verify step rescans links and proves target incoming count increased

Optional delayed verification
  -> outcome review re-inspects target URL in GSC after deployment/wait period
  -> report resolved/still failing/regressed
```

## Frontend Requirements

Update the Drift tab so the CTA matches the backend workflow.

Changes:

- Replace `createTask(projectId, 'cluster_and_link', ...)` in `GscDrift` with a typed wrapper such as `createGscIndexingRecoveryTask(projectId, options)`.
- Use queue actions with selectors rather than subscribing to the entire queue store.
- Rename button text from ambiguous "Fix links" to a concrete action such as "Start link recovery".
- Show counts before starting:
  - eligible targets
  - skipped technical blockers
  - stale GSC/link data warning
  - estimated tasks to create
- After enqueue, show the parent campaign task and let the queue display follow-up tasks as they are created.
- Keep Drift report read-only. Task creation belongs in typed Tauri wrappers and backend task factories.

## Backend Requirements

### Task Definitions

Add definitions for:

- `gsc_indexing_recovery`
- `fix_indexing_internal_links`
- `gsc_indexing_outcome_review` if implementing delayed verification in this phase

Update task definition tests for run policy, review surface, and follow-up policy.

### Workflow Steps

Add `StepKind` variants and registry entries for:

- `GscRecoveryPrepare`
- `GscRecoveryDrift`
- `GscRecoveryPlan`
- `IndexingLinkContext`
- `IndexingLinkPlan`
- `IndexingLinkApply`
- `IndexingLinkVerify`

The handler should route:

```text
gsc_indexing_recovery -> prepare, drift, plan
fix_indexing_internal_links -> context, plan, apply, verify
```

### GSC Recovery Domain Module

Create a focused module, for example:

```text
src-tauri/src/engine/exec/gsc/recovery.rs
```

Responsibilities:

- read campaign options from task artifacts or defaults
- resolve site and sitemap config using existing project/manifest rules
- call shared GSC collection helper when data is stale
- call shared link scan helper when link data is stale
- compute Drift from current inputs
- produce target plan
- build source candidates
- spawn child tasks through `TaskSpawner` from a post-action helper

Do not put this business logic in `commands/gsc.rs` or `GscDrift.tsx`.

### Shared Collection Refactor

Extract reusable helpers from `collect.rs` so `collect_gsc` and recovery use the same code path:

```text
resolve_site_config(project_id, project_path) -> SiteConfig
fetch_sitemap_urls_or_entries(sitemap_url, limit) -> Vec<SitemapEntry>
inspect_urls(site_url, urls, token) -> Vec<InspectionRecord>
write_gsc_collection(paths, records, site_config) -> CollectionSummary
sync_gsc_analytics(task, project_path, token) -> StepResult
```

The refactor should remove duplicated site logging and unify sitemap limits.

### Task Spawning

Add a post-action for `gsc_indexing_recovery`:

```text
after_task_success(gsc_indexing_recovery)
  -> load gsc_recovery_plan artifact
  -> for target in targets
       TaskSpawner::spawn(TaskSpec {
         task_type: "fix_indexing_internal_links",
         run_policy: AutoEnqueue,
         agent_policy: Required,
         priority: High/Medium from score,
         idempotency_key: "gsc-indexing-recovery:{project_id}:{reason_code}:{url}",
         dedup_policy: Cooldown { days: 14 },
         artifacts: [indexing_link_target],
         depends_on: [campaign_task_id]
       })
  -> return created task IDs
```

### Verification

Immediate verification must be deterministic:

- rescan the link graph after apply
- check the target article incoming count
- check that at least one source file now contains `/blog/{target_slug}`
- write updated `link_scan.json`
- fail or move to review if no inbound link was added

Delayed GSC verification should be deterministic:

- inspect the target URL after a configured wait period
- compare reason code before/after
- update GSC URL indexing status if using the existing SQLite table
- report `resolved`, `still_not_indexed`, `regressed`, or `unknown`

## Implementation Plan

### Phase 1: Campaign and Targeted Link Tasks

1. Add `gsc_indexing_recovery` and `fix_indexing_internal_links` task definitions.
2. Add typed IPC structs for campaign options and campaign creation response.
3. Add a thin backend command such as `create_gsc_indexing_recovery_task(project_id, options)` that calls a domain helper using `TaskSpawner::spawn`.
4. Add a typed wrapper in `src/lib/tauri.ts` and replace the Drift tab's generic `cluster_and_link` creation.
5. Use `useTaskQueueActions()` or queue store selectors from the Drift component instead of subscribing to the whole queue store.
6. Extract shared collection and site-config helpers from `collect.rs`.
7. Implement `gsc_recovery_prepare`, `gsc_recovery_drift`, and `gsc_recovery_plan`.
8. Add post-action spawning from the recovery plan using `TaskSpawner`.
9. Implement `indexing_link_context`, `indexing_link_plan`, `indexing_link_apply`, and `indexing_link_verify`.
10. Reuse Related Articles apply behavior in V1 to keep file edits conservative.
11. Add tests for target eligibility, idempotency, stale-data decisions, source candidate scoring, and verification pass/fail.

### Minimum Viable Slice

The smallest shippable version should do this end to end:

1. Drift button creates and enqueues one `gsc_indexing_recovery` parent task.
2. Parent refreshes link data when stale and refuses to plan if required GSC data is missing and cannot be refreshed.
3. Parent writes a `gsc_recovery_plan` with eligible zero-incoming targets.
4. Post-action spawns one `fix_indexing_internal_links` child per target, capped only by an explicit option.
5. Child tasks add Related Articles links from deterministic source candidates.
6. Verification proves each target has at least one inbound link after apply.

This slice deliberately excludes delayed GSC outcome tracking and contextual paragraph insertion. Those are valuable, but they are not required to fix the current "one button creates one broad task" failure.

### Phase 2: Better Placement and Outcomes

1. Add contextual paragraph placement with snapshot/rollback validation.
2. Add `gsc_indexing_outcome_review` with not-before metadata.
3. Add scheduler support for delayed outcome reviews if needed.
4. Track recovery history per URL in SQLite.
5. Surface campaign success metrics in Drift.

### Phase 3: Smarter Source Ranking

1. Add embeddings or better topical similarity if already available elsewhere in the app.
2. Use article cluster/hub metadata when present.
3. Limit source page overuse so one strong page does not get too many new links.
4. Learn from outcome reviews to adjust priority scoring.

## Files Likely Touched

Backend:

- `src-tauri/src/config/task_definitions.rs`
- `src-tauri/src/engine/workflows/step_kind.rs`
- `src-tauri/src/engine/workflows/handlers.rs`
- `src-tauri/src/engine/step_registry.rs`
- `src-tauri/src/engine/post_actions.rs`
- `src-tauri/src/engine/exec/gsc/collect.rs`
- `src-tauri/src/engine/exec/gsc/drift.rs`
- `src-tauri/src/engine/exec/gsc/recovery.rs`
- `src-tauri/src/engine/exec/content/cluster_link.rs` if extracting shared apply helpers
- `src-tauri/src/commands/gsc.rs` or `src-tauri/src/commands/tasks.rs` for a thin creation command
- `src-tauri/src/lib.rs` for command registration

Frontend:

- `src/components/gsc/GscDrift.tsx`
- `src/lib/tauri.ts`
- `src/lib/types.ts` and generated bindings if new IPC structs are exported
- `src/lib/taskQueueActions.ts` only if a reusable queue helper is useful

Tests and scripts:

- task definition tests
- recovery planner unit tests
- source candidate scoring tests
- task spawning/idempotency tests
- verification tests with fixture MDX files
- IPC and task-store checks

## Acceptance Criteria

- When Drift shows 56 eligible zero-incoming/not-in-GSC URLs, starting recovery creates a campaign that spawns 56 targeted child tasks, subject only to explicit user caps or deduplication.
- If `gsc_collection.json` is stale or missing, the campaign refreshes GSC inspection data before planning.
- If `link_scan.json` is stale or missing, the campaign refreshes the link graph before planning.
- The Drift frontend no longer creates a generic `cluster_and_link` task directly.
- Each child task contains a structured target artifact with URL, article ID, slug, reason, baseline incoming count, and source candidates.
- Each child task adds at least one inbound link to its target or fails/reviews with a clear reason.
- The verification step proves `incoming_link_count_after > incoming_link_count_before` for the target.
- Running the same campaign twice within the cooldown does not duplicate active/recent target tasks.
- Technical blockers create or preserve technical fix paths and are not sent to internal-link recovery.
- No prompt needs the full site graph for all targets; per-target prompts stay below the existing provider budget.
- Queue execution remains backend-owned; components enqueue tasks and do not call executor commands directly.

## Validation Commands

Run after implementation:

```bash
pnpm run check:task-store
pnpm run check:ipc
./scripts/check-bindings.sh
pnpm run lint
pnpm exec tsc -b
pnpm test
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml task_definitions
cargo test --manifest-path src-tauri/Cargo.toml gsc
pnpm run build
```

## Open Questions

- What should the default freshness threshold be: 24 hours, 12 hours, or always refresh before recovery?
- Should V1 inspect every sitemap URL, or respect a daily URL Inspection quota cap with partial recovery?
- Should the first implementation add only Related Articles links, or support contextual paragraph links immediately?
- How many source links should one target receive by default: 1, 2, or 3?
- Should delayed outcome reviews be user-enqueued first, or should the scheduler auto-enqueue them after a wait period?
- Should `interlinking` be preserved as a legacy alias, or migrated to `fix_indexing_internal_links` when a target artifact exists?