# Agent Development Playbook

Scenario-based guide for the most common agent tasks in this repo.

> **When to use this:** You know what you're building and need the concrete file path + validation step.  
> **When to use AGENTS.md instead:** You need architecture context, the full directory map, or the pre-change checklist.

---

## Scenario: Changing a Skill

**Use when:** The AI's output tone, structure, or instructions need to change, but the execution flow stays the same.

**Primitive:** Skill files (`.github/skills/{skill_name}/SKILL.md`) + embedded defaults (`src-tauri/src/skills/`)

**Files to inspect first:**
- `src-tauri/src/engine/skills.rs` — skill loading order (project overrides embedded)
- `src-tauri/src/engine/prompts.rs` — how skill content is assembled into prompts
- Existing skill in `.github/skills/{name}/SKILL.md` or `src-tauri/src/skills/{name}/SKILL.md`

**Files usually touched:**
- `.github/skills/{new_skill}/SKILL.md` (new skill)
- Or existing skill file for edits

**Files NOT touched:**
- `engine/workflows/handlers.rs`
- `engine/executor.rs`
- `config/task_definitions.rs`
- Any `engine/exec/` module

**Validation:**
```bash
# Verify the skill loads
cargo test --manifest-path src-tauri/Cargo.toml skills::
```

**Rule of thumb:** If your change can be expressed as "tell the AI to do X differently," it's a skill change. If you need new data prep or a different step sequence, it's a workflow change.

---

## Scenario: Adding a New Content-Writing Behavior

**Use when:** You need the AI to write a new kind of content (hub page, landing page, glossary entry, etc.).

**Primitive:** `write_article` task type + `ContentHandler` + skill param

**Files to inspect first:**
- `src-tauri/src/engine/workflows/handlers.rs` — `ContentHandler::plan()` shows the single agentic step
- `src-tauri/src/engine/workflows/handlers.rs` — `exec_agentic()` shows how skills are loaded and prompts built
- `src-tauri/src/content/ops.rs` — `ingest_orphan_files()` for how articles are saved

**Files usually touched:**
- New skill: `.github/skills/{new_skill}/SKILL.md`
- If article persistence needs tweaking: `content::ops` or `db::export`

**Files NOT touched:**
- `config/task_definitions.rs` (no new task type needed)
- `engine/workflows/handlers.rs` (unless the step graph changes)
- Any new `engine/exec/` module

**Validation:**
```bash
# Verify the task routes to ContentHandler
cargo test --manifest-path src-tauri/Cargo.toml task_definitions
```

**Anti-pattern:** See AGENTS.md "Anti-Pattern Case Study: Hub Page Creation" for what happens when you ignore this.

---

## Scenario: Attaching Tasks to Execution (Queue)

**Use when:** A component needs to run tasks or show queue state.

**Primitive:** Backend queue commands in `tauri.ts`

**Files to inspect first:**
- `src/lib/tauri.ts` — `enqueueTasks`, `getQueueSnapshot`, `pauseQueue`, `resumeQueue`
- `src/stores/queueStore.ts` — how the frontend subscribes to queue state
- `src/hooks/useQueueRunner.ts` — how the UI reacts to queue events

**Files usually touched:**
- Component file in `src/components/`
- Possibly `src/stores/queueStore.ts` for new queue actions

**Files NOT touched:**
- `engine/executor.rs` (queue execution is already wired)
- `engine/batch.rs` (batch loop is self-contained)
- Any direct `invoke('execute_task')` call

**Rule:** Components call `enqueueTasks()` and listen to events. They never call `executeTask` directly.

---

## Scenario: Designing Task Lifecycle Behavior

**Use when:** A feature creates tasks, puts tasks in the queue, changes what happens after a task succeeds, or requires user input before downstream tasks exist.

**Primitive:** `TaskDefinition` + `TaskSpawner` + backend queue + review surface

**Files to inspect first:**
- `src-tauri/src/config/task_definitions.rs` — source of truth for `run_policy`, `review_surface`, `follow_up_policy`, and `handler_family`
- `src-tauri/src/engine/spawner.rs` — centralized task creation and idempotency
- `src-tauri/src/engine/post_actions.rs` — backend follow-up creation after successful task runs
- `src-tauri/src/engine/queue.rs` — backend-owned queue, persistence, auto-enqueue behavior
- `src/lib/taskQueueActions.ts` and `src/stores/queueStore.ts` — frontend queue entry points

**Choose the lifecycle lane first:**

| If the feature needs... | Use this lane | Usually touched |
|---|---|---|
| A user clicks a button to run existing tasks | Enqueue existing tasks | Component + `taskQueueActions`/queue context |
| Code creates tasks without user picking from results | System-created tasks | Domain module or `post_actions` + `TaskSpawner::spawn` |
| A completed task creates automatic downstream work | Backend follow-up | `post_actions.rs` + `TaskSpawner::spawn_follow_up` |
| The user must choose keywords/recommendations/opportunities first | User-selection follow-up | `task_definitions.rs`, review UI, selection command |
| Results should be reviewed but not converted into tasks | Review-only artifact | `task_definitions.rs` review surface, no task creation |

**Rules:**
- `run_policy` answers: can this task be auto-enqueued, or must the user enqueue it?
- `review_surface` answers: should completion stop in `review` and show a picker/review UI?
- `follow_up_policy` answers: are follow-ups backend-created, user-selected, or absent?
- The executor already sends any task with a non-`none` review surface to `review`; do not reimplement that status logic.
- The queue auto-enqueues only created follow-ups whose `run_policy` is `auto_enqueue`; user-selected follow-ups wait for a selection command.
- Selection commands validate the selected IDs against the parent task artifact, create downstream tasks through the task creation primitive, and mark the parent done.

**Files NOT touched:**
- Do not add ad-hoc task execution calls in components.
- Do not add lifecycle branches to `engine/executor.rs` unless the executor contract itself changes.
- Do not call `task_store::create_task` from new programmatic task factories.

**Validation:**
```bash
pnpm run check:task-store
cargo test --manifest-path src-tauri/Cargo.toml task_definitions
```

---

## Scenario: Adding Follow-Up Tasks

**Use when:** A task should automatically create downstream tasks on success.

**Primitive:** `TaskSpawner::spawn_follow_up()`

**Files to inspect first:**
- `src-tauri/src/engine/spawner.rs` — `spawn_follow_up()` and idempotency key format
- `src-tauri/src/engine/post_actions.rs` — where follow-ups are triggered after task success
- `src-tauri/src/config/task_definitions.rs` — follow-up task `run_policy`, review surface, and default metadata

**Files usually touched:**
- `src-tauri/src/engine/post_actions.rs` — add follow-up creation logic
- If a new artifact needs parsing: the parent task's exec module

**Files NOT touched:**
- `engine/task_store.rs` directly (use `TaskSpawner`)
- `commands/` (follow-ups are backend-side)

**Validation:**
```bash
# Verify no direct create_task calls outside allowlist
pnpm run check:task-store
```

---

## Scenario: Adding Downstream Data to Prompts

**Use when:** An agentic step needs structured context (GSC metrics, article excerpts, keyword data).

**Primitive:** Task artifacts + deterministic prep steps

**Files to inspect first:**
- `src-tauri/src/engine/workflows/handlers.rs` — how steps pass artifacts via `latest_raw_output`
- `src-tauri/src/models/task.rs` — `TaskArtifact` structure
- `src-tauri/src/engine/exec/` — existing deterministic steps that build context

**The correct pattern:**
1. Deterministic step: gather data, compute metrics, write structured JSON artifact
2. Agentic step: load artifact via `step.params["artifact"]` or task artifacts, interpret and act

**Files usually touched:**
- `engine/workflows/handlers.rs` — add deterministic step before agentic step
- `engine/exec/{domain}.rs` — implement deterministic data gathering
- Or reuse existing deterministic step if data already available

**Files NOT touched:**
- Do not make the agent rediscover file paths from prose — pass structured paths in artifacts
- Do not pass raw bulk data to the LLM — filter/sort/group first

---

## Scenario: Adding a New Workflow Step

**Use when:** You need a new kind of step that doesn't exist (new API integration, new file format, new analysis).

**Primitive:** `StepKind` enum + registry + executor match arm + exec function

**Files to inspect first:**
- `src-tauri/src/engine/workflows/step_kind.rs` — existing variants
- `src-tauri/src/engine/step_registry.rs` — registry mapping
- `src-tauri/src/engine/executor.rs` — `run_step()` match dispatch

**Files touched (in order):**
1. `engine/workflows/step_kind.rs` — add `StepKind::YourStep`
2. `engine/step_registry.rs` — register the string ↔ enum mapping
3. `engine/executor.rs` — add match arm in `run_step()`
4. `engine/exec/{domain}.rs` — implement `exec_your_step()`
5. `engine/workflows/handlers.rs` — add step to handler's plan if needed

**Validation:**
```bash
# Verify StepKind round-trips through registry
cargo test --manifest-path src-tauri/Cargo.toml step_registry
```

---

## Scenario: Adding Article Persistence

**Use when:** You need to save, update, or export article data.

**Primitive:** `content::ops` + `db::export`

**Files to inspect first:**
- `src-tauri/src/content/ops.rs` — `ingest_orphan_files()`, `sync_and_validate()`, `read_file_metadata()`
- `src-tauri/src/db/export.rs` — `write_articles_to_repo()`, `export_articles()`
- `src-tauri/src/content/publish.rs` — `apply_publish()` for status transitions

**Files usually touched:**
- Reuse existing function from `content::ops` or `db::export`
- If new fields needed: `models/article.rs` + `db/mod.rs` migration + `db/export.rs`

**Files NOT touched:**
- Do not write raw SQL + manual `fs::write` of `articles.json`
- Do not bypass `content::ops` for ID assignment or date policy

**Validation:**
```bash
# Verify export round-trip works
cargo test --manifest-path src-tauri/Cargo.toml export::
```

---

## Scenario: Adding Frontend UI Around Backend Data

**Use when:** You need a new panel, table, or form that displays or mutates data.

**Primitive:** Tauri command → `tauri.ts` wrapper → component

**Files to inspect first:**
- `src/lib/tauri.ts` — existing invoke wrappers
- `src/lib/types.ts` — type definitions
- `src/components/` — similar component for patterns

**Files touched (in order):**
1. Rust: `commands/{domain}.rs` — thin command (or reuse existing)
2. Rust: `lib.rs` — register in `generate_handler!` if new command
3. TypeScript: `src/lib/tauri.ts` — add typed wrapper
4. TypeScript: `src/lib/types.ts` — add/update type if needed
5. React: `src/components/{domain}/` — build component

**Files NOT touched:**
- No `invoke()` calls outside `tauri.ts`
- No business logic in components
- No Zustand bare store subscriptions (use selectors)

**Validation:**
```bash
# Verify no unregistered invokes
pnpm run check:ipc
# Type check
pnpm exec tsc -b
```

---

## Quick Validation Cheat Sheet

| What you changed | Run this |
|---|---|
| Rust model with `#[ts(export)]` | `./scripts/sync-bindings.sh && ./scripts/check-bindings.sh` |
| New command or changed signature | `pnpm run check:ipc` |
| New task type or handler | `cargo test --manifest-path src-tauri/Cargo.toml task_definitions` |
| Task lifecycle or task creation logic | `pnpm run check:task-store && cargo test --manifest-path src-tauri/Cargo.toml task_definitions` |
| Documentation links | `./scripts/check-docs-links.sh` |
| Skill paths | `./scripts/check-skill-paths.sh` |
| Frontend invoke usage | `./scripts/check-invoke-usage.sh` |
| Full validation | `./scripts/pre-release-checks.sh` |

---

## See Also

- [`AGENTS.md`](../AGENTS.md) — Full architecture reference, directory map, deep rules, pre-change checklist
- [`WORKFLOW_ENGINE.md`](./WORKFLOW_ENGINE.md) — How handlers, steps, and the executor interact
- [`TASK_QUEUE.md`](./TASK_QUEUE.md) — Queue semantics and event flow
- [`CONTRACTS.md`](../CONTRACTS.md) — Runtime invariants and hidden rules
