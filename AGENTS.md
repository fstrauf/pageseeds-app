# AI Agent Guide — PageSeeds App

Concise reference for AI agents adding or maintaining features in this repo.

> **Fast path:** If you know what you're building, skip to the [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) for scenario-specific instructions. Read this file for architecture, rules, and reference material.

---

## What This Repo Is

A **Tauri 2 desktop app** — self-contained binary, no Python, no external CLI dependency.

- **Backend**: Rust (`src-tauri/src/`)
- **Frontend**: React + TypeScript + Vite + Tailwind v4 + shadcn/ui (`src/`)
- **Store**: SQLite (runtime state) + JSON in the user's repo (committed content data)
- **Not related to `pageseeds-cli`**: business logic is re-implemented here in Rust, not imported

---

## Read This First: Common Development Moves

**Before you open any file, find your scenario:**

| If you need to... | Use this path | Do NOT |
|---|---|---|
| **Adjust how an AI writes/reviews content** | Edit or add a skill in `.github/skills/{skill}/SKILL.md` (or embedded defaults in `src-tauri/src/skills/`). Test with `build_prompt_preview` before touching executor logic. | Add a new task type or handler just to change the prompt |
| **Add a new content-writing behavior** | Reuse `write_article` + `ContentHandler` + a `skill` param. | Add a new handler unless the step graph changes |
| **Add or change task lifecycle behavior** | Follow the Task Lifecycle Contract below, then update `config/task_definitions.rs`, `engine/post_actions.rs`, or the user-selection command as appropriate. | Encode lifecycle rules in a component, executor special case, or ad-hoc task factory |
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

---

## Directory Map

```
src-tauri/src/
├── main.rs              # entry point — no logic here
├── lib.rs               # Tauri setup, plugin registration, state management
├── commands/            # #[tauri::command] bindings — organized by domain
│   ├── mod.rs           # AppState, GscState, SeoState definitions
│   ├── settings.rs      # Global + project settings commands
│   ├── projects.rs      # Project CRUD
│   ├── tasks.rs         # Task CRUD
│   ├── articles.rs      # Article queries
│   ├── engine.rs        # Workflow execution commands
│   ├── executor.rs      # Task execution
│   ├── gsc.rs           # Google Search Console commands
│   ├── reddit.rs        # Reddit opportunity commands
│   ├── seo.rs           # SEO research commands
│   ├── content.rs       # Content health commands
│   ├── social.rs        # Social media commands
│   ├── skills.rs        # Skill management
│   └── logging.rs       # Logging commands
├── error.rs             # Central Error enum + Result<T> alias
├── db/
│   ├── mod.rs           # SQLite init + schema migrations (versioned SQL constants)
│   ├── export.rs        # Read/write articles.json and task_list.json in the user's repo
│   └── global_settings.rs # Global app settings (agent_provider, etc.)
├── models/              # Pure serde structs — no logic
│   ├── task.rs          # Task, TaskArtifact, TaskRun (#[ts(export)])
│   ├── article.rs       # Article metadata (#[ts(export)])
│   ├── project.rs       # Project config (#[ts(export)])
│   ├── reddit.rs        # RedditOpportunity, ReplyStatus (#[ts(export)])
│   ├── gsc.rs           # GSC types (#[ts(export)])
│   └── social.rs        # Social media types (#[ts(export)])
├── engine/              # Workflow orchestration
│   ├── task_store.rs    # CRUD against SQLite tasks/projects tables
│   ├── spawner.rs       # CENTRALIZED task creation — idempotent follow-ups
│   ├── executor.rs      # Runs a task: finds handler → plans steps → executes
│   ├── batch.rs         # Autonomous batch execution loop
│   ├── scheduler.rs     # Scheduled rule evaluation + auto task creation
│   ├── ledger.rs        # Append-only execution history (JSONL)
│   ├── agent.rs         # LLM provider calls (Kimi / Copilot)
│   ├── normalizer.rs    # Parse agent raw output → structured JSON
│   ├── post_actions.rs  # Domain-specific post-step / post-task side effects
│   ├── skills.rs        # Load SKILL.md files from the user's repo
│   ├── prompts.rs       # Prompt assembly
│   ├── project_paths.rs # Resolve content dir, automation dir, output dir per project
│   └── workflows/
│       ├── mod.rs       # WorkflowStep struct + StepResult
│       └── handlers.rs  # WorkflowHandler trait + one impl per task family
├── config/
│   ├── mod.rs           # Constants, re-exports
│   ├── task_definitions.rs # Single source of truth for task type metadata
│   └── env_resolver.rs  # Secrets + env file loading with precedence chain
├── content/             # MDX file operations
│   ├── locator.rs       # Find content directory (project override → heuristics)
│   ├── cleaner.rs       # Validate/fix MDX structure
│   ├── dates.rs         # Analyze/fix frontmatter date distribution
│   ├── linking.rs       # Scan internal links, detect gaps
│   └── ops.rs           # Sync, slug generation, frontmatter I/O
├── reddit/
│   ├── mod.rs           # Post struct, API constants
│   ├── search.rs        # Reddit JSON API search
│   └── db.rs            # Opportunity CRUD + reply validation (SQLite)
├── gsc/                 # Google Search Console
│   ├── auth.rs          # Service account + OAuth2 flows
│   ├── client.rs        # Authenticated reqwest client
│   ├── analytics.rs     # Page/query metrics, movers
│   ├── indexing.rs      # URL inspection API + classification
│   ├── coverage.rs      # 404 detection + categorization
│   ├── redirects.rs     # Redirect analysis + classification
│   └── reports.rs       # JSON/CSV/markdown report generation
└── seo/
    ├── keywords.rs      # Ahrefs keyword generator + difficulty
    ├── backlinks.rs     # Backlink analysis + signature caching
    └── traffic.rs       # Traffic lookup (CapSolver + Ahrefs)

src/
├── lib/
│   ├── bindings/        # Auto-generated TypeScript from Rust (ts-rs)
│   ├── tauri.ts         # All invoke() wrappers — one function per command
│   └── types.ts         # Re-exports bindings + frontend-only types
└── components/          # Feature-scoped React components
    ├── ui/              # shadcn/ui primitives only
    ├── tasks/           # TaskBoard, TaskDetail, TaskCreate
    ├── articles/        # ArticleTable, ContentHealth
    ├── reddit/          # OpportunityFeed, ReplyDraft, RedditStats
    ├── gsc/             # GSCDashboard, IndexingReport, CoverageView
    ├── seo/             # KeywordResearch, BacklinkView, TrafficOverview
    ├── projects/        # ProjectSwitcher, ProjectSettings
    ├── skills/          # SkillBrowser
    └── settings/        # SecretsManager, SchedulerConfig
```

---

## Core Rules

### Rust backend
1. **Business logic lives in Rust modules** — never in `commands.rs` or the frontend.
2. **`commands.rs` is thin**. Each command does: validate inputs → call a module function → return result. No logic beyond that.
3. **One error type**. Use `error::Error` and `error::Result<T>` throughout. Commands return `Result<T, String>` (Tauri requirement).
4. **SQLite is the runtime store**. All mutable state goes through `engine/task_store.rs`. Schema changes require a new migration constant in `db/mod.rs` — never alter existing SQL migration blocks.
5. **Task creation goes through `engine::spawner::TaskSpawner`**. Never call `task_store::create_task` directly for programmatic task creation. The spawner enforces idempotency (preventing duplicate follow-up tasks) and dependency validation. Use `TaskSpawner::spawn()` for general creation or `TaskSpawner::spawn_follow_up()` for follow-up tasks.
6. **No subprocess calls**. All I/O uses Rust crates directly (`reqwest`, `rusqlite`, `walkdir`, `regex`, etc.).
6. **Independent but isolated codebase**. Do not share code with `pageseeds-cli`. If a Python module needs porting, re-implement it cleanly in Rust.

### Task Lifecycle Contract

Before adding or changing anything that creates, queues, reviews, or spawns tasks, identify which lifecycle lane it belongs to. Put the answer in the spec, PR summary, or implementation note.

| Lane | Source of truth | How it works | Reuse |
|---|---|---|---|
| **User starts an existing task** | `src/lib/taskQueueActions.ts`, `src/stores/queueStore.ts`, `src-tauri/src/engine/queue.rs` | Frontend sends `EnqueueItem`s with `enqueueTasks`; backend persists queue rows, starts the runner, emits queue events. | `enqueueTasks`, `getQueueSnapshot`, `pauseQueue`, `resumeQueue`, `removeQueueItem` |
| **System creates a task** | `src-tauri/src/engine/spawner.rs` | Build a `TaskSpec`; defaults come from `config/task_definitions.rs`; idempotency prevents duplicate active work. | `TaskSpawner::spawn` |
| **Task creates backend follow-ups after success** | `src-tauri/src/engine/post_actions.rs` | `after_task_success` runs after executor completion and returns created task IDs; queue auto-enqueues only follow-ups whose `run_policy` is `auto_enqueue`. | `TaskSpawner::spawn_follow_up` or `TaskSpawner::spawn` with an idempotency key |
| **Task requires user input before follow-ups** | `config/task_definitions.rs` + a review UI + a selection command | The parent task gets `review_surface != none` and usually `follow_up_policy = UserSelection`; executor leaves it in `review`; the selection command validates user choices, creates downstream tasks, then marks the parent done. | Existing patterns: keyword picker, Reddit picker, cannibalization picker |
| **Task should only show results, not spawn work** | `config/task_definitions.rs` | Use a review surface such as `artifact_review` and `follow_up_policy = None`; do not create queue items from the UI unless the user explicitly enqueues a task. | Existing artifact review surfaces |

Hard rules:
- `config/task_definitions.rs` owns `run_policy`, `review_surface`, `follow_up_policy`, and `handler_family`. Do not duplicate those decisions in React state or executor branches.
- Components enqueue; they do not execute. Use `src/lib/taskQueueActions.ts` or the queue context/store wrappers, which call backend queue commands.
- Backend follow-ups live in `engine/post_actions.rs` or a domain module called from it. The generic executor should stay an orchestrator.
- User-selection follow-ups must not be spawned before the user chooses. Store selectable options as artifacts, route the completed parent to `review`, and create downstream tasks from the selection command.
- Every generated task needs an idempotency key unless it is intentionally one-off. Active `todo`, `queued`, `in_progress`, and `review` tasks count as duplicates.
- Run `pnpm run check:task-store` after task-creation changes.

7. **Choose execution mode deliberately.** Every new workflow step requires an explicit decision. Use the tests below — if you cannot answer them, go back to the design.

   **The Deterministic-First Test:** Could a developer write a finite set of rules that produces the correct output for *all* valid inputs? If yes → deterministic. If the rules would need to understand intent, weigh tradeoffs between equally valid options, or generate prose → agentic.

   **The Input/Output Test:**
   - Structured input → structured output via a computable mapping = **deterministic**
   - Any input → prose, or open-ended selection from a large option space = **agentic**
   - Structured input → prioritised/recommended subset requiring judgment = **agentic for the selection, deterministic for the execution**

   **External API calls are deterministic.** Calling Ahrefs, GSC, Reddit, etc. is deterministic — the API does the computation. The step that *calls the API* is deterministic. The step that *interprets the API results* may be agentic.

   **The Hybrid Pattern (canonical example: `content_review`).** Most real workflows contain both aspects. The correct pattern is always:
   1. Deterministic step: collect data, compute metrics, filter, rank, group, format
   2. Agentic step: interpret, recommend, write prose, make judgment calls using the structured output from step 1
   
   Never feed raw bulk data to an LLM when a `sort/filter/group_by` could produce a structured summary first. Never hard-code a heuristic where understanding intent is required.

   **Minimum viability for an agentic step.** An agentic step MUST have:
   - Specific input context (task details, artifacts, structured data from prior steps)
   - A documented output contract (schema/format in the handler comment or in the prompt via `output_contract`)
   - A comment explaining *why* this cannot be deterministic

   If a step lacks all three → it is a placeholder, not a feature. Use kind `"manual"` instead until it is real.

   **Do not add deterministic fallbacks that reinterpret ambiguous brief text.** When an agentic selection step is required, it must run. A hard-coded heuristic that extracts themes from a brief is fake intelligence — it will silently produce wrong answers on inputs it was never tested against.

### RIG / LLM integration
1. **Use standard `rig-core` primitives first.** For new LLM work, prefer Rig providers/completion models, `Extractor<T>`, `Tool`/tool sets, embeddings, and agents before writing custom HTTP loops, regex JSON extraction, bespoke tool registries, or prompt-only protocols.
2. **Keep Rig behind the local integration layer.** Put provider, extraction, tool, embedding, and agent adapters in `src-tauri/src/rig/` (or the existing domain module that already wraps Rig). Do not scatter direct Rig setup across commands, React, or unrelated exec modules.
3. **Structured output uses schemas.** New agentic steps that need JSON must define a typed Rust output struct with `serde` + `schemars::JsonSchema` and use a Rig extractor or equivalent typed extraction wrapper. Do not add new normalizer regex paths unless preserving a legacy workflow.
4. **Tools use Rig tool abstractions.** If a model needs to call Ahrefs, GSC, Reddit, file analysis, or other deterministic capabilities, expose them as typed Rig tools with explicit argument/result structs. Do not build another string-parsed tool calling loop.
5. **Provider fallback is centralized.** Any fallback to `agent-wrapper`, CLI providers, or compatibility shims belongs in the Rig/provider layer and must be documented as temporary. Workflow steps should not choose subprocess paths directly.
6. **Test without live providers by default.** Unit tests for Rig-backed code should use fixtures, mock providers, or mocked tools. Tests that require real Kimi/OpenAI/Claude credentials, local machine paths, or external APIs must be `#[ignore]` and clearly named as live smoke tests.

### Frontend
1. **Frontend calls Rust**. All data fetching and mutations go through `invoke()` in `src/lib/tauri.ts`. No direct file I/O in React.
2. **Keep `tauri.ts` the single IPC file**. Every new command gets a typed wrapper in `tauri.ts`. Don't call `invoke()` inline in components.
3. **`types.ts` mirrors Rust models exactly**. When you change a Rust struct, update the corresponding TypeScript interface immediately.
4. **UI stack**: Tailwind v4, shadcn/ui primitives (`components/ui/`), Manrope body font, Fraunces display font. See `STYLE_GUIDE.md` for tokens.
5. **All UI must use shadcn/ui — no raw HTML alternatives**. Every panel, overlay, form field, scroll container, and layout primitive must use the corresponding shadcn component: `Sheet`/`SheetContent`/`SheetHeader`/`SheetTitle`/`SheetDescription`/`SheetFooter`/`SheetClose` for side panels; `ScrollArea` for scrollable regions; `Textarea` for multi-line inputs; `Input`, `Label`, `Button`, `Badge`, `Separator`, `Select`, `Tabs`, `Dialog` etc. Do not use raw `<div>` wrappers as sheet shells, raw `<textarea>`, or custom close buttons — use shadcn primitives and `SheetClose asChild`.
6. **No business logic in components**. Components render and dispatch. They call `tauri.ts` helpers and display the results.

### Secrets
- Precedence: `~/.config/automation/secrets.env` → `{repo}/.env.local` → `{repo}/.env` → shell vars.
- Managed by `config/env_resolver.rs` (`EnvResolver`). Use it everywhere; don't read env vars directly.
- Never embed keys or paths in code. Settings UI writes to the secrets file via `import_env_file` command.

### Settings Architecture

Settings are split into **Global** (user preferences) and **Project** (project-specific config):

| Type | Table/Module | Examples | Access |
|------|--------------|----------|--------|
| **Global** | `global_settings` table | `agent_provider` (kimi/copilot/claude), future: theme, defaults | `db::global_settings` |
| **Project** | `projects` table | `seo_provider`, `content_dir`, `site_url` | `engine::task_store` |

**Global Settings:**
- Apply to ALL projects (user tool preference)
- Stored in `global_settings` table (key/value)
- Default `agent_provider` is `"kimi"`
- Use `db::global_settings::get_agent_provider()` / `set_agent_provider()`

**Project Settings:**
- Each project has independent values
- Stored in `projects` table
- Legacy `agent_provider` column exists but is ignored (backward compatibility)

**Migration Note:** Migration V14 creates `global_settings` and initializes `agent_provider` to `"kimi"`. Legacy project `agent_provider` values are preserved but ignored.

---

## Layer Responsibilities

There are four places backend logic can live. Knowing which to use removes guesswork when adding features.

| Layer | File Pattern | Responsibility | What goes here |
|-------|--------------|----------------|----------------|
| **Commands** | `commands/{domain}.rs` | IPC boundary | Validate inputs, lock DB, call a module function, return result. **No business logic.** |
| **Domain modules** | `{domain}/` | Business logic, data access, external API calls | Ahrefs client, Reddit search, GSC auth, content parsing, etc. |
| **Engine exec** | `engine/exec/{domain}.rs` | Deterministic step implementations | Code called by the executor during workflow runs. One function per `StepKind`. |
| **Workflow handlers** | `engine/workflows/handlers.rs` | Orchestration / planning | Returns `Vec<WorkflowStep>` for a task. Never executes. |

### Decision Tree: Where Does My Logic Go?

```
I have new logic — where does it go?
│
├─ Is it reading request inputs and returning a Tauri response?
│  └─→ commands/{domain}.rs (thin wrapper)
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

### Reference Implementation

The `social/` domain (`src-tauri/src/social/` and `src-tauri/src/engine/exec/social.rs`) is the canonical example of a fully modularized domain:
- `social/` — models, DB access, templates, prompts, image generation
- `engine/exec/social.rs` — one `exec_social_*` function per step kind
- `engine/workflows/handlers.rs` — `SocialHandler` plans the step sequence
- `commands/social.rs` — thin command wrappers that validate and delegate

---

## DRY: Core Reusable Functions

**Before writing new logic, verify the capability does not already exist.** The most common agent mistake in this repo is re-implementing article writing, file I/O, or DB export because the existing function was not discovered.

### Content / Article Operations (`content/`)

| Function | File | What it does | Use instead of writing... |
|---|---|---|---|
| `ingest_orphan_files()` | `content/ops.rs` | Discovers untracked MDX files, assigns next ID, inserts into SQLite, exports `articles.json` | A new "save article to DB + disk" function |
| `sync_and_validate()` | `content/ops.rs` | Cross-checks SQLite ↔ MDX files, patches dates, finds orphans | Ad-hoc file discovery or date-sync logic |
| `read_file_metadata()` | `content/ops.rs` | Reads frontmatter + counts words from any MDX file | Inline `std::fs::read_to_string` + regex word counting |
| `load_article_by_slug()` | `content/ops.rs` | DB lookup → resolve content dir → read file | Manual article lookup + path construction |
| `resolve_content_dir()` | `content/ops.rs` | Finds content directory (project override → heuristics) | Any content path guessing logic |
| `slug_from_filename()` | `content/ops.rs` | Strips numeric prefix, returns slug | String manipulation on filenames |
| `apply_publish()` | `content/publish.rs` | Transitions articles to published, assigns safe dates, syncs frontmatter, exports JSON | Any publish/status-change workflow |
| `content_health_check()` | `content/ops.rs` | Read-only sync check for UI health display | One-off file existence checks in UI code |

### Database / Export (`db/`)

| Function | File | What it does | Use instead of writing... |
|---|---|---|---|
| `export::write_articles_to_repo()` | `db/export.rs` | Exports SQLite articles → `.github/automation/articles.json` | Any `fs::write` of articles.json |
| `export::export_articles()` | `db/export.rs` | Returns articles JSON string (no disk write) | Manual SQL → JSON serialization |
| `export::merge_unknown_fields()` | `db/export.rs` | Preserves custom fields (e.g. `gsc`) across export rounds | Naive JSON overwrite that drops extra fields |
| `task_store::list_articles()` | `engine/task_store.rs` | Lists all articles for a project | Raw SQL `SELECT * FROM articles` |

### Article Writing / Workflow (`engine/`)

| Function / Handler | File | What it does | Use instead of writing... |
|---|---|---|---|
| `ContentHandler` | `engine/workflows/handlers.rs` | Plans `write_article`, `optimize_article`, `create_content` as a single agentic step | A new handler for "write an MDX file" |
| `exec_agentic()` | `engine/workflows/handlers.rs` | Loads skill, builds prompt, calls agent, enforces MDX naming, returns raw output | Custom agent invocation code |
| `TaskSpawner::spawn()` | `engine/spawner.rs` | Creates tasks with idempotency, dependency validation | Direct `task_store::create_task` calls |
| `TaskSpawner::spawn_follow_up()` | `engine/spawner.rs` | Idempotent follow-up creation | Manual follow-up task logic |
| `compute_next_publish_date()` | `engine/workflows/handlers.rs` | Finds the next safe past date for publishing | Date math in article writing code |

### Linking / Clustering (`engine/exec/content/`)

| Function | File | What it does | Use instead of writing... |
|---|---|---|---|
| `append_related_section()` | `engine/exec/content/cluster_link.rs` | Appends "Related Articles" section to MDX | Inline string manipulation for link sections |
| `exec_cluster_link_scan()` | `engine/exec/content/cluster_link.rs` | Native Rust link graph scan | Custom file traversal for internal links |

### Validation / Audit (`engine/exec/`)

| Function | File | What it does | Use instead of writing... |
|---|---|---|---|
| `exec_content_audit()` | `engine/exec/content_audit.rs` | 13-rule deterministic audit | One-off frontmatter/check structure validators |

### Per-Article Fix Pipeline (`engine/exec/content/`)

| Function | File | What it does | Use instead of writing... |
|---|---|---|---|
| `exec_fix_content_article_context()` | `engine/exec/content/fix_context.rs` | Deterministic: loads recommendations + file content for a single article | Ad-hoc file reading in agentic steps |
| `exec_fix_content_article_generate()` | `engine/exec/content/fix_generate.rs` | Agentic: structured `ContentFixPatch` extraction via Rig | Raw agent calls with regex JSON extraction |
| `exec_fix_content_article_apply()` | `engine/exec/content/fix_apply.rs` | Deterministic: applies typed patch to MDX with snapshot/restore | Inline string manipulation for file edits |
| `exec_fix_content_article_verify()` | `engine/exec/content/fix_verify.rs` | Deterministic: re-runs health checks after fixes | One-off validation scripts |

**CTR equivalents** (same pattern, older codebase): `exec_ctr_analyze`, `exec_ctr_fix_generate`, `exec_ctr_fix_apply`, `exec_ctr_verify_fix` in `engine/exec/ctr_audit/`.

---

## How to Add a Feature

### New Rust module (e.g. a new data source)

1. Create `src-tauri/src/{domain}/` with `mod.rs` + focused `.rs` files.
2. Declare the module in `lib.rs`: `mod {domain};`
3. Add data types in `src-tauri/src/models/` if they need to cross the IPC boundary.
4. Add `#[tauri::command]` function(s) to `commands.rs` (thin wrapper only).
5. Register the command in `lib.rs` inside `tauri::generate_handler![...]`.
6. Add the typed wrapper to `src/lib/tauri.ts`.
7. Add the TypeScript type to `src/lib/types.ts`.
8. Build the React component in `src/components/{domain}/`.

### New SQLite table

1. Add a new migration constant in `db/mod.rs` (e.g. `MIGRATION_V2`) with all DDL.
2. Apply it in the `init()` function after existing migrations using `conn.execute_batch(MIGRATION_V2)?`.
3. Add CRUD functions in the relevant engine or module file — not in `commands.rs`.

### New workflow task type

#### Step 0 — Before you create a new task type, answer these questions

**If any answer is "yes", you probably do NOT need a new task type.**

| Question | If yes → |
|---|---|
| Does the output go into the content directory as an MDX file? | Reuse `write_article` with a different `skill` parameter. Do not create a new handler. |
| Does it apply structured recommendations to existing MDX files? | Reuse `fix_content_article`. |
| Does it add/remove internal links between articles? | Reuse `cluster_and_link` (or its deterministic steps). |
| Does it read GSC data and spawn fix tasks? | Reuse `collect_gsc` or `indexing_diagnostics`. |
| Does it audit article health? | Reuse `content_audit` or `content_review`. |
| Does it analyze keyword coverage? | It is now an invisible prerequisite — see "Invisible Prerequisites" below. |
| Is the only difference from an existing task the prompt/skill used? | Reuse the existing handler — change the skill param, not the handler. |

**Rule:** A new skill file (`.github/skills/{skill_name}/SKILL.md`) is ~20 lines. A new task type + handler + exec module is ~200+ lines. Prefer the skill.

#### If you still need a new task type

1. Register the task type in `config/task_definitions.rs` (phase, execution mode, review behavior, handler family).
2. Add a `WorkflowHandler` impl in `engine/workflows/handlers.rs`.
3. Register it in `default_handlers()` (same file).
4. Each handler only returns a `Vec<WorkflowStep>` — no execution logic.
5. Execution runs through `engine/executor.rs` unchanged.

**Step constructors are typed:** Use `WorkflowStep::new("name", StepKind::X)` — never pass string step kinds.

**If the new task type is a per-article fix pipeline** (parent audits → child fixes individual items), follow the canonical 4-step pattern documented in `docs/AGENT_DEVELOPMENT_PLAYBOOK.md` → **"Building a Per-Article Fix Pipeline"**. Do not use a bare `StepKind::Agentic` with no skill — it will timeout or produce garbage. Use deterministic context → structured extraction (`Extractor<T>`) → deterministic apply → deterministic verify.

### Invisible Prerequisites (Support Tasks)

Some tasks exist only to produce data artifacts consumed by other tasks. When a task has **no independent user value** and **only one or two consumers**, it should not appear in the UI as a standalone task. Instead, it becomes an **invisible prerequisite** that runs automatically when needed.

**Canonical example: keyword coverage**

`analyze_keyword_coverage` used to be a standalone task with its own handler, UI panel, and manual trigger. It was converted to an invisible prerequisite because:
- Users never ran it for its own sake — they only ran it because `research_keywords` required the output
- It had exactly one consumer family (research tasks)
- It was lightweight (one deterministic load + one agentic clustering step)

**How to convert a support task to an invisible prerequisite:**

1. **Remove standalone task infrastructure:**
   - Delete the task type from `config/task_definitions.rs`
   - Delete the `WorkflowHandler` from `engine/workflows/handlers.rs`
   - Delete the UI panel/component
   - Delete the Tauri command and `tauri.ts` wrapper
   - Remove from task creation menus (`Overview.tsx`, etc.)

2. **Add a freshen step to consumers:**
   - Add `StepKind::EnsureXxxFresh` variant to `engine/workflows/step_kind.rs`
   - Register the exec function in `engine/step_registry.rs`
   - Implement the exec function in the domain module (e.g. `engine/exec/coverage.rs`):
     ```rust
     pub(crate) fn exec_ensure_coverage_fresh(
         task: &Task,
         project_path: &str,
         agent_provider: &str,
     ) -> StepResult {
         if is_coverage_fresh(project_path, 7) {
             return StepResult::success("Coverage data is fresh");
         }
         // Inline: run the full analysis synchronously
         let load = exec_coverage_load_articles(task, project_path)?;
         let cluster = exec_coverage_cluster_analysis(task, project_path, agent_provider, &load.output.unwrap())?;
         exec_coverage_save(task, project_path)
     }
     ```
   - Add the step as **step 1** in every handler that needs the artifact:
     ```rust
     WorkflowStep::new("ensure_coverage_fresh", StepKind::EnsureCoverageFresh)
     ```

3. **Remove hard failures from downstream steps:**
   - Replace `"artifact not found. Run X first"` errors with graceful fallbacks or no-ops
   - The ensure step guarantees freshness, so downstream steps should assume the artifact exists

**Rules:**
- Only do this for tasks with **no user-facing value** and **≤2 consumers**
- The ensure step MUST be deterministic (no agent calls in the freshness check itself)
- The ensure step MAY trigger agentic work inline if the artifact is stale
- Keep the artifact file and the read helper (`read_keyword_coverage`) — only the task infrastructure is removed
- If a support task grows more than 2 consumers, consider making it a generic pre-flight hook instead

**Threshold for generalization:** If you find yourself adding a 3rd invisible prerequisite, stop and build a generic artifact-freshen layer in the executor rather than hand-rolling `EnsureXxxFresh` steps.

---

## Anti-Pattern Case Study: Hub Page Creation

**What happened:** An agent implemented `create_hub_page` as a completely new workflow task type with its own handler (`HubPageHandler`), 7 new step kinds, and a ~2,000-line execution module (`engine/exec/content/hub_page.rs`).

**Why it was wrong:** The hub page is just an article. It is written to the content directory as an MDX file, registered in SQLite, exported to `articles.json`, and linked to other articles. All of this already existed.

| Hub page "needed" | What already existed |
|---|---|
| Write MDX content via agent | `write_article` task + `ContentHandler` |
| Register new article in SQLite + assign ID | `content::ops::ingest_orphan_files()` |
| Export to `articles.json` | `db::export::write_articles_to_repo()` |
| Add "Related Articles" links | `cluster_and_link` task + `append_related_section()` |
| Validate frontmatter / word count | `content_audit` + `content::ops::read_file_metadata()` |

**What the correct implementation should have been:**

1. **Skill-only approach (preferred):** Create a `hub-write` skill (`.github/skills/hub-write/SKILL.md`). The hub page task becomes a `write_article` task whose spec includes spoke metadata as a task artifact. The existing `ContentHandler` plans a single agentic step with `"skill": "hub-write"`. The agent writes the MDX. Done.

2. **If hub-specific deterministic prep is needed:** Add ONE deterministic step (`hub_build_brief`) that reads the strategy artifact and assembles a JSON brief. Then the existing `write_article` agentic step consumes that brief via the normal artifact-loading mechanism. No new handler family. No new article-persistence logic.

3. **Linking:** After the hub article is written, auto-spawn a `cluster_and_link` follow-up task (or reuse `append_related_section()` directly in a post-action).

**The cost of the mistake:**
- ~2,000 lines of duplicated code to maintain
- 7 new `StepKind` variants bloating the step registry
- A new `HandlerFamily::HubPage` and handler registry entry
- Custom SQLite insertion logic that diverged from `content::ops` conventions
- Custom file naming (`_hub.mdx`) that bypassed the standard numbering pipeline
- Tests that tested the duplication, not the reuse

**Lesson:** When the task output is an MDX article, the answer is almost always "reuse `write_article` with a different skill", not "build a new pipeline".

---

## How to Maintain a Feature

### Changing a Rust model

1. Update the struct in `src-tauri/src/models/{file}.rs`.
2. Add or keep `#[derive(..., TS)]` and `#[ts(export)]` on the struct to auto-generate TypeScript bindings.
3. Update the matching SQLite schema (`db/mod.rs`) if stored — add a new migration.
4. Update `export.rs` if the model is serialized to/from JSON.
5. Regenerate TypeScript bindings: `./scripts/sync-bindings.sh`
6. Run `cargo check` to catch compile errors before touching the frontend.

### Type Safety with ts-rs

We use [ts-rs](https://github.com/Aleph-Alpha/ts-rs) to auto-generate TypeScript types from Rust structs.

**How it works:**
- Rust structs in `src-tauri/src/models/` have `#[ts(export)]` derive
- Running `./scripts/sync-bindings.sh` exports TypeScript to `src/lib/bindings/`
- `src/lib/types.ts` re-exports auto-generated types + defines frontend-only types

**When to add `#[ts(export)]`:**
- Any struct that crosses the Tauri IPC boundary (commands, events)
- Any struct serialized to JSON and used by the frontend
- Keep internal structs (DB-only, logic-only) without it

**Regenerating bindings:**
```bash
./scripts/sync-bindings.sh
```

This runs `cargo test export_bindings --lib` and copies the generated `.ts` files to `src/lib/bindings/`.
The script now fails loudly if Rust exports fail and removes stale bindings before copying.

**Don't manually edit** files in `src/lib/bindings/` — they are auto-generated.

### Changing a command signature

1. Update the `#[tauri::command]` function in `commands.rs`.
2. Update the matching `invoke()` wrapper in `src/lib/tauri.ts`.
3. Update all call sites in React components.

### Changing secrets/env handling

- Only touch `config/env_resolver.rs`.
- Add new required secrets to `REQUIRED_SECRETS` so the UI surfaces them in `SecretsManager`.

---

## Business Logic Overview

| Domain | Rust module | What it does |
|---|---|---|
| Task lifecycle | `engine/task_store.rs`, `engine/executor.rs` | CRUD tasks in SQLite; run step graphs per task family |
| Workflows | `engine/workflows/` | Trait-based handlers plan deterministic/agentic step sequences |
| Batch execution | `engine/batch.rs` | Run all autonomous tasks up to a configurable limit |
| Scheduling | `engine/scheduler.rs` | Evaluate rules → auto-create due tasks |
| Content health | `content/` | Locate MDX dirs; validate structure, dates, internal links |
| Reddit | `reddit/` | Search Reddit JSON API; track opportunities + replies in SQLite |
| GSC | `gsc/` | Authenticate + call Google Search Console APIs; classify results |
| SEO research | `seo/` | Ahrefs keyword difficulty + backlink data + traffic estimates |
| Secrets | `config/env_resolver.rs` | Unified credential resolution across all modules |
| Persistence | `db/mod.rs` | SQLite schema migrations; `db/export.rs` for JSON repo interchange |
| Global Settings | `db/global_settings.rs` | Application-wide settings (agent_provider, defaults) |

---

## Shared State in Tauri

Three managed states declared in `lib.rs` and available as `State<'_>` in commands:

| State struct | Contents | Used by |
|---|---|---|
| `AppState` | `Arc<Mutex<Connection>>` — SQLite connection | All DB commands |
| `GscState` | `Mutex<Option<TokenState>>` — OAuth token cache | GSC commands |
| `SeoState` | `Mutex<HashMap<String, CachedSignature>>` — Ahrefs cache | SEO commands |

---

## Key Dependencies

| Crate | Purpose |
|---|---|
| `tauri 2` | Desktop shell + IPC |
| `rusqlite` (bundled) | SQLite, no system lib needed |
| `serde` / `serde_json` | Serialization across IPC |
| `reqwest` + `tokio` | Async HTTP (GSC, Reddit, Ahrefs) |
| `thiserror` | Derive-based error variants |
| `chrono` | Timestamps and date math |
| `walkdir` | Recursive file system traversal |
| `regex` | MDX frontmatter + link parsing |
| `jsonwebtoken` | Google service account JWT signing |
| `dotenvy` | .env file parsing |
| `dirs` | Platform home/config directory resolution |

---

## Development Process

Before starting multi-step features, write a spec in `docs/`. Key rules:

1. **Port behavior, not architecture** — identify inputs/outputs first, not class hierarchies.
2. **Test the agent prompt before writing the executor** — paste it into the CLI manually.
3. **One end-to-end run before any UI work** — backend must produce correct output first.
4. **Read the CLI reference implementation first** — `pageseeds-cli` has working versions of every workflow.
5. **Spec before code** — any feature touching 2+ files gets a spec in `docs/` first.
6. **Ship one thing at a time** — verify it works, then start the next.

Feature specs live in `docs/`. Write one before writing code.

### Legacy Code Warning

The `create_hub_page` and `refresh_hub_page` task types, along with their dedicated `Hub*` step kinds and `engine/exec/content/hub_page.rs` module, are **legacy**. They exist only for backward compatibility during migration. New work must NOT copy this pattern. Hub pages should be created using the standard `write_article` task type with a `hub-write` skill and structured artifacts. See the "Anti-Pattern Case Study: Hub Page Creation" section above for details.

---

## Frontend Development, Testing & Tooling

### Commands you must run before finishing any frontend change

```bash
# 1. Lint — catches hooks errors, missing deps, setState in effects, etc.
pnpm run lint

# 2. Typecheck — catches TS errors that Vite might miss in dev
pnpm exec tsc -b

# 3. Tests — run the suite
pnpm test

# 4. Build — verifies the production bundle compiles
pnpm run build
```

**CI enforces all of the above** on every PR/push (see `.github/workflows/ci.yml`).

### Testing stack

| Tool | Purpose |
|---|---|
| **Vitest** | Test runner (replaces Jest) |
| **@testing-library/react** | Component/hook testing utilities |
| **@testing-library/jest-dom** | DOM matchers (`toBeInTheDocument`, etc.) |
| **jsdom** | Browser environment for tests |

**Test files** live next to the code they test: `src/hooks/useQueueRunner.test.ts`, `src/components/tasks/TaskRunner.test.tsx`, etc.

**Writing a hook test:**
```tsx
import { renderHook } from '@testing-library/react'
import { useQueueRunner } from './useQueueRunner'

it('returns stable items between renders', () => {
  const { result, rerender } = renderHook(() => useQueueRunner())
  const first = result.current.items
  rerender()
  expect(result.current.items).toBe(first) // same reference
})
```

**Writing a component test:**
```tsx
import { render, screen } from '@testing-library/react'
import { TaskRunner } from './TaskRunner'

it('shows completed count', () => {
  render(<TaskRunner items={mockItems} isRunning={false} ... />)
  expect(screen.getByText('1 / 2 complete')).toBeInTheDocument()
})
```

### Dev guard: why-did-you-render

In development, `why-did-you-render` is active. If a component re-renders because of an unstable prop or reference, the console prints exactly which prop changed.

**Example output:**
```
[why-did-you-render] TaskRunner
Re-rendered because of props changes:
- items changed from === to !== (new reference)
- onOpenTask changed from === to !== (new reference)
```

If you see this, the fix is usually:
- Wrap the array in `useMemo`
- Wrap the callback in `useCallback`
- Use Zustand selectors instead of subscribing to the whole store

---

## React Patterns & Pitfalls

### Zustand: always use selectors

**Bad** — subscribes to the ENTIRE store; any mutation re-renders the component:
```tsx
const store = useQueueStore()
const items = store.items
```

**Good** — only re-renders when `items` changes:
```tsx
const items = useQueueStore(s => s.items)
const isRunning = useQueueStore(s => s.isRunning)
```

### Memoize mapped arrays

**Bad** — new array reference on every render:
```tsx
const items = store.items.map(item => ({ ...item, status: 'queued' }))
```

**Good** — stable reference when source hasn't changed:
```tsx
const items = useMemo(() =>
  store.items.map(item => ({ ...item, status: 'queued' })),
  [store.items]
)
```

### Memoize callbacks passed as props

**Bad** — new function reference on every parent render:
```tsx
<TaskRunner onOpenTask={(taskId) => { setActiveView('tasks'); setPendingTaskId(taskId) }} />
```

**Good** — stable reference:
```tsx
const handleOpenTask = useCallback((taskId: string) => {
  setActiveView('tasks')
  setPendingTaskId(taskId)
}, [])

<TaskRunner onOpenTask={handleOpenTask} />
```

### Never copy `useQuery` data into local state via `useEffect`

**Bad** — creates an unnecessary render cycle and triggers `react-hooks/set-state-in-effect`:
```tsx
const { data: fetchedOpps } = useQuery(...)
const [opps, setOpps] = useState([])

useEffect(() => {
  setOpps(fetchedOpps) // ← lint error + extra render
}, [fetchedOpps])
```

**Good** — use the query data directly:
```tsx
const { data: opps = [] } = useQuery(...)
```

### Do not define components inside render

**Bad** — `react-hooks/static-components` error + state resets every render:
```tsx
function OpportunityFeed() {
  function SortIcon({ col }) { ... } // ← lint error
  return <th>Score<SortIcon col="score" /></th>
}
```

**Good** — define at module scope:
```tsx
function SortIcon({ col, sortKey, sortAsc }: SortIconProps) { ... }

function OpportunityFeed() {
  return <th>Score<SortIcon col="score" sortKey={sortKey} sortAsc={sortAsc} /></th>
}
```

### ESLint rules that catch infinite loops

The project uses `eslint-plugin-react-hooks` v7.0.1. These rules are set to **error** and will block CI:

| Rule | What it catches |
|---|---|
| `react-hooks/exhaustive-deps` | Missing dependencies in `useEffect` / `useCallback` / `useMemo` |
| `react-hooks/set-state-in-effect` | `setState` called directly inside `useEffect` (can cascade) |
| `react-hooks/refs` | Reading or writing `ref.current` during render |
| `react-hooks/static-components` | Component definitions inside other components |
| `react-hooks/rules-of-hooks` | Hooks called conditionally or in loops |

**If you hit `set-state-in-effect` legitimately** (e.g. tracking previous prop values for derived state), add an `eslint-disable-next-line` comment with a justification explaining why the effect is bounded:
```tsx
// eslint-disable-next-line react-hooks/set-state-in-effect
setPrevStatusMap(new Map(items.map(it => [it.task.id, it.status])))
```

---

---

## Async Architecture

### The Pattern

All async task execution follows this pattern (see `engine/runtime.rs`):

```rust
#[tauri::command]
pub async fn execute_task(...) -> Result<...> {
    let db_path = state.db_path.clone();
    
    tokio::task::spawn_blocking(move || {
        // 1. Open dedicated SQLite connection (per-thread)
        let db = rusqlite::Connection::open(&db_path)?;
        db.busy_timeout(Duration::from_secs(10))?;
        
        // 2. Create local Tokio runtime for async execution
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            executor::execute_task(&db, &task_id).await
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
```

### Why This Pattern?

| Constraint | Solution |
|------------|----------|
| SQLite connections are !Send | Each task gets its own OS thread + connection |
| Tauri runtime is multi-threaded | Use `spawn_blocking` for SQLite operations |
| Async HTTP calls in step handlers | Local Tokio runtime per task enables `.await` |

### Key Rules

1. **Never use `Handle::current().block_on()`** - Causes panic in async context
2. **Always use `.await` for async operations** - Never block the async runtime
3. **One connection per task** - SQLite connections cannot be shared between threads
4. **Step handlers can be async** - Executor supports both sync and async step handlers

### Runtime Helpers

`engine/runtime.rs` provides helper functions:

- `open_connection()` - Open SQLite connection with proper timeout
- `create_local_runtime()` - Create Tokio runtime for async execution
- `spawn_with_db()` - Spawn blocking task with connection
- `spawn_async_with_db()` - Spawn async blocking task with local runtime

### Future Optimization (Phase 3)

Consider `deadpool-sqlite` for connection pooling if:
- You need 10+ concurrent tasks regularly
- Task startup latency (1-5ms) becomes a bottleneck
- Memory usage (2-4MB per task) becomes a concern

See `docs/async-architecture.md` for detailed comparison.

---

## Pre-Change Checklist

### Rust backend
- [ ] **Checked for reuse**: Reviewed the "DRY: Core Reusable Functions" catalog above. If the feature writes MDX, reuses `write_article`; if it links articles, reuses `cluster_and_link`; if it exports articles.json, reuses `db::export::write_articles_to_repo`; etc.
- [ ] **Task lifecycle contract checked**: If the change creates, queues, reviews, or spawns tasks, identified the lifecycle lane and reused `TaskSpawner`, backend queue commands, `task_definitions`, and `post_actions` as appropriate.
- [ ] `cargo check` passes before touching the frontend
- [ ] `cargo test` passes — especially workflow routing and task definition tests
- [ ] New SQLite columns added via a new migration, not by altering existing ones
- [ ] **Settings placed correctly**: User preferences → `global_settings`; Project config → `projects` table
- [ ] No business logic added to `commands/*.rs` — only thin wrappers
- [ ] `tauri.ts` wrapper added/updated for any new or changed command
- [ ] `types.ts` updated to match Rust struct changes (or run `./scripts/sync-bindings.sh` if `#[ts(export)]` is present)
- [ ] `./scripts/check-bindings.sh` passes if a Rust model with `#[ts(export)]` was changed
- [ ] `pnpm run check:ipc` passes — every frontend `invoke` must be statically registered or explicitly allowlisted
- [ ] No secrets or absolute machine paths in source code
- [ ] No `subprocess` / shell calls — use Rust crates instead
- [ ] Reviewed `CONTRACTS.md` for any affected implicit contracts (statuses, step ordering, auto-spawned tasks, handler registry order)
- [ ] New task types added to `config/task_definitions.rs` before wiring handlers
- [ ] Every new agentic step has: (a) specific input context, (b) an output contract in a code comment, (c) a comment explaining why it cannot be deterministic
- [ ] Every new deterministic step does not contain a hard-coded heuristic that substitutes for judgment (that is fake intelligence — use an agentic step for the selection)

### Frontend
- [ ] `pnpm run lint` passes — no `exhaustive-deps`, `set-state-in-effect`, `refs`, or `static-components` errors
- [ ] `pnpm exec tsc -b` passes — no TypeScript errors
- [ ] `pnpm test` passes — all existing tests green, new tests added for new hooks/components
- [ ] `pnpm run check:ipc` passes — no unregistered frontend invokes
- [ ] `pnpm run build` passes — production bundle compiles
- [ ] Zustand store accesses use selectors (`useQueueStore(s => s.items)`) not bare `useQueueStore()`
- [ ] Arrays mapped in hooks/components are wrapped in `useMemo`
- [ ] Callbacks passed as JSX props are wrapped in `useCallback`
- [ ] `useQuery` data is used directly — never copied into local state via `useEffect`
- [ ] No components defined inside other components
