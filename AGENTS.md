# AI Agent Guide — PageSeeds App

Concise orientation and rules for AI agents working in this repo.

> **Fast path:** If you already know what you're building, see the [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) for concrete file paths and validation steps.  
> **Deep context:** For business workflows, execution mechanics, data architecture, and LLM integration, see the docs listed under [See Also](#see-also).

---

## What This Repo Is

A **Tauri 2 desktop app** — self-contained binary, no Python, no external CLI dependency.

- **Backend**: Rust (`src-tauri/src/`)
- **Frontend**: React + TypeScript + Vite + Tailwind v4 + shadcn/ui (`src/`)
- **Store**: SQLite (runtime state) + JSON in the user's repo (committed content data)
- **LLM layer**: Rig-core providers with a legacy CLI fallback (`src-tauri/src/rig/`, `src-tauri/src/engine/agent.rs`)
- **Not related to `pageseeds-cli`**: business logic is re-implemented here in Rust

---

## Fast Path: Common Scenarios

| If you need to... | Use this path | Do NOT |
|---|---|---|
| **Adjust how an AI writes/reviews content** | Edit or add a skill in `.github/skills/{skill}/SKILL.md` (or embedded defaults in `src-tauri/src/skills/`). Test with `build_prompt_preview` before touching executor logic. | Add a new task type or handler just to change the prompt |
| **Add a new content-writing behavior** | Reuse `write_article` + `ContentHandler` + a `skill` param. | Add a new handler unless the step graph changes |
| **Add or change task lifecycle behavior** | Follow the [Task Lifecycle Contract](#task-lifecycle-contract), then update `config/task_definitions.rs`, `engine/post_actions.rs`, or the user-selection command as appropriate. | Encode lifecycle rules in a component, executor special case, or ad-hoc task factory |
| **Attach tasks to execution** | Use backend queue commands through `tauri.ts` (`enqueueTasks`, `getQueueSnapshot`, `pauseQueue`, `resumeQueue`). | Call `executeTask` directly from components |
| **Programmatically create tasks** | Use `TaskSpawner::spawn` or `TaskSpawner::spawn_follow_up`. | Call `task_store::create_task` directly |
| **Pass downstream context/data** | Use task artifacts + deterministic prep steps. | Make the agent rediscover file paths or metrics from prose |
| **Get JSON from an agent** | Use shared extraction helpers (`engine::text` in Rust; `artifacts.ts` on frontend). | Write new regex parsers or normalizer paths |
| **Sync article state to repo JSON** | Use `db::export::write_articles_to_repo()` or `content::ops::sync_and_validate()`. | Write raw SQL + manual `fs::write` of `articles.json` |
| **Add a new workflow step** | Add `StepKind` variant → register in `step_registry.rs` → add match arm in `executor.rs` → implement in `engine/exec/`. | Put business logic in `commands/` or the handler |
| **Add frontend UI around backend data** | Add command wrapper in `tauri.ts` → add type in `types.ts` → build component in `src/components/`. | Call `invoke()` inline in components |

**Golden rules:**
1. A new skill file is ~20 lines. A new task type + handler + exec module is ~200+ lines. **Prefer the skill.**
2. When the output is an MDX article, reuse `write_article` with a different skill — do not build a new pipeline.
3. The queue is backend-managed. Components enqueue; they do not execute.
4. **NEVER re-implement a primitive that already exists.** Before writing `split_whitespace().count()`, `parse_frontmatter()`, or a slug normalizer, check the [DRY catalog](#dry-core-reusable-functions). The canonical version strips markdown, handles edge cases, and is the single source of truth. Fragmentation creates bugs that surface weeks later in unrelated workflows.

---

## Directory Map

```
src-tauri/src/
├── commands/            # #[tauri::command] bindings — thin wrappers by domain
├── db/                  # SQLite init, migrations, JSON repo export
├── models/              # Serde structs crossing the IPC boundary
├── engine/              # Workflow orchestration: store, spawner, executor, handlers
│   ├── workflows/       # Handler trait + step plans
│   └── exec/            # Step implementations
├── config/              # Constants, task definitions, env resolver
├── content/             # MDX file operations
├── reddit/, gsc/, seo/, social/, clarity/, rig/  # Domain modules
└── lib.rs               # Tauri setup + state

src/
├── lib/
│   ├── bindings/        # Auto-generated TS from Rust (ts-rs)
│   ├── tauri.ts         # All invoke() wrappers
│   └── types.ts         # Re-exports bindings + frontend-only types
└── components/          # Feature-scoped React components
```

For the full tree and per-folder responsibilities, see [`AI_QUICK_START.md`](./AI_QUICK_START.md).

---

## Core Rules

### Rust Backend

1. **Business logic lives in Rust modules** — never in `commands.rs` or the frontend.
2. **`commands.rs` is thin**. Each command validates inputs → calls a module function → returns result. No logic beyond that.
3. **One error type**. Use `error::Error` and `error::Result<T>` throughout. Commands return `Result<T, String>` (Tauri requirement).
4. **SQLite is the runtime store**. All mutable state goes through `engine/task_store.rs`. Schema changes require a new migration constant in `db/mod.rs` — never alter existing SQL migration blocks.
5. **Task creation goes through `engine::spawner::TaskSpawner`**. Never call `task_store::create_task` directly for programmatic task creation. The spawner enforces idempotency and dependency validation.
6. **No subprocess calls**. All I/O uses Rust crates directly (`reqwest`, `rusqlite`, `walkdir`, `regex`, etc.). The only CLI fallback is inside the agent compatibility layer and is legacy.

### RIG / LLM Integration

1. **Use standard `rig-core` primitives first.** Prefer Rig providers/completion models, `Extractor<T>`, `Tool`/tool sets, embeddings, and agents before writing custom HTTP loops, regex JSON extraction, bespoke tool registries, or prompt-only protocols.
2. **Keep Rig behind the local integration layer.** Put provider, extraction, tool, embedding, and agent adapters in `src-tauri/src/rig/` (or the existing domain module that already wraps Rig).
3. **Structured output uses schemas.** New agentic steps that need JSON must define a typed Rust output struct with `serde` + `schemars::JsonSchema` and use a Rig extractor or equivalent typed extraction wrapper.
4. **Tools use Rig tool abstractions.** If a model needs to call Ahrefs, GSC, Reddit, file analysis, or other deterministic capabilities, expose them as typed Rig tools.
5. **Provider fallback is centralized.** Any fallback to `agent-wrapper`, CLI providers, or compatibility shims belongs in the Rig/provider layer and must be documented as temporary.
6. **Test without live providers by default.** Unit tests for Rig-backed code should use fixtures, mock providers, or mocked tools. Live tests must be `#[ignore]`.

### Frontend

1. **Frontend calls Rust**. All data fetching and mutations go through `invoke()` in `src/lib/tauri.ts`. No direct file I/O in React.
2. **Keep `tauri.ts` the single IPC file**. Every new command gets a typed wrapper there.
3. **`types.ts` mirrors Rust models exactly**. When you change a Rust struct, update the corresponding TypeScript interface immediately (or regenerate bindings with `./scripts/sync-bindings.sh`).
4. **UI stack**: Tailwind v4, shadcn/ui primitives (`components/ui/`), Manrope body font, Fraunces display font. See `STYLE_GUIDE.md` for tokens.
5. **All UI must use shadcn/ui** — no raw HTML alternatives. Use `Sheet`, `ScrollArea`, `Textarea`, `Input`, `Button`, `Badge`, `Separator`, `Select`, `Tabs`, `Dialog`, etc.
6. **No business logic in components**. Components render and dispatch. They call `tauri.ts` helpers and display results.

### Secrets

- Precedence: `~/.config/automation/secrets.env` → `{repo}/.env.local` → `{repo}/.env` → shell vars.
- Managed by `config/env_resolver.rs` (`EnvResolver`). Use it everywhere; don't read env vars directly.
- Never embed keys or paths in code. Settings UI writes to the secrets file via `import_env_file` command.

### Settings Architecture

| Type | Table/Module | Examples |
|------|--------------|----------|
| **Global** | `global_settings` table | `agent_provider`, `kimi_backend_mode`, theme defaults |
| **Project** | `projects` table | `seo_provider`, `content_dir`, `site_url` |

Legacy project `agent_provider` values are preserved but ignored.

---

## Task Lifecycle Contract

Before adding or changing anything that creates, queues, reviews, or spawns tasks, identify which lifecycle lane it belongs to.

| Lane | Source of truth | How it works | Reuse |
|---|---|---|---|
| **User starts an existing task** | `src/lib/taskQueueActions.ts`, `src/stores/queueStore.ts`, `src-tauri/src/engine/queue.rs` | Frontend sends `EnqueueItem`s; backend persists queue rows, starts the runner, emits queue events. | `enqueueTasks`, `getQueueSnapshot`, `pauseQueue`, `resumeQueue`, `removeQueueItem` |
| **System creates a task** | `src-tauri/src/engine/spawner.rs` | Build a `TaskSpec`; defaults come from `config/task_definitions.rs`; idempotency prevents duplicate active work. | `TaskSpawner::spawn` |
| **Task creates backend follow-ups after success** | `src-tauri/src/engine/post_actions.rs` | `after_task_success` runs after executor completion and returns created task IDs; queue auto-enqueues only follow-ups whose `run_policy` is `auto_enqueue`. | `TaskSpawner::spawn_follow_up` |
| **Task requires user input before follow-ups** | `config/task_definitions.rs` + review UI + selection command | Parent task gets `review_surface != none` and usually `follow_up_policy = UserSelection`; executor leaves it in `review`; the selection command validates choices, creates downstream tasks, marks parent done. | Existing patterns: keyword picker, Reddit picker, cannibalization picker |
| **Task should only show results, not spawn work** | `config/task_definitions.rs` | Use `artifact_review` and `follow_up_policy = None`. | Existing artifact review surfaces |

**Hard rules:**
- `config/task_definitions.rs` owns `run_policy`, `review_surface`, `follow_up_policy`, and `handler_family`. Do not duplicate those decisions in React state or executor branches.
- Components enqueue; they do not execute.
- Backend follow-ups live in `engine/post_actions.rs` or a domain module called from it.
- User-selection follow-ups must not be spawned before the user chooses. Store selectable options as artifacts, route the completed parent to `review`, and create downstream tasks from the selection command.
- Every generated task needs an idempotency key unless it is intentionally one-off.
- Run `pnpm run check:task-store` after task-creation changes.

---

## Choose Execution Mode Deliberately

Every new workflow step requires an explicit decision. Use these tests:

**Deterministic-First Test:** Could a developer write a finite set of rules that produces the correct output for *all* valid inputs? If yes → deterministic. If the rules would need to understand intent, weigh tradeoffs, or generate prose → agentic.

**Input/Output Test:**
- Structured input → structured output via a computable mapping = **deterministic**
- Any input → prose, or open-ended selection from a large option space = **agentic**
- Structured input → prioritised/recommended subset requiring judgment = **agentic for the selection, deterministic for the execution**

**External API calls are deterministic.** Calling Ahrefs, GSC, Reddit, etc. is deterministic — the API does the computation. The step that *interprets* the API results may be agentic.

**Hybrid Pattern (canonical example: `content_review`):**
1. Deterministic step: collect data, compute metrics, filter, rank, group, format
2. Agentic step: interpret, recommend, write prose, make judgment calls using the structured output from step 1

Never feed raw bulk data to an LLM when a `sort/filter/group_by` could produce a structured summary first. Never hard-code a heuristic where understanding intent is required.

**Minimum viability for an agentic step:**
- Specific input context (task details, artifacts, structured data from prior steps)
- A documented output contract (schema/format in the handler comment or prompt via `output_contract`)
- A comment explaining *why* this cannot be deterministic

If a step lacks all three → it is a placeholder. Use kind `"manual"` instead until it is real.

---

## DRY: Core Reusable Functions

**Before writing new logic, verify the capability does not already exist.** The most common agent mistake in this repo is re-implementing article writing, file I/O, or DB export because the existing function was not discovered.

### Content / Article Operations (`content/`)

| Function | File | Use instead of writing... |
|---|---|---|
| `count_words()` | `content/ops.rs` | `text.split_whitespace().count()` |
| `frontmatter::split_mdx()` | `content/frontmatter.rs` | Ad-hoc `---` scanning |
| `frontmatter::parse()` | `content/frontmatter.rs` | Line-by-line `key: value` HashMap |
| `frontmatter::extract_frontmatter_string()` | `content/frontmatter.rs` | Inline frontmatter field extraction |
| `slug::normalize_url_slug()` | `content/slug.rs` | Custom `slugify()` |
| `slug::strip_numeric_prefix()` | `content/slug.rs` | Inline regex `^\d+_` |
| `date_policy::find_first_free_past_date()` | `content/date_policy.rs` | Backward-cursor date loops |
| `date_policy::suggest_next_safe_date()` | `content/date_policy.rs` | Reading dates and walking backward manually |
| `ingest_orphan_files()` | `content/ops.rs` | A new "save article to DB + disk" function |
| `sync_and_validate()` | `content/ops.rs` | Ad-hoc file discovery or date-sync logic |
| `read_file_metadata()` | `content/ops.rs` | Inline `fs::read_to_string` + regex word counting |
| `load_article_by_slug()` | `content/ops.rs` | Manual article lookup + path construction |
| `resolve_content_dir()` | `content/ops.rs` | Content path guessing |
| `slug_from_filename()` | `content/ops.rs` | String manipulation on filenames |
| `apply_publish()` | `content/publish.rs` | Any publish/status-change workflow |
| `content_health_check()` | `content/ops.rs` | One-off file existence checks in UI code |

### Database / Export (`db/`)

| Function | File | Use instead of writing... |
|---|---|---|
| `export::write_articles_to_repo()` | `db/export.rs` | Any `fs::write` of `articles.json` |
| `export::export_articles()` | `db/export.rs` | Manual SQL → JSON serialization |
| `export::merge_unknown_fields()` | `db/export.rs` | Naive JSON overwrite that drops extra fields |
| `task_store::list_articles()` | `engine/task_store.rs` | Raw SQL `SELECT * FROM articles` |

### Article Writing / Workflow (`engine/`)

| Function / Handler | File | Use instead of writing... |
|---|---|---|
| `ContentHandler` | `engine/workflows/handlers.rs` | A new handler for "write an MDX file" |
| `exec_agentic()` | `engine/workflows/handlers.rs` | Custom agent invocation code |
| `TaskSpawner::spawn()` | `engine/spawner.rs` | Direct `task_store::create_task` calls |
| `TaskSpawner::spawn_follow_up()` | `engine/spawner.rs` | Manual follow-up task logic |
| `compute_next_publish_date()` | `engine/workflows/handlers.rs` | Date math in article writing code |

### Linking / Clustering (`engine/exec/content/`)

| Function | File | Use instead of writing... |
|---|---|---|
| `append_related_section()` | `engine/exec/content/cluster_link.rs` | Inline string manipulation for link sections |
| `exec_cluster_link_scan()` | `engine/exec/content/cluster_link.rs` | Custom file traversal for internal links |

### Per-Article Fix Pipeline (`engine/exec/content/`, `engine/exec/ctr_audit/`)

| Function | File | Use instead of writing... |
|---|---|---|
| `exec_fix_content_article_context()` | `engine/exec/content/fix_context.rs` | Ad-hoc file reading in agentic steps |
| `exec_fix_content_article_generate()` | `engine/exec/content/fix_generate.rs` | Raw agent calls with regex JSON extraction |
| `exec_fix_content_article_apply()` | `engine/exec/content/fix_apply.rs` | Inline string manipulation for file edits |
| `exec_fix_content_article_verify()` | `engine/exec/content/fix_verify.rs` | One-off validation scripts |
| `exec_ctr_analyze`, `exec_ctr_fix_generate`, `exec_ctr_fix_apply`, `exec_ctr_verify_fix` | `engine/exec/ctr_audit/` | Same pattern for CTR fixes |

**The canonical 4-step fix pipeline:** deterministic context → structured `Extractor<T>` generation → deterministic apply with snapshot/restore → deterministic verify.

---

## Layer Responsibilities

There are four places backend logic can live. Knowing which to use removes guesswork.

| Layer | File Pattern | Responsibility |
|-------|--------------|----------------|
| **Commands** | `commands/{domain}.rs` | IPC boundary: validate inputs, lock DB, call a module function, return result. **No business logic.** |
| **Domain modules** | `{domain}/` | Business logic, data access, external API calls |
| **Engine exec** | `engine/exec/{domain}.rs` | Deterministic step implementations called by the executor |
| **Workflow handlers** | `engine/workflows/handlers.rs` | Orchestration / planning: returns `Vec<WorkflowStep>` |

### Decision Tree

```
I have new logic — where does it go?
│
├─ Is it reading request inputs and returning a Tauri response?
│  └─→ commands/{domain}.rs
│
├─ Is it building a step graph for a task type?
│  └─→ engine/workflows/handlers.rs
│
├─ Is it executing a single workflow step?
│  └─→ engine/exec/{domain}.rs
│
└─ Everything else (API clients, parsers, DB access, algorithms)
   └─→ {domain}/
```

The `social/` domain and the `clarity/` domain are the canonical examples of fully modularized domains.

---

## How to Add a Feature

For scenario-specific instructions (changing a skill, adding content behavior, wiring the queue, building a fix pipeline, adding UI), see the [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md).

For workflow mechanics, data persistence, business processes, and LLM integration, see the docs listed under [See Also](#see-also).

At a high level:

1. **Port behavior, not architecture** — identify inputs/outputs first.
2. **Test the agent prompt before writing the executor**.
3. **One end-to-end run before any UI work** — backend must produce correct output first.
4. **Spec before code** — any feature touching 2+ files gets a spec in `docs/` first.
5. **Ship one thing at a time**.

---

## Anti-Pattern: Rebuilding Article Writing

The hub page case study in the old codebase is the canonical mistake: creating a brand-new handler, 7 new step kinds, and ~2,000 lines of duplicated persistence logic for output that is just an MDX article.

When the output is an MDX article, the answer is almost always **reuse `write_article` with a different skill**, not build a new pipeline. See the [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) → "Building a Per-Article Fix Pipeline" and "Scenario: Adding a New Content-Writing Behavior" for the correct patterns.

---

## Pre-Change Checklist

### Rust Backend
- [ ] Checked for reuse against the DRY catalog above
- [ ] Task lifecycle contract checked (if creating/queuing/spawning tasks)
- [ ] `cargo check` passes before touching the frontend
- [ ] `cargo test` passes — especially `all_task_types_have_non_fallback_handler`
- [ ] New SQLite columns added via a new migration, not by altering existing ones
- [ ] Settings placed correctly: user preferences → `global_settings`; project config → `projects`
- [ ] No business logic added to `commands/*.rs`
- [ ] `tauri.ts` wrapper added/updated for any new or changed command
- [ ] `types.ts` updated to match Rust struct changes (or run `./scripts/sync-bindings.sh`)
- [ ] `./scripts/check-bindings.sh` passes if a Rust model with `#[ts(export)]` changed
- [ ] `pnpm run check:ipc` passes
- [ ] No secrets or absolute machine paths in source code
- [ ] No `subprocess` / shell calls outside the agent compatibility layer
- [ ] Reviewed `CONTRACTS.md` for affected implicit contracts
- [ ] New task types added to `config/task_definitions.rs` before wiring handlers
- [ ] Every new agentic step has specific input context, output contract, and a comment explaining why it cannot be deterministic
- [ ] Every new deterministic step does not contain a hard-coded heuristic that substitutes for judgment

### Frontend
- [ ] `pnpm run lint` passes
- [ ] `pnpm exec tsc -b` passes
- [ ] `pnpm test` passes
- [ ] `pnpm run check:ipc` passes
- [ ] `pnpm run build` passes
- [ ] Zustand store accesses use selectors (`useQueueStore(s => s.items)`)
- [ ] Arrays mapped in hooks/components are wrapped in `useMemo`
- [ ] Callbacks passed as JSX props are wrapped in `useCallback`
- [ ] `useQuery` data is used directly — never copied into local state via `useEffect`
- [ ] No components defined inside other components

---

## See Also

- [`docs/AGENT_DEVELOPMENT_PLAYBOOK.md`](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) — Scenario-based development guide
- [`docs/BUSINESS_PROCESSES.md`](./docs/BUSINESS_PROCESSES.md) — What the app does and how workflows connect
- [`docs/WORKFLOW_ENGINE.md`](./docs/WORKFLOW_ENGINE.md) — Handlers, steps, executor, async execution
- [`docs/DATA_PERSISTENCE.md`](./docs/DATA_PERSISTENCE.md) — SQLite/JSON architecture
- [`docs/AGENT_INTEGRATION.md`](./docs/AGENT_INTEGRATION.md) — LLM integration with Rig
- [`AI_QUICK_START.md`](./AI_QUICK_START.md) — Full directory map and quick orientation
- [`CONTRACTS.md`](./CONTRACTS.md) — Runtime invariants and hidden rules
- [`STYLE_GUIDE.md`](./STYLE_GUIDE.md) — Design system and UI tokens
