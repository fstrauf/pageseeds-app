# PageSeeds App — AI Quick Start

> TL;DR for AI agents: What this repo is, where things live, and the rules you must follow.

## What This Is

A **Tauri 2 desktop app** for SEO content workflows. Self-contained binary — no Python, no external CLI dependencies.

| Layer | Tech |
|-------|------|
| Backend | Rust (`src-tauri/src/`) |
| Frontend | React + TypeScript + Vite + Tailwind v4 + shadcn/ui (`src/`) |
| Store | SQLite (runtime state) + JSON in user's repo (committed content data) |
| IPC | Tauri commands (`invoke()` frontend → `#[tauri::command]` Rust) |

**Important:** Business logic is re-implemented here in Rust — not imported from `pageseeds-cli`.

---

## Directory Structure

```
src-tauri/src/
├── commands/            # ALL #[tauri::command] handlers — thin IPC wrappers only
├── models/              # Pure serde structs (Task, Article, Project, etc.)
├── db/                  # SQLite init, migrations, JSON export to user's repo
├── engine/              # Workflow orchestration
│   ├── executor.rs      # Runs tasks: finds handler → plans steps → executes
│   ├── task_store.rs    # SQLite CRUD for tasks/projects
│   ├── workflows/
│   │   ├── handlers.rs  # WorkflowHandler trait + impls per task family
│   │   └── ...
│   ├── agent.rs         # LLM provider calls (Kimi / Copilot)
│   ├── batch.rs         # Autonomous batch execution
│   └── scheduler.rs     # Scheduled rule evaluation
├── content/             # MDX operations (locate, sync, audit, validate)
├── reddit/              # Reddit JSON API + opportunity DB
├── gsc/                 # Google Search Console auth + APIs
├── seo/                 # Ahrefs keyword/backlink/traffic
├── social/              # Social media campaign management
├── config/              # Constants, env resolution
├── lib.rs               # Tauri setup, state management
└── error.rs             # Central Error enum + Result<T> alias

src/
├── lib/
│   ├── tauri.ts         # ALL invoke() wrappers — one function per command
│   └── types.ts         # TypeScript types mirroring Rust models exactly
└── components/
    ├── ui/              # shadcn/ui primitives ONLY
    ├── tasks/           # TaskBoard, TaskDetail, TaskRunner
    ├── articles/        # ArticleTable, ContentHealth, PublishPanel
    ├── reddit/          # OpportunityFeed, ReplyDraft
    ├── gsc/             # GSCDashboard, IndexingReport
    ├── seo/             # KeywordResearch, BacklinkView
    ├── social/          # CampaignCreate, PostEditor
    └── settings/        # SecretsManager
```

---

## Core Rules (Non-Negotiable)

### 1. Rust Backend
- **Business logic lives in Rust modules** — never in `commands/` or frontend
- **Commands are thin**: validate inputs → call module function → return result
- **One error type**: `error::Error` and `error::Result<T>` everywhere
- **No subprocess calls** — use Rust crates directly (`reqwest`, `rusqlite`, etc.)
- **SQLite migrations**: Never alter existing migration blocks — add new `MIGRATION_VN` constants

### 2. Frontend
- **All data goes through `invoke()`** in `src/lib/tauri.ts` — no direct file I/O
- **Types mirror Rust exactly**: Update `src/lib/types.ts` when Rust structs change
- **UI stack**: Tailwind v4, shadcn/ui primitives, Manrope (body), Fraunces (display)
- **All UI uses shadcn components**: `Sheet`, `ScrollArea`, `Dialog`, `Tabs`, etc. — no raw HTML shells

### 3. Workflow Steps
Every step must be explicitly **deterministic** or **agentic**:

| Mode | Use When | Never For |
|------|----------|-----------|
| **Deterministic** | Machine-checkable, repeatable logic (API calls, filtering, sorting) | Interpreting ambiguous text or intent |
| **Agentic** | Judgment required (theme curation, prioritization, prose generation) | Stable API calls that have deterministic paths |

**Hybrid pattern** (canonical): Deterministic step collects data → Agentic step interprets.

---

## Key Contracts (Read CONTRACTS.md for Full Details)

### Task Statuses
```
"todo" | "in_progress" | "review" | "done" | "cancelled"
```
- Only `research_keywords` and `custom_keyword_research` finish with `"review"`
- All others: `in_progress → done` on success

### Execution Modes
```
"automatic" | "batchable" | "manual" | "spec"
```
- `"automatic"` + `"batchable"` run in batch runner
- `"spec"` requires a spec artifact before execution

### Workflow Step Kinds
```
"agentic" | "normalizer" | "deterministic" | "manual" | "reddit_search" | ...
```
- **Agentic → Normalizer ordering is mandatory**: Executor passes `latest_raw_output` to normalizer
- Reddit enrichment runs inline in executor loop (not as separate steps)

### Handler Registry Order (First-Match-Wins)
```
CollectionHandler → InvestigationHandler → ResearchHandler → ContentHandler
→ ContentReviewHandler → RedditHandler → PerformanceHandler → ImplementationHandler
→ ManualFallbackHandler (MUST be last)
```

---

## Adding a Feature

### New Rust Module (e.g., new data source)
1. Create `src-tauri/src/{domain}/mod.rs`
2. Declare in `lib.rs`: `mod {domain};`
3. Add types to `models/` if crossing IPC
4. Add `#[tauri::command]` to `commands/` (thin wrapper)
5. Register command in `lib.rs` `generate_handler![]`
6. Add typed wrapper to `src/lib/tauri.ts`
7. Add TypeScript type to `src/lib/types.ts`
8. Build React component in `src/components/{domain}/`

### New SQLite Table
1. Add `MIGRATION_VN` constant in `db/mod.rs`
2. Apply in `db::init()` after prior migrations
3. Add CRUD functions in relevant module — not in commands

### New Workflow Task Type
1. Add `WorkflowHandler` impl in `engine/workflows/handlers.rs`
2. Register in `default_handlers()` (order matters!)
3. Each handler returns `Vec<WorkflowStep>` — execution runs through `executor.rs`

---

## Secrets Resolution Order

```
1. ~/.config/automation/secrets.env   (highest — always wins)
2. {repo}/.env.local
3. {repo}/.env
4. Shell environment variables
```

Use `config::env_resolver::EnvResolver` — never `std::env::var()` directly.

---

## Pre-Change Checklist

- [ ] `cargo check` passes before touching frontend
- [ ] New SQLite columns added via new migration (not altering existing)
- [ ] No business logic in `commands/`
- [ ] `tauri.ts` wrapper added for any new/changed command
- [ ] `types.ts` updated to match Rust struct changes
- [ ] No secrets or absolute paths in source code
- [ ] No `subprocess` / shell calls
- [ ] Reviewed CONTRACTS.md for affected contracts
- [ ] New agentic step has: (a) specific input context, (b) output contract comment, (c) why-not-deterministic comment

---

## Common Commands

```bash
pnpm dev              # Vite dev server
pnpm tauri dev        # Tauri dev mode (Rust + frontend)
cargo check           # Check Rust code
pnpm build            # Production build
./build-release.sh    # Build macOS release
```

---

## Documentation References

| File | Purpose |
|------|---------|
| `AGENTS.md` | Full agent guide (268 lines) — comprehensive reference |
| `CONTRACTS.md` | Runtime contracts, invariants, hidden rules |
| `STYLE_GUIDE.md` | Design system — fonts, colors, Tailwind tokens |
| `docs/dev-process.md` | Feature development process |
| `docs/*-spec.md` | Feature specifications |

**Read AGENTS.md before multi-file changes. Read CONTRACTS.md before touching executor/workflows.**
