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
├── commands.rs          # ALL #[tauri::command] bindings — IPC surface
├── error.rs             # Central Error enum + Result<T> alias
├── db/
│   ├── mod.rs           # SQLite init + schema migrations (versioned SQL constants)
│   └── export.rs        # Read/write articles.json and task_list.json in the user's repo
├── models/              # Pure serde structs — no logic
│   ├── task.rs          # Task, TaskArtifact, TaskRun
│   ├── article.rs       # Article metadata
│   ├── project.rs       # Project config
│   ├── reddit.rs        # RedditOpportunity, ReplyStatus
│   └── gsc.rs           # TokenState
├── engine/              # Workflow orchestration
│   ├── task_store.rs    # CRUD against SQLite tasks/projects tables
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
│   ├── tauri.ts         # All invoke() wrappers — one function per command
│   └── types.ts         # TypeScript types that mirror Rust models exactly
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
5. **No subprocess calls**. All I/O uses Rust crates directly (`reqwest`, `rusqlite`, `walkdir`, `regex`, etc.).
6. **Independent but isolated codebase**. Do not share code with `pageseeds-cli`. If a Python module needs porting, re-implement it cleanly in Rust.

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
2. Update the matching SQLite schema (`db/mod.rs`) if stored — add a new migration.
3. Update `export.rs` if the model is serialized to/from JSON.
4. Update the TypeScript interface in `src/lib/types.ts`.
5. Update any `tauri.ts` wrapper that passes the changed fields.
6. Run `cargo check` to catch compile errors before touching the frontend.

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

## Pre-Change Checklist

- [ ] `cargo check` passes before touching the frontend
- [ ] New SQLite columns added via a new migration, not by altering existing ones
- [ ] No business logic added to `commands.rs`
- [ ] `tauri.ts` wrapper added/updated for any new or changed command
- [ ] `types.ts` updated to match Rust struct changes
- [ ] No secrets or absolute machine paths in source code
- [ ] No `subprocess` / shell calls — use Rust crates instead
