# PageSeeds App — AI Quick Start

> TL;DR for AI agents: Where to find what you need.

---

## What This Is

A **Tauri 2 desktop app** for SEO content workflows. Self-contained binary — no Python, no external CLI dependencies.

| Layer | Tech |
|-------|------|
| Backend | Rust (`src-tauri/src/`) |
| Frontend | React + TypeScript + Vite + Tailwind v4 + shadcn/ui (`src/`) |
| Store | SQLite (runtime state) + JSON in user's repo (committed content) |
| IPC | Tauri commands (`invoke()` frontend → `#[tauri::command]` Rust) |

---

## Quick Navigation

### Understanding the Domain
- **[Business Processes](./docs/BUSINESS_PROCESSES.md)** — What the app does: keyword research, content creation, optimization, publishing, GSC monitoring, CTR optimization, cannibalization detection, Reddit marketing, social media, agentic investigation

### Understanding the Architecture
- **[Workflow Engine](./docs/WORKFLOW_ENGINE.md)** — How tasks are planned and executed (handlers, steps, deterministic vs agentic)
- **[Data Persistence](./docs/DATA_PERSISTENCE.md)** — SQLite runtime state + JSON committed content
- **[Agent Integration](./docs/AGENT_INTEGRATION.md)** — How LLM agents are invoked and responses normalized

### Critical Reference
- **[CONTRACTS.md](./CONTRACTS.md)** — Runtime invariants that WILL break things if violated (status values, handler order, auto-spawned tasks)
- **[AGENTS.md](./AGENTS.md)** — Full agent guide with directory map, coding rules, and development process

### Debugging
---

## Directory Structure

```
src-tauri/src/
├── main.rs              # Entry point — no logic
├── lib.rs               # Tauri setup, state management, command registration
├── error.rs             # Central Error enum + Result<T>
├── commands/            # ALL #[tauri::command] handlers — thin IPC wrappers
│   ├── mod.rs
│   ├── tasks.rs
│   ├── gsc.rs
│   ├── reddit.rs
│   └── ...
├── models/              # Pure serde structs — no logic
│   ├── task.rs          # Task, TaskArtifact, TaskRun, TaskStatus, etc.
│   ├── article.rs
│   ├── project.rs
│   └── ...
├── db/
│   ├── mod.rs           # SQLite init + migrations
│   └── export.rs        # JSON read/write for user's repo
├── engine/              # Workflow orchestration
│   ├── executor.rs      # Orchestrator only (~400 lines)
│   ├── spawner.rs       # CENTRALIZED task creation — use this, not task_store
│   ├── batch.rs         # Autonomous batch execution
│   ├── scheduler.rs     # Scheduled rule evaluation
│   ├── task_store.rs    # SQLite CRUD for tasks/projects
│   ├── agent.rs         # LLM provider calls
│   ├── prompts.rs       # Prompt assembly
│   ├── normalizer.rs    # Parse agent output → JSON
│   ├── skills.rs        # Load SKILL.md files
│   ├── project_paths.rs # Resolve automation/content dirs
│   ├── runtime.rs       # Async execution helpers
│   ├── workflows/
│   │   ├── mod.rs       # WorkflowStep struct
│   │   └── handlers.rs  # WorkflowHandler trait + all handlers
│   └── exec/            # Domain-specific execution logic
│       ├── mod.rs
│       ├── keywords.rs  # Keyword research
│       ├── content.rs   # Content review/apply
│       ├── content_audit.rs
│       ├── reddit.rs    # Reddit search + enrichment
│       ├── gsc.rs       # GSC collection + sync
│       └── utils.rs
├── content/             # MDX operations
│   ├── locator.rs       # Find content directory
│   ├── ops.rs           # Sync, slug generation, frontmatter
│   ├── cleaner.rs       # Validate/fix MDX structure
│   ├── dates.rs         # Date analysis/redistribution
│   ├── linking.rs       # Internal link scanning
│   └── publish.rs       # Publishing workflow
├── reddit/              # Reddit JSON API
│   ├── mod.rs
│   ├── search.rs
│   ├── db.rs            # Opportunity CRUD
│   ├── prompts.rs       # Reply drafting prompts
│   └── history.rs       # Reply history tracking
├── gsc/                 # Google Search Console
│   ├── auth.rs          # Service account + OAuth
│   ├── client.rs        # Authenticated HTTP client
│   ├── analytics.rs     # Search analytics
│   ├── indexing.rs      # URL Inspection API
│   ├── classification.rs # Reason codes
│   ├── coverage.rs      # 404 detection
│   ├── redirects.rs     # Redirect analysis
│   └── reports.rs       # Report generation
├── seo/                 # Ahrefs integration
│   ├── keywords.rs      # Keyword ideas + difficulty
│   ├── backlinks.rs     # Backlink analysis
│   └── traffic.rs       # Traffic estimates
└── config/              # Configuration
    ├── mod.rs           # Constants, default values
    └── env_resolver.rs  # Secrets resolution

src/
├── lib/
│   ├── tauri.ts         # ALL invoke() wrappers — one function per command
│   └── types.ts         # TypeScript types mirroring Rust exactly
├── stores/
│   ├── queueStore.ts    # Global task queue state
│   └── ...
└── components/          # Feature-scoped React components
    ├── ui/              # shadcn/ui primitives ONLY
    ├── tasks/           # TaskBoard, TaskDetail, TaskRunner
    ├── articles/        # ArticleTable, ContentHealth, PublishPanel
    ├── reddit/          # OpportunityFeed, ReplyDraft, RedditStats
    ├── gsc/             # GSCDashboard, IndexingReport, CoverageView
    ├── seo/             # KeywordResearch, BacklinkView, TrafficOverview
    ├── social/          # SocialDashboard, CampaignList, PostEditor, TemplateList
    ├── health/          # HealthDashboard, InvestigationPanel
    ├── cannibalization/ # CannibalizationReview
    ├── projects/        # ProjectSwitcher, ProjectSettings
    └── settings/        # SecretsManager, SchedulerConfig
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
- **All UI uses shadcn components**: `Sheet`, `ScrollArea`, `Dialog`, `Tabs`, etc.

### 3. Workflow Steps

| Mode | Use When | Never For |
|------|----------|-----------|
| **Deterministic** | Machine-checkable, repeatable logic (API calls, filtering, sorting) | Interpreting ambiguous text or intent |
| **Agentic** | Judgment required (theme curation, prioritization, prose generation) | Stable API calls that have deterministic paths |

**Hybrid pattern** (canonical): Deterministic step collects data → Agentic step interprets.

---

## Key Contracts (Read CONTRACTS.md)

### Task Statuses
```
"todo" | "in_progress" | "review" | "done" | "cancelled"
```
- `research_keywords`, `custom_keyword_research`, `research_landing_pages`, `reddit_opportunity_search` finish with `"review"
- All others: `in_progress → done` on success

### Execution Modes
```
"automatic" | "batchable" | "manual" | "spec"
```

### Handler Registry Order (First-Match-Wins)
```
CollectionHandler → InvestigationHandler → ResearchHandler → ContentHandler
→ ContentReviewHandler → RedditHandler → PerformanceHandler → ImplementationHandler
→ ManualFallbackHandler (MUST be last)
```

### Task Creation
**Always use `TaskSpawner`** — never call `task_store::create_task` directly:
```rust
// For general creation
TaskSpawner::spawn(conn, TaskSpec { ... })?;

// For follow-ups (idempotent)
TaskSpawner::spawn_follow_up(conn, parent_task, "task_type", "title")?;
```

---

## Adding a Feature

### New Rust Module
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
3. Add execution logic in `engine/exec/{domain}.rs`
4. Wire in executor's `run_step()` match

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
- [ ] No business logic added to `commands/`
- [ ] `tauri.ts` wrapper added/updated for any new/changed command
- [ ] `types.ts` updated to match Rust struct changes
- [ ] No secrets or absolute paths in source code
- [ ] No `subprocess` / shell calls
- [ ] Reviewed CONTRACTS.md for affected contracts
- [ ] Every new agentic step has: (a) specific input context, (b) output contract comment, (c) why-not-deterministic comment

---

## Common Commands

```bash
pnpm dev              # Vite dev server
pnpm tauri dev        # Tauri dev mode (Rust + frontend)
cargo check           # Check Rust code
pnpm build            # Production build
./build-release.sh    # Build macOS release
./publish-release.sh  # Interactive release
```

---

## When You Need More Detail

| Question | Read |
|----------|------|
| What workflows exist? | [Business Processes](./docs/BUSINESS_PROCESSES.md) |
| How does task execution work? | [Workflow Engine](./docs/WORKFLOW_ENGINE.md) |
| How is the queue managed? | [AGENTS.md](./AGENTS.md) |
| Where is data stored? | [Data Persistence](./docs/DATA_PERSISTENCE.md) |
| How do LLM agents work? | [Agent Integration](./docs/AGENT_INTEGRATION.md) |
| What are the runtime invariants? | [CONTRACTS.md](./CONTRACTS.md) |
| How do I add a feature? | [AGENTS.md](./AGENTS.md) |
| Why did my task fail? | Check the task detail panel → Run History |
