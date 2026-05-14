# Feature Spec: Unified Task Deduplication System

> **Status:** Draft  
> **Scope:** Backend + Frontend  
> **Files touched:** 8-12  
> **Estimated effort:** 4-6 hours

---

## 1. Problem Statement

The system has multiple avenues where tasks are created (autonomous audits, user approvals, scheduler, quick-run, follow-ups). While an idempotency layer exists (`TaskSpawner` + `task_idempotency_keys`), it is inconsistently applied and contains exploitable gaps that allow duplicate tasks targeting the same work to be created silently.

Specifically:
- **Cannibalization approval flow** embeds the parent audit task UUID in idempotency keys, so every re-run of the audit bypasses deduplication entirely.
- **`quick_run_workflow`** includes a timestamp in its idempotency key, making it useless.
- **Every domain reinvents dedup rules**: GSC hardcodes a 14-day cooldown, CTR scans all tasks and parses JSON artifacts, content review has no cooldown, and follow-ups are permanently blocked after completion.
- **The frontend has zero awareness** of whether an approved recommendation already has a task created, completed, or failed.

The user can currently click "Create Tasks from Approved" multiple times and spawn duplicates without any feedback.

---

## 2. Goals

1. **Prevent duplicate task creation** for the same semantic work across re-runs of parent audits.
2. **Allow legitimate re-creation** when enough time has passed or the underlying issue has changed.
3. **Give the user visibility** into which approved recommendations already have tasks and what their status is.
4. **Unify deduplication logic** so new task types get safe defaults without reinventing the wheel.

Non-goals:
- Automatic retry of failed tasks (out of scope — see executor retry design).
- Changing the queue execution model.
- Deduplicating tasks across projects.

---

## 3. Guiding Principle

> **A task's idempotency key identifies WHAT work it does, not WHICH parent spawned it.**

If two audits recommend merging the same cluster, that is the same work. The key should be `can_fix:merge:{project}:{cluster_id}`. The parent audit task UUID must not appear in the key.

If a new audit finds different CTR issues for the same article (different content hash), that is new work. The key may include the content hash: `ctr_fix:article:{project}:{article_id}:{hash}:{issues}`.

---

## 4. Current State Inventory

| Flow | Trigger | Idempotency Key | Checks Active? | Cooldown? | Status |
|---|---|---|---|---|---|
| **Cannibalization fix** | Manual (frontend button) | `can_fix:{type}:{project}:{strategy_id}:{rec_id}` | ❌ Key only | ❌ | **BROKEN** |
| **`quick_run`** | Manual (frontend button) | `quick_run:{type}:{timestamp}` | ✅ `find_active_task_by_type` | ❌ | **BROKEN** |
| **GSC fix** | Auto (`post_actions`) | `gsc:{reason}:{url}` | ✅ + 14-day cooldown | ✅ 14 days | Works but hardcoded |
| **CTR fix** | Auto (`post_actions`) | `ctr_fix:article:{project}:{id}:{hash}:{issues}` | ✅ Active scan | ❌ | Works but O(n) scan |
| **Content review fix** | Auto (`post_actions`) | `fix_content_article:{project}:{article_id}` | ✅ Status check | ❌ | Works |
| **Follow-up** | Auto (`post_actions`) | `followup:{parent}:{type}:{title}` | ✅ Blocks forever | ❌ Permanent | Too aggressive |
| **Scheduler** | Auto (`scheduler.rs`) | `scheduler:{rule_id}:{YYYYMMDD}` | ✅ Key only | ❌ Daily only | Works |

---

## 5. Proposed Architecture

### 5.1 Core Abstraction: `DeduplicationPolicy`

Introduce a typed policy enum that replaces ad-hoc boolean checks and hardcoded cooldowns.

```rust
/// How the spawner should behave when an idempotency key matches an existing task.
pub enum DeduplicationPolicy {
    /// Always create a new task. Use only for genuinely one-off tasks.
    AlwaysCreate,

    /// Skip creation if ANY task exists with this key, regardless of status.
    /// Used for follow-ups that should never be duplicated.
    SkipIfAnyExists,

    /// Skip only if an active (todo / queued / in_progress / review) task exists.
    /// If the existing task is done / failed / cancelled, create a new one.
    SkipIfActive,

    /// Skip if active. If done/failed/cancelled, allow re-creation only after
    /// the cooldown has expired. When expired, the old idempotency key is deleted.
    Cooldown {
        /// Cooldown duration in days.
        days: u32,
    },
}
```

### 5.2 Database Changes

#### Migration: `task_idempotency_keys` gets `expires_at`

```sql
ALTER TABLE task_idempotency_keys ADD COLUMN expires_at TEXT;
CREATE INDEX idx_task_idempotency_keys_expires ON task_idempotency_keys(expires_at);
```

- `expires_at` is nullable. `NULL` means "never expire" (current behaviour).
- On `TaskSpawner::spawn`, if a key is found but `expires_at < now`, the key is deleted and creation proceeds.
- On `find_by_idempotency_key`, expired keys are lazily cleaned up.

#### Composite index for active-task lookups

```sql
CREATE INDEX idx_tasks_project_type_status ON tasks(project_id, type, status);
```

This speeds up `find_active_task_by_type` and any new semantic lookup queries.

### 5.3 `TaskSpawner` API Changes

```rust
pub struct TaskSpec {
    // ... existing fields ...

    /// Optional stable key for deduplication. If omitted, no dedup is performed.
    pub idempotency_key: Option<String>,

    /// How to handle duplicate keys. Defaults to SkipIfActive if a key is provided.
    pub dedup_policy: Option<DeduplicationPolicy>,
}
```

`TaskSpawner::spawn` behaviour:

1. If no `idempotency_key`, create task directly.
2. Look up key in `task_idempotency_keys`.
3. If key not found → create task, record key (with `expires_at` if policy is `Cooldown`).
4. If key found and task exists:
   - `AlwaysCreate` → ignore key, create task (new key overwrites old? or append? — decide: overwrite, log warning).
   - `SkipIfAnyExists` → return existing task.
   - `SkipIfActive` + existing is active → return existing task.
   - `SkipIfActive` + existing is done/failed/cancelled → delete old key, create new task.
   - `Cooldown` + existing is active → return existing task.
   - `Cooldown` + existing is done/failed/cancelled + within cooldown → return existing task.
   - `Cooldown` + existing is done/failed/cancelled + expired → delete old key, create new task.
5. If key found but task was deleted (orphan key) → delete key, create new task.

### 5.4 Refactoring Domain Spawners

Each domain spawner is updated to use `TaskSpawner` with an explicit policy instead of custom pre-checks.

| Domain | Current Behaviour | New Policy | Key Format (unchanged) |
|---|---|---|---|
| **GSC fix** | Custom `should_skip_issue()` with 14-day hardcoded logic | `Cooldown { days: 14 }` | `gsc:{reason}:{url}` |
| **CTR fix** | Custom active scan + idempotency | `SkipIfActive` | `ctr_fix:article:{project}:{id}:{signature}` |
| **Content review** | Status check in spawner | `SkipIfActive` | `fix_content_article:{project}:{article_id}` |
| **Cannibalization** | Broken (includes strategy_id) | `SkipIfActive` | `can_fix:{type}:{project}:{rec_id}` |
| **Follow-up** | Blocks forever | `SkipIfActive` | `followup:{parent}:{type}:{title}` |
| **Scheduler** | Daily key | `SkipIfAnyExists` | `scheduler:{rule_id}:{YYYYMMDD}` |
| **quick_run** | Useless timestamp key | `SkipIfActive` | `quick_run:{project}:{task_type}` |

The GSC `should_skip_issue()` function is **deleted**. Its logic moves into `TaskSpawner`.
The CTR `find_active_ctr_fix_task_for_article()` scan is **kept as a secondary check** (because it catches tasks created before this migration that lack idempotency keys), but new tasks rely on the spawner policy.

### 5.5 Semantic Identity Helper (Future-proofing)

Add a helper in `task_store.rs` for queries that need to find tasks by the entity they target, without parsing JSON artifacts:

```rust
/// Find the most recent task of a given type targeting a specific entity.
/// This is a convenience wrapper around (project_id, type, status) filtering.
pub fn find_active_task_by_type_and_target(
    conn: &Connection,
    project_id: &str,
    task_type: &str,
    target_entity: &str,   // e.g. "article:42", "cluster:csp"
) -> Result<Option<Task>> {
    // Filter tasks by type + active status, then check artifacts or title for target match.
    // Used by CTR spawner as a fallback and by future spawners that want explicit target tracking.
}
```

This is **not** required for Phase 1-2, but spec'd here so future spawners don't copy the CTR O(n) scan pattern.

---

## 6. Frontend Changes

### 6.1 Cannibalization Review: Surface Existing Task Status

When `get_cannibalization_strategy` loads, the backend should also return the status of any existing fix tasks for approved recommendations.

New backend command or enriched return type:

```rust
pub struct RecommendationTaskStatus {
    pub recommendation_type: String,
    pub recommendation_id: String,
    pub task_id: Option<String>,
    pub task_status: Option<TaskStatus>,
}
```

The `StrategyWithReviews` struct gets a new field:

```rust
pub struct StrategyWithReviews {
    pub strategy: CannibalizationStrategy,
    pub reviews: Vec<StrategyReview>,
    pub task_statuses: Vec<RecommendationTaskStatus>,  // NEW
    pub strategy_id: String,
    pub project_id: String,
}
```

`get_cannibalization_strategy` queries for existing tasks by looking up tasks whose idempotency keys match the patterns `can_fix:*:{project_id}:*`. For each match, it maps the key back to the recommendation type + id.

### 6.2 UI Updates

In `CannibalizationReview.tsx`, each recommendation card shows:

- **No task yet** → current behaviour (Approve/Reject buttons)
- **Task exists, active** (todo / queued / in_progress) → Badge "Task queued" + disable Create Tasks for this specific rec
- **Task exists, review** → Badge "Under review" + link to task
- **Task exists, done** → Badge "Completed {date}" + allow re-approval with cooldown warning
- **Task exists, failed** → Badge "Failed" + retry button (resets task + re-enqueues)

The "Create Tasks from Approved" button caption changes:
- If all approved recs already have active tasks: "All approved tasks already created"
- If some are new: "Create N new tasks from approved (M already exist)"

---

## 7. Implementation Phases

### Phase 1: Immediate Fixes (closes active duplicates)

**Files:** `commands/cannibalization.rs`, `commands/skills.rs`

1. Change cannibalization idempotency keys to exclude `strategy_id`:
   - Merge: `can_fix:merge:{project_id}:{cluster_id}`
   - Hub: `can_fix:hub:{project_id}:{topic}`
   - Territory: `can_fix:territory:{project_id}:{theme}`
   - Calculator: `can_fix:calculator:{project_id}:{strategy}`
2. Change `quick_run` idempotency key to `quick_run:{project_id}:{task_type}` (drop timestamp).
3. Verify `find_active_task_by_type` is still called first (it is) so in-progress tasks can't be duplicated.

### Phase 2: Expiration + Policy Infrastructure

**Files:** `db/mod.rs`, `engine/spawner.rs`, `models/task.rs`, `config/task_definitions.rs`

1. Add migration for `task_idempotency_keys.expires_at`.
2. Add `DeduplicationPolicy` enum to `models/task.rs` or `engine/spawner.rs`.
3. Extend `TaskSpec` with `dedup_policy: Option<DeduplicationPolicy>`.
4. Update `TaskSpawner::spawn` to implement the policy matrix.
5. Update `TaskSpawner::spawn_follow_up` to use `SkipIfActive` instead of permanent blocking.
6. Add composite index `idx_tasks_project_type_status`.

### Phase 3: Refactor Domain Spawners to Unified Policy

**Files:** `engine/exec/gsc/task_spawner.rs`, `engine/exec/ctr_audit/task_spawner.rs`, `engine/exec/content/task_spawner.rs`

1. **GSC:** Delete `should_skip_issue()`. Pass `DeduplicationPolicy::Cooldown { days: 14 }` in the `TaskSpec`.
2. **CTR:** Keep `find_active_ctr_fix_task_for_article()` as a defensive fallback for legacy tasks, but new tasks use `SkipIfActive`.
3. **Content review:** Remove the manual status check after `TaskSpawner::spawn`. Rely on `SkipIfActive`.
4. **Scheduler:** Confirm it already uses `TaskSpawner` — no change needed if `SkipIfAnyExists` is the default for keys.

### Phase 4: Frontend Awareness (Cannibalization)

**Files:** `components/cannibalization/CannibalizationReview.tsx`, `lib/tauri.ts`, `lib/types.ts`, `commands/cannibalization.rs`, `models/cannibalization.rs`

1. Add `task_statuses` to `StrategyWithReviews`.
2. Update `get_cannibalization_strategy` to query existing fix tasks and populate the field.
3. Regenerate TypeScript bindings.
4. Update React component to render badges and refined button states.

---

## 8. Testing Plan

### Unit tests (Rust)

In `engine/spawner.rs` tests:

1. `spawn_with_idempotency_key_and_skip_if_active_blocks_active_tasks`
2. `spawn_with_idempotency_key_and_skip_if_active_allows_recreate_after_done`
3. `spawn_with_cooldown_blocks_within_period`
4. `spawn_with_cooldown_allows_recreate_after_expiry`
5. `spawn_follow_up_allows_recreate_after_failure_when_policy_is_skip_if_active`
6. `orphan_idempotency_key_is_cleaned_up`
7. `expired_idempotency_key_is_cleaned_up`

### Integration tests

1. Run `cannibalization_audit` → approve cluster "csp" → create tasks → assert 1 task created.
2. Run `cannibalization_audit` again → approve same cluster "csp" → create tasks → assert 0 new tasks created, existing task returned.
3. Mark task as Done → create tasks again → assert 1 new task created (because `SkipIfActive` allows re-creation).
4. Run `collect_gsc` → assert fix tasks created.
5. Run `collect_gsc` again within 14 days → assert same fix tasks returned, no new ones.
6. Wait 14 days (or manipulate `updated_at` in test DB) → run `collect_gsc` → assert new fix tasks created.

### Frontend tests

1. Load CannibalizationReview with approved recs that have active tasks → button shows "All approved tasks already created".
2. Load with approved recs where some have failed tasks → button shows "Create 2 new tasks (1 failed — can retry)".

---

## 9. Rollback / Compatibility

- The `task_idempotency_keys` schema change is additive (new nullable column). No migration rollback needed.
- Existing keys without `expires_at` are treated as "never expire" — current behaviour preserved.
- The cannibalization key format change is **not backward compatible**: old keys with `strategy_id` will remain in the DB but will never match new keys. This is intentional — the old keys were broken. Orphan keys will be lazily cleaned up when the parent task is deleted.
- CTR's `find_active_ctr_fix_task_for_article()` fallback ensures legacy tasks (created before this spec) are still detected.

---

## 10. Open Questions

1. **Should `Cooldown` use `updated_at` or `task_runs.finished_at`?**  
   GSC currently uses `task.updated_at`. `task_runs.finished_at` is more precise for "when did the work actually complete?". Recommendation: use `task_runs.finished_at` if available, fall back to `tasks.updated_at`.

2. **Should we add `target_entity` as a first-class column on `tasks`?**  
   This would make `find_task_by_target` an indexed O(1) lookup instead of an O(n) scan. However, it adds schema complexity. Recommendation: defer to a future migration if the O(n) scan becomes a performance issue.

3. **Should failed tasks be treated differently from done tasks for `SkipIfActive`?**  
   The user said: "if they failed, okay, we can rerun them". So `SkipIfActive` should allow re-creation for both Done and Failed. This is the proposed default.

---

## 11. Related Files Reference

| File | Role in this change |
|---|---|
| `src-tauri/src/commands/cannibalization.rs` | Fix idempotency keys; add task status lookup |
| `src-tauri/src/commands/skills.rs` | Fix `quick_run` idempotency key |
| `src-tauri/src/engine/spawner.rs` | Add `DeduplicationPolicy`; implement expiration logic |
| `src-tauri/src/engine/task_store.rs` | Add `find_active_task_by_type_and_target`; index |
| `src-tauri/src/engine/exec/gsc/task_spawner.rs` | Delete `should_skip_issue`; use `Cooldown` policy |
| `src-tauri/src/engine/exec/ctr_audit/task_spawner.rs` | Use `SkipIfActive`; keep fallback scan |
| `src-tauri/src/engine/exec/content/task_spawner.rs` | Remove manual status checks |
| `src-tauri/src/db/mod.rs` | Migration for `expires_at` column + composite index |
| `src-tauri/src/models/task.rs` | Add `DeduplicationPolicy` |
| `src-tauri/src/models/cannibalization.rs` | Add `RecommendationTaskStatus` |
| `src/components/cannibalization/CannibalizationReview.tsx` | Show task status badges |
| `src/lib/tauri.ts` | Update type wrappers |
| `src/lib/types.ts` | Re-export updated types |
