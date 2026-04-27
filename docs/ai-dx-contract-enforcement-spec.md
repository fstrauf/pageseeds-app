# AI DX Contract Enforcement Spec

Status: Draft  
Owner: TBD  
Created: 2026-04-27

## Summary

The current architecture is broadly sound, but feature work still depends on too many manually synchronized contracts across Rust, TypeScript, workflow routing, generated bindings, and docs. This is especially painful for AI-assisted development because agents can make small local changes that compile, while the app fails later at runtime.

This spec converts the repo's most important implicit contracts into generated code, CI checks, or smaller typed surfaces. The goal is not a large rewrite. The goal is to remove the memory game from common feature work.

## Problem

Feature implementation currently requires developers and agents to keep these surfaces aligned by hand:

- Rust command functions in `src-tauri/src/commands/`.
- Manual command registration in `src-tauri/src/lib.rs`.
- Manual frontend wrappers in `src/lib/tauri.ts` and secondary bridges such as `src/lib/logging-bridge.ts`.
- Generated Rust model bindings in `src/lib/bindings/`.
- Manual frontend re-exports and extra types in `src/lib/types.ts`.
- Task type strings in `src-tauri/src/config/mod.rs`, workflow handlers, executor status logic, task spawners, and UI logic.
- Step kinds in `step_kind.rs`, workflow plans, and the step registry.
- Executor side effects keyed on task type, step name, and step kind.

This creates a pattern where `cargo check`, `pnpm exec tsc -b`, and even many tests can pass while the product breaks in the UI or during a workflow run.

## Review Findings To Address

### 1. IPC Drift Is Possible And Currently Present

`src/lib/tauri.ts` exposes wrappers for commands that are not registered in `src-tauri/src/lib.rs`:

- `get_ctr_health_summary`
- `repair_article_paths`

These wrappers are used by the UI, so this is a runtime failure path even though TypeScript and Rust can both compile.

The one-off audit command used during review was:

```bash
python3 - <<'PY'
import re
from pathlib import Path
root=Path('/Users/fstrauf/01_code/pageseeds-app')
lib=(root/'src-tauri/src/lib.rs').read_text()
ts=(root/'src/lib/tauri.ts').read_text()
registered=set(re.findall(r'commands::([a-zA-Z0-9_]+)', lib))
invoked=set(re.findall(r"invoke\('([^']+)'", ts))
print('registered', len(registered), 'invoked', len(invoked))
print('\nInvoked but not registered:')
for x in sorted(invoked-registered): print(x)
print('\nRegistered but not wrapped in tauri.ts:')
for x in sorted(registered-invoked): print(x)
PY
```

Result at time of review:

```text
registered 138 invoked 135

Invoked but not registered:
get_ctr_health_summary
repair_article_paths

Registered but not wrapped in tauri.ts:
check_agent_status_for_project
dry_run_task
execute_task
get_kimi_backend_mode
set_kimi_backend_mode
```

The critical failure is `invoked but not registered`. Missing wrappers for registered commands may be intentional, but should be explicit.

### 2. The Rust/TypeScript Bridge Is A Manual Parallel System

Command names, argument names, and return types are hand-written in `src/lib/tauri.ts`. The Rust side is hand-registered in `src-tauri/src/lib.rs`. Tauri's camelCase/snake_case conversion helps at runtime, but it does not give compile-time coverage that a command exists or that its payload matches the Rust signature.

### 3. Workflow Additions Still Touch Too Many Places

A new workflow step usually requires edits in:

- `src-tauri/src/engine/workflows/step_kind.rs`
- `src-tauri/src/engine/workflows/handlers.rs`
- `src-tauri/src/engine/step_registry.rs`
- one or more `src-tauri/src/engine/exec/*` modules
- sometimes `src-tauri/src/engine/executor.rs` for follow-up task behavior or output propagation

The `StepKind` enum is an improvement over raw strings, but `as_str` and `FromStr` duplicate every mapping. `WorkflowStep::new(name, kind: &str)` also still asks callers to pass strings instead of enum variants.

### 4. The Executor Still Owns Domain Side Effects

`src-tauri/src/engine/executor.rs` is much smaller than it used to be, but it still contains cross-domain behavior:

- passing `latest_raw_output` by special-casing step kinds and step names
- Reddit persistence and result fetching
- content write orphan ingestion
- follow-up task spawning for content review, write article, GSC, CTR, and cannibalization
- GSC fix resolution bookkeeping
- special failed-status behavior for `fix_ctr_article`

This keeps the executor as a place developers must inspect whenever they add workflow behavior.

### 5. CI Does Not Enforce Enough Backend Behavior

The current CI runs binding staleness checks and frontend lint/type/build checks. It should also run backend checks and tests. This repo has meaningful Rust tests, and the highest-risk behavior is in Rust workflow orchestration.

### 6. Local Binding Sync Can Hide Failures

`scripts/check-bindings.sh` is strict. `scripts/sync-bindings.sh` currently swallows binding export failures with `|| true`, then copies whatever binding files exist. That can preserve stale TypeScript after a failed Rust export.

### 7. Docs Are Helpful But Partly Historical

The repo has useful docs, especially `AGENTS.md`, `CONTRACTS.md`, and `docs/dev-process.md`. Some older DX plans still describe already-solved problems or teach the old multi-file workflow loop as the normal path. Agents can follow stale guidance and reintroduce friction.

## Goals

- Make missing Tauri command registrations fail in CI.
- Make local binding generation fail loudly.
- Add backend checks to CI.
- Reduce stringly typed workflow construction.
- Centralize task type metadata so phase, execution mode, review behavior, and handler ownership are not scattered.
- Move workflow side effects out of the generic executor where practical.
- Give future agents one current implementation path instead of several historical docs.

## Non-Goals

- Rewriting the app architecture.
- Replacing Tauri.
- Replacing SQLite.
- Removing Rust from the backend.
- Migrating every frontend data-fetching pattern in this spec.
- Solving all product UX issues. This spec targets developer experience and maintainability.

## Implementation Plan

## Phase 0: Fix The Known IPC Drift

Before adding the guardrail, decide what to do with the two frontend wrappers that currently have no registered Rust command.

### Option A: Implement And Register The Missing Commands

Use this if the UI actions are intended to ship.

Files likely involved:

- `src-tauri/src/commands/content.rs`
- `src-tauri/src/lib.rs`
- `src/lib/tauri.ts`
- `src/lib/types.ts` or generated bindings if new structs cross IPC

Commands:

- `get_ctr_health_summary(project_id: String) -> Result<CtrHealthSummary, String>`
- `repair_article_paths(project_id: String) -> Result<RepairPathResult, String>`

Implementation notes:

- Do not put business logic in the command body.
- Add or reuse domain functions under `src-tauri/src/content/` or `src-tauri/src/engine/exec/content/`.
- If `CtrHealthSummary` and `RepairPathResult` are frontend-only manual types today, either generate Rust bindings for them or rename the frontend types to make the manual boundary explicit.

### Option B: Remove The Dead Wrappers And UI Calls

Use this if the feature was experimental or replaced.

Files likely involved:

- `src/lib/tauri.ts`
- `src/components/overview/Overview.tsx`
- `src/components/articles/CtrHealthPanel.tsx`
- `src/lib/types.ts`

Acceptance for Phase 0:

- No frontend `invoke(...)` call references an unregistered command.
- `pnpm exec tsc -b` passes.
- `cargo check` passes.

## Phase 1: Add An IPC Surface Check

Create a script that compares all frontend `invoke('command_name')` calls against the commands registered in `tauri::generate_handler![...]`.

### New Script

Add `scripts/check-ipc-surface.mjs`.

Behavior:

1. Read `src-tauri/src/lib.rs`.
2. Extract command registrations with `commands::name` from the `generate_handler!` block.
3. Search `src/**/*.ts` and `src/**/*.tsx` for static Tauri invokes:
   - `invoke('command')`
   - `invoke<Type>('command')`
4. Fail if any static invoke command is not registered.
5. Print registered commands that have no frontend invoke as warnings by default.
6. Support an optional allowlist for intentionally backend-only commands.

Suggested allowlist file:

```json
{
  "registeredButNotInvoked": [
    "execute_task",
    "dry_run_task"
  ]
}
```

Only add to the allowlist after confirming the command is intentionally not called through a static frontend wrapper.

### Package Script

Add to `package.json`:

```json
{
  "scripts": {
    "check:ipc": "node scripts/check-ipc-surface.mjs"
  }
}
```

### CI

Add `pnpm run check:ipc` to `.github/workflows/ci.yml`.

This check does not need app dependencies beyond Node, so it can run in the frontend job after `pnpm install`, or in a separate lightweight job.

### Acceptance

- `pnpm run check:ipc` fails on `invoke('missing_command')`.
- `pnpm run check:ipc` passes on the current repo after Phase 0.
- CI fails if frontend code invokes an unregistered command.

## Phase 2: Make Binding Sync Fail Loudly

Update `scripts/sync-bindings.sh`.

Required changes:

- Use `set -euo pipefail`.
- Remove `|| true` from `cargo test export_bindings --lib --quiet`.
- Delete or refresh the destination bindings directory before copying, so removed Rust exports do not leave stale TypeScript files behind.
- Keep generating `src/lib/bindings/index.ts`.

Suggested shell shape:

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "Generating TypeScript bindings from Rust..."
cd src-tauri
cargo test export_bindings --lib --quiet
cd ..

rm -rf src/lib/bindings
mkdir -p src/lib/bindings
cp src-tauri/bindings/*.ts src/lib/bindings/

cd src/lib/bindings
{
  echo "// Auto-generated TypeScript bindings from Rust"
  echo "// Generated by ts-rs - DO NOT EDIT MANUALLY"
  echo "// Run ./scripts/sync-bindings.sh to regenerate"
  echo
  for file in *.ts; do
    if [[ "$file" != "index.ts" ]]; then
      name="${file%.ts}"
      echo "export type { $name } from './$name'"
    fi
  done
} > index.ts
```

Acceptance:

- A failing Rust export fails `./scripts/sync-bindings.sh`.
- Removed bindings are removed from `src/lib/bindings/`.
- `./scripts/check-bindings.sh` passes after running sync.

## Phase 3: Add Backend CI Checks

Add a backend job to `.github/workflows/ci.yml`.

Recommended commands:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

Use the same Linux system dependencies already installed for the binding check:

```bash
sudo apt-get update
sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libglib2.0-dev pkg-config
```

Acceptance:

- CI runs Rust check and test jobs on PRs.
- Existing Rust tests pass in CI.
- New workflow routing tests cannot be skipped accidentally by only running frontend checks.

## Phase 4: Tighten Workflow Step Typing

The repo already has `StepKind`, but handler plans still commonly use `StepKind::X.as_ref()` and `WorkflowStep::new(name, kind: &str)`. Make enum usage the default path.

### API Change

Change `WorkflowStep` constructors in `src-tauri/src/engine/workflows/mod.rs`:

```rust
impl WorkflowStep {
    pub fn new(name: &str, kind: StepKind) -> Self {
        Self {
            name: name.to_string(),
            kind,
            params: Default::default(),
            optional: false,
        }
    }

    pub fn from_kind_str(name: &str, kind: &str) -> Self {
        let parsed = StepKind::from_str(kind)
            .unwrap_or_else(|_| panic!("Unknown step kind '{}'", kind));
        Self::new(name, parsed)
    }
}
```

Then update handlers from:

```rust
WorkflowStep::new("ctr_build_context", StepKind::CtrBuildContext.as_ref())
```

to:

```rust
WorkflowStep::new("ctr_build_context", StepKind::CtrBuildContext)
```

### Reduce StepKind Mapping Duplication

Option A: Use `strum`.

Add to `src-tauri/Cargo.toml`:

```toml
strum = { version = "0.26", features = ["derive"] }
```

Derive `EnumString`, `Display`, and/or `AsRefStr` with `serialize = "snake_case"` attributes.

Option B: Keep manual mapping for now, but add a test that every non-`Unknown` variant round-trips through `as_str()` and `FromStr`.

Short-term acceptance:

- Handler code no longer passes string step kinds.
- `cargo check` catches invalid step kind usage in handler plans.
- Step kind round-trip tests exist.

Long-term acceptance:

- `StepKind` string mappings are derived or generated, not hand-duplicated.

## Phase 5: Centralize Task Type Metadata

Task type behavior is currently scattered across config, workflow handlers, executor status logic, and docs. Create one source of truth for task type metadata.

### New Module

Add `src-tauri/src/engine/task_definitions.rs` or `src-tauri/src/config/task_definitions.rs`.

Suggested shape:

```rust
use crate::models::task::ExecutionMode;

pub struct TaskDefinition {
    pub task_type: &'static str,
    pub phase: &'static str,
    pub execution_mode: ExecutionMode,
    pub review_on_success: bool,
    pub handler_family: HandlerFamily,
}

pub enum HandlerFamily {
    Collection,
    Investigation,
    Research,
    Content,
    ContentReview,
    Reddit,
    Social,
    Performance,
    Coverage,
    CtrAudit,
    CannibalizationAudit,
    Implementation,
    Manual,
}
```

Provide helpers:

```rust
pub fn all() -> &'static [TaskDefinition];
pub fn find(task_type: &str) -> Option<&'static TaskDefinition>;
pub fn default_phase(task_type: &str) -> &'static str;
pub fn default_execution_mode(task_type: &str) -> ExecutionMode;
pub fn review_on_success(task_type: &str) -> bool;
```

### Migration Steps

1. Move `TASK_TYPES`, `default_phase`, and `default_execution_mode` logic into the new registry.
2. Keep compatibility functions in `src-tauri/src/config/mod.rs` that delegate to the registry.
3. Update `completed_task_status` in `executor.rs` to call `review_on_success(task_type)`.
4. Update handler registry tests to iterate over `task_definitions::all()` instead of a string slice.
5. Add tests for expected review tasks:
   - `research_keywords`
   - `custom_keyword_research`
   - `research_landing_pages`
   - `reddit_opportunity_search`
6. Add tests that every task definition has a non-fallback handler unless `handler_family == Manual`.

Acceptance:

- New task types are added in one registry first.
- Phase, execution mode, and review status behavior come from the same definition.
- `CONTRACTS.md` can refer to generated/central definitions instead of duplicating lists.

## Phase 6: Move Executor Side Effects Behind Domain Hooks

Do this after the smaller checks are in place. The objective is to keep `executor.rs` as an orchestrator, not a domain behavior hub.

### Short-Term Refactor: Post-Step And Post-Task Hooks

Create `src-tauri/src/engine/post_actions.rs`.

Suggested API:

```rust
pub struct PostStepContext<'a> {
    pub conn: &'a rusqlite::Connection,
    pub task: &'a Task,
    pub step: &'a WorkflowStep,
    pub result: &'a StepResult,
    pub project_path: &'a str,
    pub agent_provider: &'a str,
}

pub struct PostTaskContext<'a> {
    pub conn: &'a rusqlite::Connection,
    pub task: &'a Task,
    pub project_path: &'a str,
    pub progress: &'a [StepProgress],
}

pub fn after_step(ctx: PostStepContext<'_>) -> PostStepResult;
pub fn after_task_success(ctx: PostTaskContext<'_>) -> Vec<String>;
```

Move these executor blocks into domain-focused functions called by `post_actions`:

- Reddit search persistence.
- Reddit enrichment loop.
- Reddit fetch results artifact creation.
- Reddit opportunities normalizer persistence.
- Content write orphan ingestion.
- Content review follow-up task creation.
- Write article `cluster_and_link` follow-up creation.
- GSC follow-up task creation.
- CTR fix task creation.
- Cannibalization fix task creation.
- Indexing diagnostics spawned task collection.
- GSC fix resolution recording.
- `fix_content_article` review-state completion.

The first refactor may still use matches internally. The win is that the orchestration loop becomes readable and side effects are grouped.

### Long-Term Refactor: Typed Step Outcomes

Extend `StepResult` or introduce `StepOutcome`:

```rust
pub struct StepOutcome {
    pub success: bool,
    pub message: String,
    pub output: Option<String>,
    pub artifacts: Vec<TaskArtifact>,
    pub follow_up_task_ids: Vec<String>,
    pub latest_raw: LatestRawPolicy,
}

pub enum LatestRawPolicy {
    Preserve,
    ReplaceWithOutput,
    Clear,
}
```

This removes the need for executor logic that says, for example, `if step.name == "ctr_build_context" { latest_raw_output = result.output.clone(); }`.

Acceptance:

- `executor.rs` no longer contains domain-specific strings such as `reddit_fetch_results`, `content_write_stage`, `ctr_audit`, or `cannibalization_audit` outside tests.
- Follow-up task behavior is tested in domain modules.
- The executor loop is responsible for sequencing, persistence, status transitions, and event emission only.

## Phase 7: Stabilize Frontend Query Utilities

This is lower priority than IPC and workflow contracts, but it will reduce UI churn.

Update `src/hooks/useQuery.ts`:

- Avoid depending on the whole `options` object in `useMutation` callbacks.
- Store `onSuccess`, `onError`, and `invalidateQueries` in refs or destructure scalar values before `useCallback`.
- Add tests for invalidation and stable callbacks.

Acceptance:

- `pnpm test src/hooks/useQuery.test.ts` covers cache invalidation.
- Inline mutation options do not recreate `mutate` unnecessarily when the actual option values have not changed.

## Phase 8: Refresh Agent-Facing Docs

Update docs after the code changes so future agents do not follow historical instructions.

Files to update:

- `AGENTS.md`
- `CONTRACTS.md`
- `docs/dev-process.md`
- `docs/dx-improvement-spec.md`
- `docs/agent-dx-improvement-plan.md`

Required doc changes:

- Mark old completed DX plans as historical.
- Add `pnpm run check:ipc` to validation checklists.
- Replace the old workflow-add loop with the new typed constructor and task definition registry.
- Document the new rule: every frontend `invoke` must be statically registered or explicitly allowlisted.
- Document that `scripts/sync-bindings.sh` fails loudly and should be run after Rust model changes.

Acceptance:

- A developer can follow one current "Add a feature" checklist.
- No doc tells agents to add string step kinds via `StepKind::X.as_ref()`.
- No doc implies frontend wrappers are validated solely by `tsc`.

## Validation Commands

Run these before merging the full feature:

```bash
pnpm run check:ipc
./scripts/check-bindings.sh
pnpm run lint
pnpm exec tsc -b
pnpm test
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
pnpm run build
```

For frontend-only phases, run the relevant subset plus `pnpm run check:ipc`. For Rust workflow phases, run `cargo check` and `cargo test` after each small migration.

## Rollout Order

1. Fix the current IPC drift.
2. Add `check-ipc-surface.mjs` and CI coverage.
3. Harden `sync-bindings.sh`.
4. Add Rust CI checks.
5. Change workflow constructors to accept `StepKind` directly.
6. Centralize task definitions.
7. Extract executor post-actions.
8. Stabilize frontend query utilities.
9. Refresh docs.

Do not combine phases 4, 5, and 6 in one PR. Those touch core workflow behavior and should stay reviewable.

## Risks And Mitigations

### Risk: IPC Check Produces False Positives

Mitigation:

- Only parse static string invokes.
- Start by failing only on `invoked but not registered`.
- Treat `registered but not invoked` as warnings until the command surface is cleaned up.

### Risk: Task Definition Registry Becomes Another Thing To Sync

Mitigation:

- Keep compatibility helpers in `config/mod.rs` that delegate to the registry.
- Update tests to iterate over the registry.
- Remove old duplicated task-type lists after migration.

### Risk: Executor Hook Refactor Changes Behavior

Mitigation:

- Move one domain at a time.
- Keep tests around follow-up task creation and status transitions.
- Preserve existing output artifact keys during the first migration.

### Risk: Strict Binding Sync Breaks Local Workflow

Mitigation:

- That is intended when Rust exports fail.
- Improve error messages in the script so the fix is obvious.

## Success Criteria

- A frontend invoke of an unregistered Tauri command fails CI.
- Rust model binding export failures cannot silently preserve stale TypeScript.
- Rust workflow tests run in CI.
- Adding a workflow step uses `StepKind` directly and requires fewer manual string edits.
- Adding a task type starts from one task definition registry.
- `executor.rs` no longer needs domain-specific edits for common follow-up behavior.
- Agent-facing docs describe the current implementation path, not historical cleanup plans.
