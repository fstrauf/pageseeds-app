# Reuse And Consolidation Audit

Date: 2026-04-30

This audit looks at why recent PageSeeds work has started to feel patchy and why agents can still reinvent existing behavior. The short version: the repo has many good primitives now, but they are not yet enforced as the easiest path. Several newer features still route around the primitives, and some agent-facing docs point to historical code paths.

## What Is Working

- `AGENTS.md` now has a strong DRY catalog for content operations, task spawning, workflow handlers, linking, audit, and export.
- `WorkflowStep::new` now accepts `StepKind` directly in `src-tauri/src/engine/workflows/mod.rs`, which is better than stringly typed step creation.
- `src-tauri/src/engine/post_actions.rs` exists and has moved many executor side effects out of the main orchestration loop.
- `src-tauri/src/config/task_definitions.rs` is a real single source for task phase, execution mode, review behavior, and handler family.
- Frontend IPC boundaries are mostly clean: `invoke()` is centralized in `src/lib/tauri.ts`, with the logging bridge as the only special case found.

## Main Findings

### 1. Workflow Additions Still Have Too Many Touch Points

Adding a workflow still tends to touch `task_definitions.rs`, `handlers.rs`, `step_kind.rs`, `step_registry.rs`, a domain exec module, and sometimes `executor.rs` or `post_actions.rs`. The `StepKind` enum has grown into a large manual registry, with repeated `as_str` and `FromStr` mappings.

Evidence:

- `src-tauri/src/engine/workflows/step_kind.rs` contains many feature-specific variants for CTR, merge, hub, territory, social, coverage, Reddit, GSC, and content.
- `src-tauri/src/engine/step_registry.rs` manually registers each step kind to a handler closure.
- `src-tauri/src/engine/executor.rs` still special-cases step names like `ctr_build_context`, `merge_extract_sections`, `hub_build_brief`, and `territory_strategy` to decide what becomes `latest_raw_output`.

Recommendation:

- Introduce a typed `StepOutcome` with a `latest_raw` policy: `Preserve`, `ReplaceWithOutput`, or `Clear`.
- Move step registration next to each domain module, or add a small domain registry API so adding a feature does not require editing one giant registry.
- Add a round-trip test for every `StepKind` variant until the mapping is generated with `strum` or a macro.

### 2. The Hub-Page Anti-Pattern Is Documented But Still Live In Code

`AGENTS.md` correctly says hub pages should usually reuse `write_article` plus a skill. But the code still has dedicated hub task definitions, a hub-specific branch in post-actions, and a large `hub_page.rs` module that writes files, inserts SQLite rows, and exports articles directly.

Evidence:

- `src-tauri/src/config/task_definitions.rs` still defines `create_hub_page` and `refresh_hub_page`.
- `src-tauri/src/engine/post_actions.rs` branches on those task types after `content_write_stage`.
- `src-tauri/src/engine/exec/content/hub_page.rs` writes an MDX file, inserts into `articles`, updates `articles_meta`, and exports `articles.json`.

Recommendation:

- Deprecate the custom hub execution path and turn hub creation into a `write_article` task with a `hub-write` skill and a structured `hub_brief` artifact.
- If compatibility must remain, make `create_hub_page` an adapter that creates or runs the normal `write_article` flow instead of owning persistence.
- Delete or quarantine unused `Hub*` step kinds after compatibility is migrated.

### 3. Task Creation Is Not Fully Centralized

`TaskSpawner` is documented as the only programmatic task creation path, but several paths still build `Task` manually and call `task_store::create_task`.

Evidence:

- `src-tauri/src/reddit/spawner.rs` creates `reddit_reply` tasks directly.
- `src-tauri/src/commands/social.rs` creates `social_generate_campaign` tasks directly.
- `src-tauri/src/engine/exec/keywords/auto_spawn.rs` creates `write_article` tasks directly.
- `src-tauri/src/engine/scheduler.rs` creates scheduled tasks directly.

Recommendation:

- Convert all programmatic creation to `TaskSpawner::spawn` or `spawn_follow_up`.
- Keep direct `task_store::create_task` only for the user-facing `create_task` command and tests.
- Add a grep-style CI check that fails on new `task_store::create_task` calls outside an allowlist.

### 4. Article Persistence Has Competing Paths

There are good primitives for article export and projection, but article state changes still happen through several routes: direct SQL updates, `db::export::write_articles_to_repo`, `content::article_index::export_projection`, and occasional `export_articles` plus `std::fs::write`.

Evidence:

- `src-tauri/src/content/dates.rs`, `src-tauri/src/content/publish.rs`, and `src-tauri/src/content/article_index.rs` use `write_articles_to_repo` or projection wrappers.
- `src-tauri/src/engine/exec/content/task_spawner.rs` exports article review state by calling `export_articles` and writing `.github/automation/articles.json` itself.
- `src-tauri/src/engine/exec/content/hub_page.rs` inserts directly into the `articles` table and exports.

Recommendation:

- Create a small article persistence facade, for example `content::article_index_service`, for common mutations: register article, update metadata from disk, update review state, publish, export projection.
- Make direct `articles` SQL updates rare and documented.
- Give agents one obvious entry point for "change article state and sync repo JSON".

### 5. JSON Extraction And Artifact Parsing Are Reimplemented Repeatedly

Rust already has `engine::text::extract_json`, but several modules still have local JSON block extractors. The frontend repeats the same pattern in artifact viewers and task pickers.

Evidence:

- Rust JSON extraction appears in `engine/text.rs`, `engine/keyword_selection.rs`, `social/generator.rs`, `content/publish.rs`, `engine/exec/reddit/config.rs`, and `engine/exec/social/mod.rs`.
- Frontend extraction/parsing appears in `src/components/tasks/KeywordPicker.tsx` and `src/components/workflow/AgentLog.tsx`.

Recommendation:

- Replace local Rust extractors with `engine::text::extract_json` or a typed helper like `extract_json_as<T>()`.
- Add `src/lib/artifacts.ts` for frontend helpers: `extractJsonArtifact`, `parseKeywordResearchArtifact`, `numberFromId`, `formatMetric`, and date range helpers.
- Keep backward compatibility in one parser instead of spreading legacy format handling through components.

### 6. Agent-Facing Docs Are Helpful But Partly Stale

The best instructions are in `AGENTS.md`, but other docs still describe old architecture or missing files. That weakens agent discoverability because agents can follow an old path with confidence.

Evidence:

- `AI_QUICK_START.md` references `QUEUE_DEBUG.md`, which was not present in the repo.
- `AGENTS.md` references `docs/dev-process.md`, which was not present in the repo.
- `docs/WORKFLOW_ENGINE.md` still shows `WorkflowStep.kind: String` and normalizer-centric flows, while current code uses `StepKind` and typed steps.
- `docs/AGENT_INTEGRATION.md` references `engine/normalizer.rs`, which was not present in the current tree.

Recommendation:

- Make `AGENTS.md` the canonical agent entrypoint and have older docs link back to it for current rules.
- Update or remove stale references in `AI_QUICK_START.md`, `docs/WORKFLOW_ENGINE.md`, `docs/AGENT_INTEGRATION.md`, and `CONTRACTS.md`.
- Add a simple docs link check for local Markdown references.

### 7. The Queue Is Only Half Backend-Owned

The queue is currently split in a fragile way. The frontend owns the ordered queue, visibility, pause flag, completed-item cache, follow-up insertion, and auto-restart behavior in Zustand. The backend owns task execution and task status transitions, but `execute_queue` still receives an owned `Vec<QueueItem>` from the frontend. After a reload, the frontend can only reconstruct an approximate queue from tasks whose status is `queued` or `in_progress`.

Evidence:

- `src/stores/queueStore.ts` owns `items`, `isRunning`, `isPaused`, `isVisible`, follow-up insertion, event listener lifecycle, and restart behavior.
- `src/hooks/useQueueRunner.ts` now rehydrates on mount by calling `get_queue_state`, but that state is derived from task statuses rather than a durable queue record.
- `src-tauri/src/commands/executor.rs` exposes `pause_queue`, `resume_queue`, and `clear_completed_queue_items`, but they are placeholders.
- `src-tauri/src/commands/executor.rs` exposes `get_queue_state`, but it returns tasks by status from `task_store::list_all_tasks_by_statuses` rather than a persisted queue table.
- `src-tauri/src/engine/queue_runner.rs` executes the `Vec<QueueItem>` passed by the frontend and does not own enqueue, remove, reorder, pause, resume, or recovery.
- `src-tauri/src/lib.rs` resets `in_progress` tasks to `todo` on startup, which is good crash recovery for tasks but confirms there is no durable queue runtime to resume.
- `docs/TASK_QUEUE.md` explicitly describes the queue as frontend-only state, while the product behavior now needs backend-owned state.

What can be reused:

- `src-tauri/src/engine/queue_runner.rs`: serial execution loop, event emission names, follow-up event shape, and GSC-token handoff.
- `src-tauri/src/engine/executor.rs`: `execute_task_with_token`, step progress event emission, follow-up task return values, and task status finalization.
- `src-tauri/src/engine/task_store.rs`: task lookup/status transitions, lightweight task listing, dependency awareness from batch execution.
- `src-tauri/src/engine/batch.rs`: ready-task selection logic for automatic/batchable tasks with dependency checks.
- `src-tauri/src/engine/runtime.rs`: existing pattern for opening per-thread SQLite connections and running async work safely.
- Existing Tauri events: `queue:task-started`, `queue:task-completed`, `queue:task-failed`, `queue:follow-up-created`, `queue:finished`, plus `task_step_progress`.
- Existing frontend `TaskRunner` component can remain mostly presentational if its props come from a backend-backed cache.

Recommendation:

- Treat task status and queue status as separate concepts. A task can be `todo`, `queued`, `in_progress`, `review`, `done`, or `cancelled`, but the queue also needs order, run membership, pause state, failure policy, visibility, insertion mode, and completed history.
- Add a backend queue module, for example `src-tauri/src/engine/queue.rs`, that owns enqueue/remove/reorder/pause/resume/start and persists queue rows.
- Keep in-memory state only for the live runner handle and pause/cancel flags; persist all queue membership and item state in SQLite so app reopen and frontend reload produce the same snapshot.
- Make the frontend Zustand store a projection cache only. It should subscribe to backend events and call `get_queue_snapshot` on mount or reconnect.

Suggested backend model:

```sql
CREATE TABLE queue_runs (
	id TEXT PRIMARY KEY,
	status TEXT NOT NULL,              -- idle | running | paused | finished | failed
	pause_on_error INTEGER NOT NULL DEFAULT 1,
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	started_at TEXT,
	finished_at TEXT
);

CREATE TABLE queue_items (
	run_id TEXT NOT NULL,
	position INTEGER NOT NULL,
	task_id TEXT NOT NULL,
	project_id TEXT NOT NULL,
	status TEXT NOT NULL,              -- pending | running | completed | failed | skipped
	error TEXT,
	result_json TEXT,
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	PRIMARY KEY (run_id, task_id),
	FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
	FOREIGN KEY (run_id) REFERENCES queue_runs(id) ON DELETE CASCADE
);
```

Suggested commands:

- `get_queue_snapshot() -> QueueSnapshot`
- `enqueue_tasks(items, mode)` where mode is `append` or `next`
- `remove_queue_item(task_id)` for pending items only
- `pause_queue()` sets backend pause flag and pauses after the current task
- `resume_queue()` clears pause and starts the runner if idle
- `clear_completed_queue_items()` removes completed/failed/skipped rows from the active run
- `dismiss_queue()` hides or archives a finished run without mutating task history

Suggested runner shape:

1. `enqueue_tasks` writes `queue_items`, marks tasks `queued`, and calls `ensure_runner_started`.
2. `ensure_runner_started` checks an in-memory `JoinHandle`/running flag under a mutex and spawns one Tokio task if idle.
3. The runner repeatedly leases the next pending queue item from SQLite, marks it `running`, then calls `execute_task_with_token`.
4. On completion, it writes `result_json`, updates queue item status, updates the task status through the existing executor, emits events, and auto-enqueues automatic/batchable follow-ups in the backend.
5. If `pause_queue` was called, the runner stops after the current item and leaves remaining rows as `pending`.
6. On app startup, any `queue_items.status = 'running'` and `tasks.status = 'in_progress'` are marked recoverable (`pending`/`todo` or `failed` depending on policy), then the UI snapshot shows what happened.

Frontend migration:

- Keep `TaskRunner` as a view component.
- Replace `queueStore.enqueue` local mutation with a call to `enqueue_tasks`.
- Replace `items/isRunning/isPaused` as source-of-truth state with `get_queue_snapshot` plus event updates.
- Keep Zustand only as a local cache of the latest backend snapshot and UI preferences such as expanded rows.
- On mount, app focus, and event reconnect, call `get_queue_snapshot` so missed events self-heal.

Open design choices:

- Whether there is one active queue run globally or one queue run per app window. The current product model is one global app queue.
- Whether completed items should persist forever, be archived with the run, or be pruned after N items/days.
- Whether pause should pause immediately before the next step or only between tasks. The current runner shape is easiest and safest if pause happens between tasks.
- Whether app restart should automatically resume pending queue items or require the user to press Resume. Safer default: restore the queue as paused after a crash/reopen.

## Suggested Consolidation Plan

### Phase 1: Guardrails First

- Add an allowlisted grep check for direct `task_store::create_task` calls.
- Add a docs local-link check for Markdown files.
- Add `StepKind` round-trip tests if not already present.
- Add a repo-memory or AGENTS reminder that `create_hub_page` is legacy and must not be copied.

### Phase 2: Centralize Small Utilities

- Create Rust helpers for typed JSON extraction: `extract_json_as<T>()` and `extract_json_string()`.
- Migrate social, Reddit config, publish, and keyword selection to the shared helper.
- Create frontend `src/lib/artifacts.ts` and move keyword artifact parsing, JSON extraction, bigint/number coercion, date helpers, and metric formatting into it.

### Phase 3: Unify Task Creation

- Convert Reddit reply task creation, social campaign tasks, keyword auto-spawn, and scheduler tasks to `TaskSpawner`.
- Use idempotency keys for every generated task.
- Keep `task_store::create_task` as low-level persistence only.

### Phase 4: Article Persistence Facade

- Add one facade for article mutations that must sync SQLite and repo JSON.
- Move content review state sync, hub article registration, publish, date repair, and orphan ingestion behind the facade.
- Make `db::export::write_articles_to_repo` an implementation detail for most callers.

### Phase 5: Workflow Outcome Cleanup

- Introduce `StepOutcome` or extend `StepResult` with artifact and `latest_raw` policy.
- Remove executor step-name special cases.
- Move domain-specific step registration into domain modules or smaller registries.

### Phase 6: Backend-Owned Queue

- Add `queue_runs` and `queue_items` migrations.
- Add a backend `engine::queue` module that owns enqueue, remove, pause, resume, snapshot, and runner startup.
- Change `queue_runner` to lease items from SQLite instead of consuming a frontend-owned `Vec<QueueItem>`.
- Auto-enqueue backend-created follow-up tasks from the runner/post-task path, not from frontend event handlers.
- Make Zustand a projection cache and update `docs/TASK_QUEUE.md` to describe the backend as source of truth.

### Phase 7: Retire Legacy Hub Pipeline

- Convert approved hub recommendations to normal `write_article` tasks with `hub-write` skill and structured artifacts.
- Keep `create_hub_page` only as a compatibility adapter during migration.
- Remove unused `Hub*` step kinds and direct hub persistence once no tasks depend on them.

## Practical Principle For Future Work

Before adding a task type, step kind, parser, or persistence function, agents should answer one question in the spec or PR summary:

> Which existing primitive did I try to reuse, and why was it insufficient?

If that answer is missing, the default should be to pause and search before writing new code.