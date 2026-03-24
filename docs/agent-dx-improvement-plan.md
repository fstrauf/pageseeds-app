# Agent DX Improvement Plan

**Goal:** Make agentic development in this repo as reliable as it is in the Python/TS sister repos — fewer silent failures, faster feedback loops, smaller files agents need to reason about.

**Root cause summary:** The architecture is sound. The problems are (1) a single 3,000-line god module, (2) invisible runtime contracts, (3) zero automated tests, and (4) string-typed values that can only fail at runtime.

---

## Tier 1 — Highest Impact, Implement First

### 1.1 Split `executor.rs` into domain execution modules ✅ DONE

**Why this matters most:** `executor.rs` is 3,093 lines — 20% of the entire Rust codebase in one file. It contains 35+ functions spanning 7 unrelated domains. An agent modifying Reddit behavior must search through GSC, content review, and keyword research code to find the right function. Typos in one domain accidentally touch another.

**Target structure:**

```
src-tauri/src/engine/
├── executor.rs          # ~200 lines — orchestrator only (execute_task, run_step, _fail_task)
└── exec/
    ├── mod.rs           # re-exports all public exec_* functions
    ├── keywords.rs      # exec_keyword_research_native + theme/brief extractors (~400 lines)
    ├── content.rs       # exec_content_review_apply/recommend, select_priority_articles,
    │                    # build_review_context/prompt, create_content_review_apply_task (~550 lines)
    ├── content_audit.rs # exec_content_audit, audit_one_article, read_source_file,
    │                    # parse_frontmatter (~300 lines)
    ├── reddit.rs        # exec_reddit_search, persist_reddit_opportunities, exec_reddit_enrich,
    │                    # extract_trigger_topics, extract_seed_subreddits, extract_query_keywords,
    │                    # extract_excluded_subreddits, compute_scores, extract_json_array (~700 lines)
    ├── gsc.rs           # exec_collect_gsc, exec_gsc_sync_articles, exec_gsc_investigate,
    │                    # normalize_site_for_url_match, create_tasks_from_collection* (~600 lines)
    └── utils.rs         # find_file_by_suffix, misc helpers shared across exec modules (~50 lines)
```

**Migration steps:**
1. Create `engine/exec/` directory with empty `mod.rs`.
2. Move functions domain by domain — start with `exec/reddit.rs` (most self-contained).
3. Update `executor.rs` to call `exec::reddit::exec_reddit_search(...)` etc.
4. Run `cargo check` after each domain migration. Do not move multiple domains in one pass.
5. After all domains moved, only the orchestration loop + step dispatch remain in `executor.rs`.

**Result:**
- `executor.rs` is 370 lines (was 3,112) — orchestration only
- `cargo check` passes with 0 errors
- `exec/keywords.rs` 391, `exec/content.rs` 533, `exec/content_audit.rs` 248, `exec/gsc.rs` 700, `exec/reddit.rs` 658, `exec/utils.rs` 31
- `commands/reddit.rs` updated to call `crate::engine::exec::reddit::exec_reddit_enrich` directly

---

### 1.2 Add `CONTRACTS.md` to repo root ✅ DONE

`CONTRACTS.md` already created. Documents:
- Status/phase/execution_mode canonical values
- Workflow step kind ordering and state passing
- Auto-spawned follow-up task rules
- Handler registry ordering constraints
- Content pipeline execution order
- Reddit enrichment loop
- Commands layer thinness rule
- Secrets resolution order
- SQLite migration rules

**Note:** `AGENTS.md` Pre-Change Checklist already updated to reference `CONTRACTS.md`. ✅

---

### 1.3 Convert string statuses/phases/modes to Rust enums

**Why:** String values can only fail at runtime. Wrong spelling (`"in-progress"` vs `"in_progress"`) silently misroutes task status. Enums give agents compile-time errors.

**Affected types:**

| Field | Current type | Target enum | File |
|---|---|---|---|
| `Task.status` | `String` | `TaskStatus` | `models/task.rs` |
| `Task.execution_mode` | `String` | `ExecutionMode` | `models/task.rs` |
| `Task.agent_policy` | `String` | `AgentPolicy` | `models/task.rs` |
| `Task.priority` | `String` | `Priority` | `models/task.rs` |
| Phase values | `&str` constants | `Phase` (or keep as `&str`) | `config/mod.rs` |

**Proposed enums:**

```rust
// models/task.rs

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Review,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Automatic,
    Batchable,
    Manual,
    Spec,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentPolicy {
    None,
    Required,
    Optional,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    Low,
}
```

**Scope:** This is a large change — the string values are compared dozens of times across `executor.rs`, `batch.rs`, `scheduler.rs`, `task_store.rs`, and handlers. Do this **after** 1.1 (splitting executor) so the diff surface is smaller per file.

**TypeScript impact:** `types.ts` must be updated to use string literal unions:
```typescript
type TaskStatus = 'todo' | 'in_progress' | 'review' | 'done' | 'cancelled';
type ExecutionMode = 'automatic' | 'batchable' | 'manual' | 'spec';
```

**SQLite impact:** serde will serialize enums as snake_case strings, matching current DB values — no migration needed.

**Migration steps:**
1. Define enums in `models/task.rs`.
2. Update `Task` struct fields.
3. Update `models/task.rs` to compile.
4. Fix all compile errors in `engine/task_store.rs` (CRUD functions).
5. Fix all compile errors in `engine/executor.rs` (after 1.1 split, this is per-file).
6. Fix all compile errors in `commands/*.rs`.
7. Update `types.ts`.
8. Run `cargo check` + `tsc --noEmit`.

---

## Tier 2 — Medium Impact, Implement After Tier 1

### 2.1 Extract business logic from `commands/reddit.rs`

**Problem:** `draft_reddit_reply` (~100 lines of prompt engineering) and `post_to_reddit` (DB writes + history file side effects) violate the thin-command rule. Agents reading AGENTS.md will not look here for business logic and will miss it.

**Target:**
```
reddit/
├── prompts.rs    # build_draft_reply_prompt(), stance_to_instruction_block()
├── history.rs    # write_history_log(), create_history_manager()
├── db.rs         # (existing — no change)
├── search.rs     # (existing — no change)
├── post.rs       # (existing — no change)
└── config.rs     # (existing — no change)
```

**commands/reddit.rs** becomes 5-10 lines per command after extraction.

**Migration steps:**
1. Create `reddit/prompts.rs`. Move prompt-building logic from `draft_reddit_reply`.
2. Create `reddit/history.rs`. Move history management from `post_to_reddit`.
3. Update `commands/reddit.rs` to call the new functions.
4. `cargo check`.

---

### 2.2 Add a `AGENTS.md` reference to `CONTRACTS.md` ✅ DONE

`AGENTS.md` Pre-Change Checklist already includes:
```markdown
- [ ] Reviewed `CONTRACTS.md` for any affected implicit contracts
```

---

### 2.3 Add minimal Rust tests for executor invariants

**Why:** Zero tests means agents cannot verify their changes. A handful of targeted tests would catch the most common regressions without requiring a full test harness.

**Minimum viable test suite** in `engine/executor.rs` (or `engine/exec/` modules after split):

```rust
#[cfg(test)]
mod tests {
    // 1. Keyword research task finishes with "review" status
    #[test]
    fn keyword_research_task_goes_to_review() { ... }

    // 2. Content review auto-spawns apply task
    #[test]
    fn content_review_spawns_apply_task() { ... }

    // 3. Agentic → normalizer step ordering passes raw output
    #[test]
    fn normalizer_receives_agentic_output() { ... }

    // 4. Handler registry routes fix_ tasks to ImplementationHandler
    #[test]
    fn fix_prefix_routes_to_implementation_handler() { ... }

    // 5. Failed task resets to "todo" status
    #[test]
    fn failed_task_resets_to_todo() { ... }
}
```

These tests use an in-memory SQLite connection (`rusqlite::Connection::open_in_memory()`) — no external deps needed.

---

### 2.4 Document `WorkflowStep` params with inline doc comments

**Problem:** The `params` HashMap on `WorkflowStep` is invisible to agents. They can't tell which keys are consumed by the executor vs the runner.

**Target:** Add `// consumed by executor::run_step()` and `// consumed by exec_agentic()` doc comments to the `WorkflowStep::with_param()` usage sites in handlers, or create a typed `StepParams` struct.

Minimal approach — add a const module to `engine/workflows/mod.rs`:

```rust
/// Param keys consumed by the executor's step dispatch logic.
/// Use these constants instead of inline string literals.
pub mod step_params {
    pub const SKILL: &str = "skill";             // exec_agentic: names the SKILL.md to load
    pub const NORMALIZER_ID: &str = "normalizer_id"; // normalizer step: selects normalizer
    pub const ARTIFACT_NAME: &str = "artifact_name"; // normalizer step: names output artifact
    pub const RUNNER: &str = "runner";           // deterministic step: selects CLI runner
}
```

Then replace string literals throughout handlers and executor.

---

## Tier 3 — Nice to Have, Low Urgency

### 3.1 Auto-generate TypeScript types from Rust with `ts-rs`

**What it does:** Derives TypeScript interfaces directly from Rust structs at build time. Eliminates the manual `types.ts` sync requirement entirely.

**How:**
```toml
# Cargo.toml
ts-rs = "10"
```

```rust
#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Task { ... }
```

Running `cargo test` generates `bindings/Task.ts` which can be imported directly. The manual `src/lib/types.ts` becomes a re-export of the generated bindings.

**Caveat:** Initial setup requires adding `#[derive(TS)]` to all exported structs. Migration is mechanical but touches many files.

---

### 3.2 Add a workspace VS Code task for full validation

Create `.vscode/tasks.json` with a compound task that runs `tsc --noEmit` and `cargo check` together:

```json
{
    "label": "validate",
    "dependsOn": ["tsc check", "cargo check"],
    "dependsOrder": "parallel"
}
```

Agents can trigger this after changes to verify both layers compile without starting the dev server.

---

### 3.3 `--dry-run` mode for executor

Add a `dry_run: bool` parameter to `execute_task_with_token`. When true:
- Plans steps via handlers (same as normal)
- Does NOT call any exec_* functions
- Returns the planned step graph as the result

Useful for agents verifying that a new task type routes to the correct handler and produces the expected step sequence before committing to a full run.

---

## Implementation Order

| Step | Change | Effort | Risk | Dependency |
|---|---|---|---|---|
| 1 | Create `CONTRACTS.md` | ✅ Done | None | None |
| 2 | Split `executor.rs` → `engine/exec/` | ✅ Done | Medium | None |
| 3 | Reference `CONTRACTS.md` from `AGENTS.md` | ✅ Done | None | Step 1 |
| 4 | Extract Reddit business logic from commands | ✅ Done | Low | Step 2 |
| 5 | Convert string statuses to Rust enums | ✅ Done | Medium | Step 2 |
| 6 | Add executor invariant tests | ✅ Done | None | Step 2 |
| 7 | Document `WorkflowStep` params as constants | ✅ Done | None | Step 2 |
| 8 | `ts-rs` type generation | ✅ Done | Low | Step 5 |
| 9 | VS Code validation task | ✅ Done | None | None |
| 10 | Executor dry-run mode | ✅ Done | None | Step 2 |

---

## What NOT to Change

- Overall architecture: thin commands → modules → engine. It's correct.
- IPC surface via `tauri.ts` — clean and complete.
- SQLite migration strategy — idempotent migrations are safe.
- `AGENTS.md` structure — supplement it, don't rewrite it.
- Shared state model (`AppState`, `GscState`, `SeoState`) — works well.
