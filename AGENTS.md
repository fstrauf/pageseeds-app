# AI Agent Guide — PageSeeds Operator CLI

Boot-time rules for coding agents. **Not** a product runbook or API catalog.

> **How to change something:** [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md)  
> **Runtime invariants:** [CONTRACTS.md](./CONTRACTS.md)  
> **Task types:** [docs/TOOL_CATALOG.md](./docs/TOOL_CATALOG.md) + `config/task_definitions.rs`

---

## What This Repo Is

**PageSeeds Operator CLI** — pure Rust workspace for SEO operator tooling. Tauri/React desktop was removed in **#184** (not rebuilt here).

| Layer | Location |
|-------|----------|
| Domain | `crates/pageseeds-core` |
| CLI (thin) | `crates/pageseeds-cli` |
| Store | SQLite (runtime) + JSON in the user's content repo |
| LLM | `crates/pageseeds-core/src/rig/` (+ legacy `engine/agent.rs` fallback) |

**Ship gate:** `pnpm test:cli` (alias `pnpm test:all`) = `cargo test -p pageseeds-core` → `check:task-store` → `check:cli-contract`. Also `cargo build -p pageseeds-cli` when the CLI surface changes. Install: `scripts/install-cli.sh` / [CLI_GETTING_STARTED](./docs/CLI_GETTING_STARTED.md).

---

## Non-Negotiables

1. **Logic in core, not CLI.** CLI = parse args → call core → print JSON/errors.
2. **One error type:** `error::Error` / `error::Result<T>` in the domain crate.
3. **SQLite migrations are additive only.** New `MIGRATION_VN` in `db/mod.rs` — never edit existing migration blocks. Mutable runtime state goes through `engine/task_store/`.
4. **Create tasks only via `TaskSpawner::spawn` / `spawn_follow_up`.** Never call `task_store::create_task` for product flows. Spawner owns idempotency and dependency checks.
5. **Lifecycle metadata lives in `config/task_definitions.rs`:** `run_policy`, `review_surface`, `follow_up_policy`, `handler_family`. Do not re-encode those decisions in CLI branches or executor special cases.
6. **Prefer a skill over a new task type.** Editing `crates/pageseeds-core/skills/{name}/SKILL.md` (or a project override) is far cheaper than a new handler + step graph. When the output is an MDX article, reuse `write_article` + `ContentHandler` + a skill param — do not build a parallel write pipeline.
7. **Do not re-implement content/DB primitives.** Search `content/`, `db/export`, `engine/spawner`, `engine/text`, `models/project` first. Common traps: word count, frontmatter, slugs, `articles.json` writes, `site_url` (GSC property ID, not always a fetchable URL — use `site_base_url()`).
8. **Structured agent JSON via typed extractors** (`rig/` + `schemars`) or shared helpers in `engine::text` — not new regex parsers.
9. **Subprocess by tier.** Commercial-surface tools stay pure Rust. Operator-tier tools (see [CLI_COMMERCIAL](./docs/CLI_COMMERCIAL.md)) may spawn node/ffmpeg/python with PATH detection and clear install errors. LLM gets no arbitrary shell.
10. **Secrets via `EnvResolver` only** (`config/env_resolver.rs`). Precedence: `~/.config/automation/secrets.env` → `{repo}/.env.local` → `{repo}/.env` → shell. Never embed keys or machine paths in source.
11. **Settings:** global prefs → `global_settings`; project config → `projects`. Legacy project `agent_provider` is ignored.

---

## Where Code Goes

```
New logic?
├─ CLI UX (flags, stdout JSON, help)     → crates/pageseeds-cli (thin)
├─ Step graph for a task type            → engine/workflows/handlers.rs
├─ Body of one workflow step             → engine/exec/{domain}/
└─ Everything else (API, parse, DB, algo)→ pageseeds-core/{domain}/
```

**Add a workflow step:** `StepKind` → `engine/step_registry/` → implement under `engine/exec/` → plan it from the handler. Do not put business logic in the CLI.

Canonical modular domains: `social/`, `clarity/`.

---

## Task Lifecycle Contract

Before creating, queuing, reviewing, or spawning tasks, pick the lane:

| Lane | Source of truth | Reuse |
|------|-----------------|-------|
| User/CLI starts existing work | `engine/queue.rs`, package/submit CLI paths | Domain queue APIs |
| System creates a task | `engine/spawner.rs` + `task_definitions.rs` | `TaskSpawner::spawn` |
| Backend follow-ups after success | `engine/post_actions/` | `TaskSpawner::spawn_follow_up`; auto-enqueue only when `run_policy` is `auto_enqueue` |
| User must choose before follow-ups | `task_definitions` + CLI selection | Parent → `review`; options as artifacts; spawn only after selection |
| Results only, no spawn | `task_definitions` | `artifact_review`, `follow_up_policy = None` |

**Hard rules:** enqueue via domain APIs (no ad-hoc executor runners); user-selection follow-ups never spawn before the user chooses; every generated task needs an idempotency key unless intentionally one-off; after task-creation changes run `pnpm run check:task-store`.

---

## Deterministic vs Agentic

- **Deterministic** if a finite rule set works for all valid inputs (API calls, filter/sort/group, file apply, verify).
- **Agentic** if intent, tradeoffs, prose, or open-ended selection is required.
- **Hybrid (preferred for audits/fixes):** deterministic context → agentic structured output → deterministic apply/verify.

Never dump raw bulk data into an LLM when sort/filter would suffice. New agentic steps need: specific input context, an output contract, and a comment on why they cannot be deterministic. Placeholders stay `manual` until real.

Canonical per-article fix pipeline: context → `Extractor<T>` generate → apply (snapshot/restore) → verify. See playbook → "Building a Per-Article Fix Pipeline".

---

## Operator Skills (Not Rust Orchestrators)

Weekly SEO and video clips are **agent skills**, not in-repo schedulers:

| Skill | Canonical path |
|-------|----------------|
| Weekly SEO | `.agents/skills/weekly-seo/SKILL.md` |
| Weekly SEO status | `.agents/skills/weekly-seo-status/SKILL.md` |
| Video clip | `.agents/skills/video-clip/SKILL.md` |

`.grok/skills/*` entries are discovery symlinks — edit only the `.agents` files. Do not build a Rust weekly orchestrator or cross-project runner for these.

App-default **prompt** skills for task execution live under `crates/pageseeds-core/skills/` (registered in `engine/skills.rs`). Project `.github/skills/` overrides still work; drift is warned when versions differ.

---

## Anti-Pattern

Rebuilding article writing as a new handler + many step kinds for output that is still MDX is the classic failure mode. **Reuse `write_article` + a skill.** Scenario recipes and validation steps: [playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md).

---

## Ship Checklist

- [ ] Reuse existing primitives / task types before inventing new ones
- [ ] Lifecycle lane + `task_definitions.rs` correct if tasks are involved
- [ ] No business logic in `pageseeds-cli` beyond I/O
- [ ] Migrations additive; secrets via `EnvResolver`
- [ ] `pnpm test:cli` green

---

## See Also

| Need | Doc |
|------|-----|
| Scenario recipes (skill, write, queue, fix pipeline, CLI) | [AGENT_DEVELOPMENT_PLAYBOOK](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) |
| Status values, migrations, link integrity, CLI contract | [CONTRACTS.md](./CONTRACTS.md) |
| Handlers, steps, executor | [WORKFLOW_ENGINE](./docs/WORKFLOW_ENGINE.md) |
| Which task type when | [TOOL_CATALOG](./docs/TOOL_CATALOG.md) |
| Rig / LLM integration | [AGENT_INTEGRATION](./docs/AGENT_INTEGRATION.md) |
| Product workflows | [BUSINESS_PROCESSES](./docs/BUSINESS_PROCESSES.md) |
| Free vs paid tools | [CLI_COMMERCIAL](./docs/CLI_COMMERCIAL.md) |
| Releases | [CLI_RELEASE](./docs/CLI_RELEASE.md) |
