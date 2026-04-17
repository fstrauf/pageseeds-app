# AI Agent Guide — PageSeeds App

Concise reference for AI agents adding or maintaining features in this repo.

---

## What This Repo Is

A **Tauri 2 desktop app** — self-contained binary, no Python, no external CLI dependency.

- **Backend**: Rust (`src-tauri/src/`)
- **Frontend**: React + TypeScript + Vite + Tailwind v4 + shadcn/ui (`src/`)
- **Store**: SQLite (runtime state) + JSON in the user's repo (committed content data)
- **Not related to `pageseeds-cli`**: business logic is re-implemented here in Rust, not imported

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
│   ├── skills.rs        # Load SKILL.md files from the user's repo
│   ├── prompts.rs       # Prompt assembly
│   ├── project_paths.rs # Resolve content dir, automation dir, output dir per project
│   └── workflows/
│       ├── mod.rs       # WorkflowStep struct + StepResult
│       └── handlers.rs  # WorkflowHandler trait + one impl per task family
├── config/
│   ├── mod.rs           # Constants: PHASES, EXECUTION_MODE_MAP, etc.
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

1. Add a `WorkflowHandler` impl in `engine/workflows/handlers.rs`.
2. Register it in `default_handlers()` (same file).
3. Each handler only returns a `Vec<WorkflowStep>` — no execution logic.
4. Execution runs through `engine/executor.rs` unchanged.

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

Read [docs/dev-process.md](docs/dev-process.md) before starting multi-step features. Key rules:

1. **Port behavior, not architecture** — identify inputs/outputs first, not class hierarchies.
2. **Test the agent prompt before writing the executor** — paste it into the CLI manually.
3. **One end-to-end run before any UI work** — backend must produce correct output first.
4. **Read the CLI reference implementation first** — `pageseeds-cli` has working versions of every workflow.
5. **Spec before code** — any feature touching 2+ files gets a spec in `docs/` first.
6. **Ship one thing at a time** — verify it works, then start the next.

Feature specs live in `docs/`. Write one before writing code.

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

- [ ] `cargo check` passes before touching the frontend
- [ ] New SQLite columns added via a new migration, not by altering existing ones
- [ ] **Settings placed correctly**: User preferences → `global_settings`; Project config → `projects` table
- [ ] No business logic added to `commands/*.rs` — only thin wrappers
- [ ] `tauri.ts` wrapper added/updated for any new or changed command
- [ ] `types.ts` updated to match Rust struct changes (or run `./scripts/sync-bindings.sh` if `#[ts(export)]` is present)
- [ ] `./scripts/check-bindings.sh` passes if a Rust model with `#[ts(export)]` was changed
- [ ] No secrets or absolute machine paths in source code
- [ ] No `subprocess` / shell calls — use Rust crates instead
- [ ] Reviewed `CONTRACTS.md` for any affected implicit contracts (statuses, step ordering, auto-spawned tasks, handler registry order)
- [ ] Every new agentic step has: (a) specific input context, (b) an output contract in a code comment, (c) a comment explaining why it cannot be deterministic
- [ ] Every new deterministic step does not contain a hard-coded heuristic that substitutes for judgment (that is fake intelligence — use an agentic step for the selection)
