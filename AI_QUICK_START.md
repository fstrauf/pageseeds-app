# PageSeeds Operator CLI — AI Quick Start

> TL;DR for AI agents: Where to find what you need.

---

## What This Is

**PageSeeds Operator CLI** — a pure Rust workspace. Desktop (Tauri + React) was removed in #184.

| Layer | Tech |
|-------|------|
| Domain | Rust library `crates/pageseeds-core` (`pageseeds_core`) |
| Operator CLI | `crates/pageseeds-cli` (`pageseeds-cli` bin) |
| Store | SQLite (operator runtime state) + JSON in user's repo (committed content) |
| Surface | CLI only (JSON stdout); no Tauri IPC |

---

## Quick Navigation

### Understanding the Domain
- **[Business Processes](./docs/BUSINESS_PROCESSES.md)** — What the product does: keyword research, content creation, optimization, publishing, GSC monitoring, CTR optimization, cannibalization detection, Reddit marketing, social media, agentic investigation

### Understanding the Architecture
- **[Workflow Engine](./docs/WORKFLOW_ENGINE.md)** — How tasks are planned and executed (handlers, steps, deterministic vs agentic)
- **[Data Persistence](./docs/DATA_PERSISTENCE.md)** — SQLite runtime state + JSON committed content
- **[Agent Integration](./docs/AGENT_INTEGRATION.md)** — How LLM agents are invoked and responses normalized

### Critical Reference
- **[CONTRACTS.md](./CONTRACTS.md)** — Runtime invariants that WILL break things if violated (status values, handler order, auto-spawned tasks)
- **[AGENTS.md](./AGENTS.md)** — Full agent guide with directory map, coding rules, and development process
- **[CLI Getting Started](./docs/CLI_GETTING_STARTED.md)** — Install + operator happy path

---

## Directory Structure

```
Cargo.toml                 # workspace: pageseeds-core, pageseeds-cli
crates/
├── pageseeds-core/        # Domain library (rlib only)
│   ├── skills/            # Embedded app-default skills (include_str!)
│   ├── config/            # tool_catalog.toml
│   └── src/
│       ├── lib.rs         # Domain modules only
│       ├── error.rs       # Central Error enum + Result<T>
│       ├── models/        # Pure serde structs — no logic
│       │   ├── task.rs    # Task, TaskArtifact, TaskRun, TaskStatus, etc.
│       │   ├── article.rs
│       │   ├── project.rs
│       │   └── ...
│       ├── db/
│       │   ├── mod.rs     # SQLite init + migrations
│       │   └── export.rs  # JSON read/write for user's repo
│       ├── engine/        # Workflow orchestration
│       │   ├── executor.rs
│       │   ├── spawner.rs # CENTRALIZED task creation — use this, not task_store
│       │   ├── task_store.rs
│       │   ├── agent.rs
│       │   ├── prompts.rs
│       │   ├── skills.rs
│       │   ├── workflows/
│       │   │   └── handlers.rs
│       │   └── exec/      # Domain-specific execution logic
│       ├── content/       # MDX operations
│       ├── reddit/, gsc/, seo/, social/, clarity/, rig/, license/, …
│       └── config/        # Constants, env_resolver, task_definitions
└── pageseeds-cli/         # Operator CLI bin (depends only on core)
    └── src/main.rs

scripts/
├── install-cli.sh
├── check-cli-contract.sh
└── check-task-store-usage.sh
```

> Edit domain logic in `crates/pageseeds-core`. Ship gate: `pnpm run test:cli`.

---

## Core Rules (Non-Negotiable)

### 1. Rust Domain + CLI
- **Business logic lives in `pageseeds-core` modules** — never in the CLI binary
- **CLI is thin**: parse args → call core → print JSON / errors
- **One error type**: `error::Error` and `error::Result<T>` everywhere in domain
- **No subprocess calls** — use Rust crates directly (`reqwest`, `rusqlite`, etc.)
- **SQLite migrations**: Never alter existing migration blocks — add new `MIGRATION_VN` constants

### 2. Workflow Steps

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
- `research_keywords`, `custom_keyword_research`, `research_landing_pages`, `reddit_opportunity_search` finish with `"review"`
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
1. Create `crates/pageseeds-core/src/{domain}/mod.rs`
2. Declare in `crates/pageseeds-core/src/lib.rs`: `mod {domain};` (or `pub mod`)
3. Add types to `crates/pageseeds-core/src/models/` if needed for serde/CLI
4. Expose via CLI in `crates/pageseeds-cli` only if operators need a new command (thin wrapper)

### New SQLite Table
1. Add `MIGRATION_VN` constant in `db/mod.rs`
2. Apply in `db::init()` after prior migrations
3. Add CRUD functions in the relevant domain module

### New Workflow Task Type
1. Add `WorkflowHandler` impl in `engine/workflows/handlers.rs`
2. Register in `default_handlers()` (order matters!)
3. Add execution logic in `engine/exec/{domain}.rs`
4. Wire in executor's `run_step()` match
5. Register lifecycle in `config/task_definitions.rs`

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

- [ ] `cargo check -p pageseeds-core` / `cargo build -p pageseeds-cli` passes
- [ ] New SQLite columns added via new migration (not altering existing)
- [ ] No business logic added to `crates/pageseeds-cli` beyond arg parse / I/O
- [ ] No secrets or absolute paths in source code
- [ ] No `subprocess` / shell calls outside agent compatibility
- [ ] Reviewed CONTRACTS.md for affected contracts
- [ ] Every new agentic step has: (a) specific input context, (b) output contract comment, (c) why-not-deterministic comment
- [ ] `pnpm run test:cli` green

---

## Common Commands

```bash
# Operator / domain gates (all product work)
cargo test -p pageseeds-core
cargo build -p pageseeds-cli
pnpm run test:cli

# Install local binary
FROM_SOURCE=1 ./scripts/install-cli.sh
```

`pnpm test:all` aliases `test:cli` (no Vite/IPC/bindings).

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
| Install / first desk read? | [CLI Getting Started](./docs/CLI_GETTING_STARTED.md) |
| Why did my task fail? | Inspect task runs / artifacts via CLI or SQLite |
