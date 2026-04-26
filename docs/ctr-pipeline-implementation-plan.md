# CTR Pipeline Implementation Plan

**Goal:** Transform the current CTR Audit workflow from a "create a task and hope" model into a reliable, self-driving pipeline that steadily reduces the article backlog, skips already-healthy work, and never silently overwrites uncommitted changes.

**Status:** Specification & implementation plan. No code changes yet.

**Key modules involved:**
- `src-tauri/src/engine/exec/ctr_audit.rs`
- `src-tauri/src/engine/exec/audit_health.rs`
- `src-tauri/src/engine/workflows/handlers.rs`
- `src-tauri/src/engine/executor.rs`
- `src-tauri/src/engine/batch.rs`
- `src-tauri/src/engine/task_store.rs`
- `src-tauri/src/db/mod.rs`
- `src/components/overview/CtrHealthPanel.tsx`
- `.github/skills/ctr-fix-apply/SKILL.md`
- `.github/skills/ctr-optimization/SKILL.md`

---

## 1. Introduce a CTR Issue State Model

**Problem:** `article_audit_state` stores only `was_healthy`, `content_hash`, and `issues_found`. It has no durable lifecycle for individual CTR issues (recommended → fix created → applied → verified → failed → skipped).

**Solution:** Add a per-project, per-article, per-issue state table.

### Schema: `article_ctr_issues`

| Column | Type | Notes |
|--------|------|-------|
| `project_id` | TEXT | FK to projects |
| `article_id` | INTEGER | FK to articles |
| `issue_type` | TEXT | `title`, `meta`, `snippet`, `faq` |
| `status` | TEXT | `open`, `recommended`, `queued`, `applied`, `verified`, `failed`, `skipped`, `manual_review` |
| `detected_at` | TEXT (ISO) | When the audit first found it |
| `last_verified_at` | TEXT (ISO) | When deterministic checks last ran |
| `content_hash_at_detection` | TEXT | Hash of the article at detection time |
| `fix_task_id` | TEXT | FK to tasks.id (nullable) |
| `failure_reason` | TEXT | Why a fix failed (nullable) |
| `verified_hash` | TEXT | Content hash when verified as fixed |

### Tasks

1. [ ] Add `MIGRATION_V{N}` in `db/mod.rs` with the new table DDL.
2. [ ] Add CRUD functions in `engine/task_store.rs` (or a new `engine/ctr_state.rs`):
   - `upsert_ctr_issue(conn, project_id, article_id, issue_type, status, ...)`
   - `get_open_ctr_issues(conn, project_id) -> Vec<CtrIssueRecord>`
   - `get_ctr_issues_for_article(conn, project_id, article_id) -> Vec<CtrIssueRecord>`
   - `mark_ctr_issue_verified(conn, issue_id, hash)`
   - `mark_ctr_issue_failed(conn, issue_id, reason)`
3. [ ] Keep `article_audit_state` as a coarse summary, but migrate CTR-specific workflow logic to read from `article_ctr_issues`.

---

## 2. Make the Pipeline Article-Centric

**Problem:** Fix tasks are grouped by fix type (`fix_title_meta`, `fix_faq_schema`, `fix_snippet_bait`), so one MDX file may be edited by 2–3 separate tasks. This complicates verification and creates race conditions.

**Solution:** Bundle all CTR issues for one article into a single fix task. The unit of work is the *article*, not the fix type.

### Recommended Flow

1. Audit step produces per-article recommendation bundles.
2. `create_ctr_fix_tasks` groups recommendations by `article_id`.
3. Each fix task receives one article (or a small batch of 2–3) with *all* issue types included.
4. The agent applies all fixes to the file in one pass.
5. One verification step per article follows.

### Tasks

1. [ ] Modify `create_ctr_fix_tasks` in `ctr_audit.rs`:
   - Group recommendations by `article_id` instead of `fix_type`.
   - Create task type `fix_ctr_article` (replacing `fix_title_meta`, `fix_faq_schema`, `fix_snippet_bait`).
   - Embed a per-article issue bundle artifact.
2. [ ] Update `ImplementationHandler::plan` in `handlers.rs`:
   - Replace the three fix-type cases with a single `fix_ctr_article` case.
   - Route to `StepKind::Agentic` with `ctr-fix-apply` skill.
3. [ ] Update `ctr-fix-apply/SKILL.md`:
   - Change instructions to accept per-article bundles with multiple fix types.
   - Instruct agent to apply all fixes to one file in a single pass.
4. [ ] Update the health summary query to count `open` issues from `article_ctr_issues` instead of raw health checks.

---

## 3. Add a Deterministic Verification Step

**Problem:** The agent reports what it changed, but the system never verifies the file was actually fixed. An issue is considered "done" when the task completes, not when the health check passes.

**Solution:** Add a `verify_ctr_fix` deterministic step that re-runs `audit_health::check_article_health` on the exact file after the agent edits it.

### Verification Rules

1. Re-read the MDX file.
2. Re-run `check_article_health` for the specific issue types in the bundle.
3. Compare current state vs. the issue bundle.
4. Update `article_ctr_issues`:
   - All issues pass → mark `verified`
   - Some issues still fail → mark `failed` or `still_open`
   - File missing → mark `failed` with `failure_reason`
5. Update project-level health summary.

### Tasks

1. [ ] Add `exec_ctr_verify_fix` in `ctr_audit.rs` (or `engine/exec/ctr_verify.rs`).
2. [ ] Add `StepKind::CtrVerifyFix` in `workflows/step_kind.rs`.
3. [ ] Register the step handler in `engine/step_registry.rs`.
4. [ ] Wire verification into `fix_ctr_article` workflow:
   - Step 1: `fix_ctr_article_apply` (agentic)
   - Step 2: `fix_ctr_article_verify` (deterministic)
5. [ ] Update `get_ctr_health_summary` to count only `open` + `failed` issues as backlog, not all unhealthy articles.

---

## 4. Enforce Dependencies at the Executor Boundary

**Problem:** `batch.rs` and `executor.rs` have separate dependency checks. The executor can mark a task `in_progress` even if its `depends_on` tasks are not `done` or `review`. A failed fix task does not block the follow-up audit.

**Solution:** Centralize dependency readiness into one helper used by both batch and queue paths.

### Required Behavior

- Before any task transitions to `in_progress`, check `depends_on`.
- If any dependency is not `done` or `review`, return `blocked`.
- Do not mutate task status to `in_progress` for blocked tasks.
- Queue runner should skip blocked tasks and continue to the next ready one.
- If a fix task fails, the follow-up audit remains blocked indefinitely (until user intervenes or the issue is retried).

### Tasks

1. [ ] Add `task_store::are_dependencies_met(conn, task_id) -> Result<bool>` as the single source of truth.
2. [ ] Update `engine/executor.rs` `execute_task` (or `execute_queue_internal`):
   - Call `are_dependencies_met` before `update_task_status(..., InProgress)`.
   - If false, return `ExecutionResult { success: false, message: "blocked: unresolved dependencies", ... }`.
3. [ ] Update `engine/batch.rs`:
   - Replace inline dependency logic with a call to `are_dependencies_met`.
4. [ ] Update queue runner UI to surface blocked tasks with a clear label.

---

## 5. Package Skills With the App

**Problem:** `exec_agentic` in `handlers.rs` loads skills from the target project repo (`{repo}/.github/skills/{name}/SKILL.md`). If the user's project doesn't contain `ctr-fix-apply`, the agent falls back to a generic prompt and summarizes instead of editing files.

**Solution:** Add an app-level fallback path for built-in skills.

### Required Change

1. [ ] Add a `skills/` directory inside `src-tauri/` (or bundle skills into the binary at build time).
2. [ ] Modify `engine/skills.rs` `load_skill`:
   - First, try `{project_repo}/.github/skills/{name}` (user override).
   - Second, try `{app_bundle}/skills/{name}` (built-in fallback).
   - Third, return `None` and surface an error.
3. [ ] Copy `ctr-optimization` and `ctr-fix-apply` into the app bundle.
4. [ ] Update `exec_agentic` to fail hard (not fallback) if a step declares `skill` param but the skill is missing.
5. [ ] Add build script or `include_str!` macro to embed critical skills into the binary.

---

## 6. Add Typed CTR Contracts

**Problem:** Agent output is passed around as `serde_json::Value`. Invalid agent output silently creates weak fix tasks instead of failing fast.

**Solution:** Replace loose JSON with typed structs and validate at the normalizer boundary.

### Recommended Structs

```rust
struct CtrRecommendation {
    article_id: i64,
    url_slug: String,
    file: String,         // enriched from articles.json
    priority: String,     // "high" | "medium" | "low"
    expected_ctr_improvement: String,
    fixes: Vec<CtrFix>,
}

struct CtrFix {
    fix_type: CtrFixType, // title_rewrite | meta_description | faq_schema | snippet_bait
    current: Option<String>,
    recommended: String,
    reason: String,
}

struct CtrAgentOutput {
    recommendations: Vec<CtrRecommendation>,
}

struct CtrFixReport {
    applied: Vec<CtrFixApplied>,
    skipped: Vec<CtrFixSkipped>,
    summary: String,
}

struct CtrFixApplied {
    article_id: i64,
    file: String,
    changes: Vec<CtrFixChange>,
}
```

### Tasks

1. [ ] Define structs in `models/ctr.rs` with `#[derive(TS)]` and `#[ts(export)]`.
2. [ ] Run `./scripts/sync-bindings.sh` to generate TypeScript types.
3. [ ] Update `engine/normalizer.rs` to attempt deserialization into `CtrAgentOutput`.
4. [ ] If deserialization fails, mark the step failed with `message: "Agent output does not match CtrAgentOutput schema"`.
5. [ ] Update `create_ctr_fix_tasks` to accept `&[CtrRecommendation]` instead of `&serde_json::Value`.

---

## 7. Clarify "Healthy" Rules

**Problem:** The deterministic checks and the skill instructions are misaligned:

| Check | Current | Skill Says |
|-------|---------|------------|
| Title length | ≤ 60 chars | ≤ 55 chars |
| Meta description | ≥ 50 chars | 140–155 chars |
| FAQ | Markdown FAQ heading accepted | JSON-LD FAQPage schema required |
| Snippet | ≥ 30 words | 40–60 words |

**Solution:** Define one shared threshold config and align both the health checker and the skill.

### Tasks

1. [ ] Add `engine/config/ctr_thresholds.rs` (or constants in `audit_health.rs`):
   - `TITLE_MAX_LEN: usize = 55`
   - `META_MIN_LEN: usize = 140`
   - `META_MAX_LEN: usize = 155`
   - `SNIPPET_MIN_WORDS: usize = 40`
   - `SNIPPET_MAX_WORDS: usize = 60`
   - `FAQ_REQUIRES_JSON_LD: bool = true`
2. [ ] Update `audit_health.rs` `check_article_health` to use these constants.
3. [ ] Update `ctr-optimization/SKILL.md` to reference the same thresholds.
4. [ ] Update `ctr-fix-apply/SKILL.md` to reference the same thresholds.
5. [ ] Decide: does "healthy" mean "SERP-ready" (strict) or "minimum acceptable" (lenient)? Document the choice in the spec.

---

## 8. Split Path Repair From Audit

**Problem:** `exec_ctr_build_context` calls `repair_article_file_paths` before analyzing. This can silently delete DB entries and overwrite `articles.json` while the audit is running.

**Solution:** Make path repair an explicit, user-triggered maintenance action. The audit should read a consistent index and report missing files as issues, not mutate the index.

### Tasks

1. [ ] Remove `repair_article_file_paths` call from `exec_ctr_build_context`.
2. [ ] Add a deterministic `sync_article_index` step at the start of the CTR workflow:
   - Non-destructive by default.
   - Reports orphaned files, missing files, path mismatches.
   - Optionally offers to repair (but does not auto-delete).
3. [ ] Keep `repair_article_paths` as a standalone user command (already exists in `commands/content.rs`).
4. [ ] In `exec_ctr_build_context`, if `articles.json` is missing, fail fast with a clear error: "Run 'Sync Article Index' first."
5. [ ] If a referenced `file` path does not resolve, treat it as a `file_not_found` issue in the audit output (do not delete the article).

---

## 9. UX Spec

**Problem:** The current UX creates a single `ctr_audit` task. The user model should be "Run CTR until the backlog is down", not "create one task and hope follow-ups work."

### Primary Button

- **Label:** "Run CTR Audit"
- **Action:** Creates or resumes a CTR run. Auto-queues ready tasks until no ready CTR tasks remain, unless a task fails.
- **State:** Disabled while a CTR run is active for this project.

### CTR Health Card

Show:
- Total articles in index
- Open CTR issues (backlog)
- Verified fixes this run
- Failed fixes (with retry button)
- Articles skipped (with reason)
- Last run status: `idle`, `running`, `blocked`, `completed`

### Optional Menu

- **Run Full Sweep** — process all eligible articles, not just top 20.
- **Run Verification Only** — re-verify all `applied` issues without re-auditing.
- **Clear CTR State** — reset `article_ctr_issues` for this project (dangerous, with confirm).

### Tasks

1. [ ] Update `Overview.tsx` CtrHealthPanel to show issue-based counts instead of article-based counts.
2. [ ] Add "Run CTR Audit" button with state machine: `idle → running → completed/blocked`.
3. [ ] Add dropdown menu for Full Sweep, Verification Only, Clear State.
4. [ ] Update `tauri.ts` with new commands if needed.
5. [ ] Add frontend state for "active CTR run" to prevent duplicate clicks.

---

## 10. Acceptance Tests

Add tests covering the following behaviors:

1. [ ] Healthy unchanged article is skipped on re-audit.
2. [ ] New article with no state enters backlog.
3. [ ] Changed healthy article is rechecked.
4. [ ] FAQ removal is detected as regression.
5. [ ] Fix task creates `article_ctr_issues` rows with status `queued`.
6. [ ] Agent report alone does not mark issues `verified`.
7. [ ] Verification step marks resolved issues `verified`.
8. [ ] Failed fix blocks follow-up audit (dependency enforcement).
9. [ ] Queue does not execute unresolved `depends_on`.
10. [ ] Re-running CTR does not create duplicate active work for the same article issue.
11. [ ] Full sweep eventually reaches articles beyond the top 20 if earlier issues are resolved or skipped.
12. [ ] Missing skill fails step with clear error instead of generic fallback.

---

## Recommended Build Order

| Phase | Focus | Modules |
|-------|-------|---------|
| **1** | Shared dependency readiness check | `executor.rs`, `batch.rs`, `task_store.rs` |
| **2** | Shared app-level skill fallback | `skills.rs`, `handlers.rs`, build scripts |
| **3** | Typed CTR contracts | `models/ctr.rs`, `normalizer.rs`, bindings |
| **4** | Per-article issue state table | `db/mod.rs`, `engine/ctr_state.rs` |
| **5** | Article-centric fix task creation | `ctr_audit.rs`, `handlers.rs`, `step_registry.rs` |
| **6** | Deterministic verification step | `ctr_audit.rs` (or new `ctr_verify.rs`), `audit_health.rs` |
| **7** | Threshold alignment + path repair split | `audit_health.rs`, `content/ops.rs`, `ctr-optimization/SKILL.md` |
| **8** | UX polish | `Overview.tsx`, `tauri.ts`, `types.ts` |

### Priority Rule

The shortest path to reliable product behavior is:
1. **Issue-based state** (so the system knows what was already tried)
2. **Deterministic verification** (so "fixed" actually means fixed)
3. **Dependency-aware executor** (so failures block follow-ups instead of silently passing)

Once those three are in place, the rest is refinement.
