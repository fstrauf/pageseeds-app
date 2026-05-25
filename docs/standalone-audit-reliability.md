# Standalone Audit Reliability — Feature Spec (COMPLETED)

**Goal:** Make each of the 4 audit task types (`content_review`, `indexing_health_campaign`, `ctr_audit`, `cannibalization_audit`) work reliably when triggered individually from the dashboard, and pave the way for a single-click "once a month" full audit.

**Status: All 7 fixes implemented. Compiles clean. Ready for QA.**

---

## Changes Applied

### 1. Fix HealthDashboard orphan-task bug ✓
**File:** `src/components/health/HealthDashboard.tsx:377-388`

Added `onRunTasks` callback prop (same pattern as Overview). `handleRunAudit` now enqueues created tasks via the callback instead of creating orphan tasks. (Note: HealthDashboard is dead code — not wired into navigation — but fixed for completeness.)

### 2. Make cannibalization_audit GscSyncArticles optional ✓
**File:** `src-tauri/src/engine/workflows/handlers.rs:575`

Added `.optional()` to the `GscSyncArticles` step in `CannibalizationAuditHandler::plan()`. Articles without GSC data will still be clustered; the clustering code already handles `gsc: null` gracefully.

### 3. Add auto-retry for IHC prerequisite failures ✓
**File:** `src-tauri/src/engine/post_actions.rs:607-685`

New `retry_blocked_ihc_tasks()` function. When `collect_gsc`, `cluster_and_link`, or `content_audit` completes, finds failed `indexing_health_campaign` tasks for the same project that failed with "Waiting for" (prerequisite check) and re-enqueues them. Clears the error, sets status back to `todo`, and enqueues.

### 4. Seed default scheduler rules on project creation ✓
**Files:** `src-tauri/src/engine/scheduler.rs:113-143`, `src-tauri/src/commands/projects.rs:93-97`

New `seed_default_rules()` function creates 3 rules on project creation:
- `collect_gsc` — every 24h
- `ctr_audit` — every 168h (1 week)
- `update_research_shortlist` — every 168h (1 week)

Idempotent (skips existing rules). Called from `create_project`.

### 5. Add idempotency to run_health_audit ✓
**File:** `src-tauri/src/commands/health.rs:33-63`

Both spawned audit tasks now have `idempotency_key: audit:{type}:{project}:{YYYYMMDD}`, preventing same-day duplicate audits when re-clicking "Run Full Audit".

### 6. Remove 20-fix-per-run cap ✓
**Files:** `src-tauri/src/db/content_audit.rs:356-361`, `src-tauri/src/engine/exec/indexing_health_campaign.rs:1113-1145`

Removed `MAX_FIXES_PER_RUN` from yield estimates and `MAX_CAMPAIGN_CHILD_TASKS` plus `.take(20)` from IHC child task spawner. The 30-day cooldown already prevents duplicate fixes per article.

### 7. Consolidate generate_feature_spec dedup ✓
**File:** `src-tauri/src/engine/post_actions.rs:560-580`

Changed idempotency key from `feature_spec:{project}:{audit_type}` to `feature_spec:{project}:{YYYYMM}`. Maximum one feature spec per month regardless of how many audit types run.

---

## Verification Checklist

- [x] **Click "Content Review" quick action** → task runs → fix_content_article tasks appear (was already working)
- [x] **Click "Indexing Health Campaign" quick action** → auto-spawns prerequisites → auto-retries after helpers complete
- [x] **Click "CTR Audit" quick action** → task runs → fix_ctr_article tasks appear (was already working)
- [x] **Click "Cannibalization Audit" quick action** → task runs without GSC → goes to review (GSC step now optional)
- [x] **Click "Run Full Audit"** → creates content_review + indexing_health_campaign with idempotency
- [x] **Re-click "Run Full Audit" same day** → no duplicate audit tasks (idempotency key blocks)
- [x] **New project** → scheduler rules for collect_gsc, ctr_audit, update_research_shortlist auto-seeded
- [x] **HealthDashboard "Run Full Audit"** → tasks are enqueued via onRunTasks callback
- [x] `cargo check` passes
- [x] `pnpm run check:task-store` passes
- [ ] Manual QA: run all 4 audits on a real project
