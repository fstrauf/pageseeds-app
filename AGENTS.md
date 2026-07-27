# AI Agent Guide — PageSeeds Operator CLI

Concise orientation and rules for AI agents working in this repo.

> **Fast path:** If you already know what you're building, see the [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) for concrete file paths and validation steps.  
> **Deep context:** For business workflows, execution mechanics, data architecture, and LLM integration, see the docs listed under [See Also](#see-also).

---

## What This Repo Is

**PageSeeds Operator CLI** — a pure Rust workspace for SEO operator tooling. The Tauri desktop shell and React frontend were removed in **#184** (historical releases only; not rebuilt here).

- **Domain library**: `crates/pageseeds-core` (`pageseeds_core`) — no UI/Tauri deps
- **Operator CLI**: `crates/pageseeds-cli` (`pageseeds-cli` bin) — depends only on core
- **Store**: SQLite (operator runtime state) + JSON in the user's repo (committed content data)
- **LLM layer**: Rig-core providers with a legacy CLI fallback (`crates/pageseeds-core/src/rig/`, `engine/agent.rs`)

> **Ship gate (local):** `pnpm run test:cli` / `pnpm test:all` (`cargo test -p pageseeds-core` → `check:task-store` → `check:cli-contract`). Also: `cargo build -p pageseeds-cli`. Install path: `scripts/install-cli.sh` / `docs/CLI_GETTING_STARTED.md`. Releases: [docs/CLI_RELEASE.md](./docs/CLI_RELEASE.md).

---

## Fast Path: Common Scenarios

| If you need to... | Use this path | Do NOT |
|---|---|---|
| **Adjust how an AI writes/reviews content** | Edit the embedded skill in `crates/pageseeds-core/skills/{skill}/SKILL.md` — it is the single source of truth for app-default skills (registered in `engine/skills.rs`). Project-level `.github/skills/{skill}/SKILL.md` overrides still work for per-project customization, but trigger a drift warning at load time when an embedded counterpart exists and its version marker differs. Test with `build_prompt_preview` before touching executor logic; validate with unit tests + Path B package/submit floors (not live LLM evals). | Add a new task type or handler just to change the prompt |
| **Run the weekly SEO pass on a project** | Invoke the `weekly-seo` skill (`.agents/skills/weekly-seo/SKILL.md`, discoverable by Kimi Code) — **desk-first** (epic #117): refresh if stale → `site-overview` → `articles`/`article`/`gsc-queries` → ≤5 actions → report. Soft audits optional, not ground truth. Skill is the workflow; judgment lives in the agent. | Build a Rust orchestrator, scheduler, or cross-project runner; treat soft clusters as merge authority |
| **Add a new content-writing behavior** | Reuse `write_article` + `ContentHandler` + a `skill` param. | Add a new handler unless the step graph changes |
| **Add or change task lifecycle behavior** | Follow the [Task Lifecycle Contract](#task-lifecycle-contract), then update `config/task_definitions.rs`, `engine/post_actions.rs`, or the user-selection command as appropriate. | Encode lifecycle rules in a component, executor special case, or ad-hoc task factory |
| **Attach tasks to execution** | Use the engine queue (`engine/queue.rs`) or CLI task/submit paths; enqueue via domain APIs. | Bypass the queue with ad-hoc executor calls from scripts |
| **Programmatically create tasks** | Use `TaskSpawner::spawn` or `TaskSpawner::spawn_follow_up`. | Call `task_store::create_task` directly |
| **Pass downstream context/data** | Use task artifacts + deterministic prep steps. | Make the agent rediscover file paths or metrics from prose |
| **Get JSON from an agent** | Use shared extraction helpers (`engine::text` in Rust). | Write new regex parsers or normalizer paths |
| **Sync article state to repo JSON** | Use `db::export::write_articles_to_repo()` or `content::ops::sync_and_validate()`. | Write raw SQL + manual `fs::write` of `articles.json` |
| **Add a new workflow step** | Add `StepKind` variant → register in `step_registry.rs` → add match arm in `executor.rs` → implement in `engine/exec/`. | Put business logic in the CLI binary or the handler |

> **weekly-seo skill source of truth:** Canonical path is `.agents/skills/weekly-seo/SKILL.md`. `.grok/skills/weekly-seo/SKILL.md` is a symlink for Grok discovery — edit only the `.agents` file.  
> **video-clip skill:** Article → short via `/video-clip` (`.agents/skills/video-clip/SKILL.md`; `.grok` symlink). Edit only the `.agents` file.

**Golden rules:**
1. A new skill file is ~20 lines. A new task type + handler + exec module is ~200+ lines. **Prefer the skill.**
2. When the output is an MDX article, reuse `write_article` with a different skill — do not build a new pipeline.
3. The queue is backend-managed. CLI/operators enqueue or execute via documented paths; do not invent parallel runners.
4. **NEVER re-implement a primitive that already exists.** Before writing `split_whitespace().count()`, `parse_frontmatter()`, or a slug normalizer, check the [DRY catalog](#dry-core-reusable-functions). The canonical version strips markdown, handles edge cases, and is the single source of truth. Fragmentation creates bugs that surface weeks later in unrelated workflows.

---

## Directory Map

```
Cargo.toml                 # workspace: pageseeds-core, pageseeds-cli
crates/
├── pageseeds-core/        # Domain library (rlib only)
│   ├── skills/            # Embedded app-default skills (include_str!)
│   ├── config/            # tool_catalog.toml
│   └── src/
│       ├── db/            # SQLite init, migrations, JSON repo export
│       ├── models/        # Serde structs (CLI + domain)
│       ├── engine/        # Workflow orchestration: store, spawner, executor, handlers
│       │   ├── workflows/ # Handler trait + step plans
│       │   └── exec/      # Step implementations
│       ├── config/        # Constants, task definitions, env resolver
│       ├── content/       # MDX file operations
│       ├── reddit/, gsc/, seo/, social/, clarity/, rig/, license/, …
│       └── lib.rs         # Domain modules only
└── pageseeds-cli/         # Operator CLI bin (depends only on core)
    └── src/main.rs
scripts/
├── install-cli.sh         # Customer + contributor install path
├── check-cli-contract.sh
└── check-task-store-usage.sh
```

For the full tree and per-folder responsibilities, see [`AI_QUICK_START.md`](./AI_QUICK_START.md).

---

## Core Rules

### Rust Domain + CLI

1. **Business logic lives in `pageseeds-core` modules** — keep `pageseeds-cli` thin (parse args → call core → print JSON/errors).
2. **One error type**. Use `error::Error` and `error::Result<T>` throughout the domain crate.
3. **SQLite is the operator runtime store**. All mutable state goes through `engine/task_store.rs`. Schema changes require a new migration constant in `db/mod.rs` — never alter existing SQL migration blocks.
4. **Task creation goes through `engine::spawner::TaskSpawner`**. Never call `task_store::create_task` directly for programmatic task creation. The spawner enforces idempotency and dependency validation.
5. **Subprocess by tier**. Commercial-surface tools (free desk + paid) are pure Rust — they must work from the single prebuilt binary (`reqwest`, `rusqlite`, `walkdir`, `regex`, etc.). **Operator-tier tools** (marked in `docs/CLI_COMMERCIAL.md`; dev-machine only, not part of the commercial promise) may spawn external processes (node, ffmpeg, python) when the capability cannot be Rust-native — detect deps on PATH and fail with a clear install hint. The agent compatibility layer keeps its CLI fallbacks, and the LLM still gets no arbitrary shell (`docs/AGENT_INTEGRATION.md` "No Shell Escapes").

### RIG / LLM Integration

1. **Use standard `rig-core` primitives first.** Prefer Rig providers/completion models, `Extractor<T>`, `Tool`/tool sets, embeddings, and agents before writing custom HTTP loops, regex JSON extraction, bespoke tool registries, or prompt-only protocols.
2. **Keep Rig behind the local integration layer.** Put provider, extraction, tool, embedding, and agent adapters in `crates/pageseeds-core/src/rig/` (or the existing domain module that already wraps Rig).
3. **Structured output uses schemas.** New agentic steps that need JSON must define a typed Rust output struct with `serde` + `schemars::JsonSchema` and use a Rig extractor or equivalent typed extraction wrapper.
4. **Tools use Rig tool abstractions.** If a model needs to call Ahrefs, GSC, Reddit, file analysis, or other deterministic capabilities, expose them as typed Rig tools.
5. **Provider fallback is centralized.** Any fallback to `agent-wrapper`, CLI providers, or compatibility shims belongs in the Rig/provider layer and must be documented as temporary.
6. **Test without live providers by default.** Unit tests for Rig-backed code should use fixtures, mock providers, or mocked tools. Live tests must be `#[ignore]`.

### Secrets

- Precedence: `~/.config/automation/secrets.env` → `{repo}/.env.local` → `{repo}/.env` → shell vars.
- Managed by `config/env_resolver.rs` (`EnvResolver`). Use it everywhere; don't read env vars directly.
- Never embed keys or paths in code. Operators manage secrets via env files / CLI setup; use `EnvResolver`.

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
| **User / CLI starts an existing task** | `crates/pageseeds-core/src/engine/queue.rs`, CLI package/submit paths | Enqueue or execute via domain queue APIs; backend persists queue rows and runs the executor. | queue APIs, CLI task tools |
| **System creates a task** | `crates/pageseeds-core/src/engine/spawner.rs` | Build a `TaskSpec`; defaults come from `config/task_definitions.rs`; idempotency prevents duplicate active work. | `TaskSpawner::spawn` |
| **Task creates backend follow-ups after success** | `crates/pageseeds-core/src/engine/post_actions.rs` | `after_task_success` runs after executor completion and returns created task IDs; queue auto-enqueues only follow-ups whose `run_policy` is `auto_enqueue`. | `TaskSpawner::spawn_follow_up` |
| **Task requires user input before follow-ups** | `config/task_definitions.rs` + CLI selection tools | Parent task gets `review_surface != none` and usually `follow_up_policy = UserSelection`; executor leaves it in `review`; selection validates choices, creates downstream tasks, marks parent done. | Existing patterns: keyword picker, Reddit picker, cannibalization / content-review select |
| **Task should only show results, not spawn work** | `config/task_definitions.rs` | Use `artifact_review` and `follow_up_policy = None`. | Existing artifact review surfaces |

**Hard rules:**
- `config/task_definitions.rs` owns `run_policy`, `review_surface`, `follow_up_policy`, and `handler_family`. Do not duplicate those decisions in ad-hoc CLI branches or executor special cases.
- Enqueue via domain APIs; do not execute outside the queue/executor contract.
- Backend follow-ups live in `engine/post_actions.rs` or a domain module called from it.
- User-selection follow-ups must not be spawned before the user chooses. Store selectable options as artifacts, route the completed parent to `review`, and create downstream tasks from the selection path.
- Every generated task needs an idempotency key unless it is intentionally one-off.
- Run `pnpm run check:task-store` after task-creation changes.

---

## Overview Tool Catalog

The operator tool catalog is **task types**, not raw function calls — enqueue/execute via the engine queue or documented CLI paths; never invent parallel runners. Full per-tool reference: [`docs/TOOL_CATALOG.md`](./docs/TOOL_CATALOG.md).

> **Desk model (epic #117 / #139):** Weekly organic growth uses the [weekly-seo skill](./.agents/skills/weekly-seo/SKILL.md) — Site State reads then few hard actions. **Do not** nest `content_review` as the weekly strategy brain. Specialist audits below are when **already scoped**; soft audits are optional, not the weekly spine.

**When to use which (quick guide):**

| Situation | Tool (task type) |
|---|---|
| Weekly organic growth / explore then act | weekly-seo skill **desk only**: site-overview → articles/article + GSC → hard actions (e.g. `fix_content_article -S`) |
| Make a vertical short from an article | `/video-clip` skill (`.agents/skills/video-clip/`) — context → craft → render → quality gate |
| Need new blog/informational topics | `research_keywords` |
| Need new conversion/landing pages | `research_landing_pages` |
| "Something is underperforming" (unknown cause) | **CLI:** desk reads → targeted `fix_content_article`. **UI/unattended:** `content_review` umbrella when a nested task is wanted — not every specialist audit |
| Low click-through rate from search | Desk → targeted `fix_content_article` (CLI/weekly best-path). Full `ctr_audit` is UI/unattended BackendAuto fan-out — not default for agent/CLI weekly |

| Your own pages compete for the same query (hard evidence) | `cannibalization_audit` |
| Pages exist but Google hasn't indexed them | `indexing_health_campaign` |
| Want UX/behavioral signals (Clarity) | `clarity_analytics` |
| Weekly Reddit audience engagement | `reddit_opportunity_search` |
| Broken MDX structure (headings, frontmatter) | `content_cleanup` |
| Rename frontmatter fields (`metaDescription` → `description`) | `sanitize_content` |
| Plan a feature for this app | `generate_feature_spec` |

Rules:
- **Desk-first for weekly/CLI.** Explore with Site State reads, then hard actions. Specialist audits (`ctr_audit`, `cannibalization_audit`, `indexing_health_campaign`, `clarity_analytics`) only when already scoped (e.g. hard same-query evidence). Soft clusters are not ground truth.
- **`content_review` is UI/unattended umbrella** when a nested investigation task + picker is desired — not the weekly-seo skill's strategy brain (CLI: desk → `fix_content_article`).
- **Low CTR for agent/CLI weekly:** prefer desk-selected `fix_content_article` over enqueueing full `ctr_audit` (BackendAuto spawns many children and burns execution budget). Do not flip AutoEnqueue/BackendAuto defaults — UI unattended path stays.

- **Collection tasks (`collect_gsc`, `collect_clarity`) are `AutoEnqueue`** — the system runs them. CLI weekly path may create+execute when desk data is stale.
- **Lifecycle metadata is owned by `config/task_definitions.rs`.** That file wins when docs or CLI help disagree.

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
| `slug::resolve_slug()` | `content/slug.rs` | Bare `set.contains(&normalize_url_slug(...))` — exact match first, normalized fallback |
| `redirects::load_redirect_source_slugs()` | `content/redirects.rs` | Ad-hoc `redirects.csv` parsing |
| `date_policy::find_first_free_past_date()` | `content/date_policy.rs` | Backward-cursor date loops |
| `date_policy::suggest_next_safe_date()` | `content/date_policy.rs` | Reading dates and walking backward manually |
| `ingest_orphan_files()` | `content/ops.rs` | A new "save article to DB + disk" function |
| `sync_and_validate()` | `content/ops.rs` | Ad-hoc file discovery or date-sync logic |
| `read_file_metadata()` | `content/ops.rs` | Inline `fs::read_to_string` + regex word counting |
| `load_article_by_slug()` | `content/ops.rs` | Manual article lookup + path construction |
| `resolve_content_dir()` | `content/ops.rs` | Content path guessing |
| `slug_from_filename()` | `content/ops.rs` | String manipulation on filenames |
| `apply_publish()` | `content/publish.rs` | Any publish/status-change workflow |
| `content_health_check()` | `content/ops.rs` | One-off file existence checks outside ops |
| `keyword_match::normalize_keyword()` / `keyword_present()` / `keyword_occurrences()` | `content/keyword_match.rs` | Raw `contains()` / `matches()` keyword checks (stored keywords may contain quotes or long phrases) |

### Project / Site URL (`models/project.rs`)

| Function | File | Use instead of writing... |
|---|---|---|
| `site_base_url()` | `models/project.rs` | Any inline `sc-domain:` → `https://` conversion or `format!("{}/sitemap.xml", site_url)` — `site_url` stores the GSC property ID and is **not** always a fetchable URL. GSC API calls are the only exception (they need the raw property ID) |
| `validate_site_url()` | `models/project.rs` | Ad-hoc site_url validation at write boundaries |

### Database / Export (`db/`)

| Function | File | Use instead of writing... |
|---|---|---|
| `export::write_articles_to_repo()` | `db/export.rs` | Any `fs::write` of `articles.json` |
| `export::export_articles()` | `db/export.rs` | Manual SQL → JSON serialization |
| `export::merge_unknown_fields()` | `db/export.rs` | Naive JSON overwrite that drops extra fields |
| `insert_gsc_page_daily_snapshots()` / `gsc_page_daily_window_metrics()` | `db/mod.rs` | Any write/read of the append-only `gsc_page_daily` snapshot table (never DELETE from it) |
| `insert_content_outcome_result()` / `list_content_outcome_results()` | `db/mod.rs` | Raw SQL on `content_outcome_results` (outcome history) |
| `research_shortlist::mark_covered_for_keywords()` | `db/research_shortlist.rs` | Inline shortlist theme/seed matching when keywords become article tasks |
| `task_store::list_articles()` | `engine/task_store.rs` | Raw SQL `SELECT * FROM articles` |
| `task_store::load_valid_link_targets()` | `engine/task_store.rs` | Raw `load_project_slug_set()` for link-target checks — redirected slugs are not valid targets |

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
| `extract_blog_link_hrefs()` | `content/linking.rs` | Any new `/blog/` link regex — canonical + malformed patterns are shared here |
| `repair_blog_link_hrefs()` | `content/linking.rs` | Inline string replacement of link hrefs |

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

There are three places domain logic can live. Knowing which to use removes guesswork.

| Layer | File Pattern | Responsibility |
|-------|--------------|----------------|
| **CLI binary** | `crates/pageseeds-cli/src/` | Arg parse, license gate, call core, print JSON/errors. **No business logic.** |
| **Domain modules** | `crates/pageseeds-core/src/{domain}/` | Business logic, data access, external API calls |
| **Engine exec** | `engine/exec/{domain}.rs` | Deterministic step implementations called by the executor |
| **Workflow handlers** | `engine/workflows/handlers.rs` | Orchestration / planning: returns `Vec<WorkflowStep>` |

### Decision Tree

```
I have new logic — where does it go?
│
├─ Is it CLI UX (flags, stdout JSON, help text)?
│  └─→ crates/pageseeds-cli (thin; call core)
│
├─ Is it building a step graph for a task type?
│  └─→ engine/workflows/handlers.rs
│
├─ Is it executing a single workflow step?
│  └─→ engine/exec/{domain}.rs
│
└─ Everything else (API clients, parsers, DB access, algorithms)
   └─→ {domain}/ under pageseeds-core
```

The `social/` domain and the `clarity/` domain are the canonical examples of fully modularized domains.

---

## How to Add a Feature

For scenario-specific instructions (changing a skill, adding content behavior, wiring the queue, building a fix pipeline), see the [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md).

For workflow mechanics, data persistence, business processes, and LLM integration, see the docs listed under [See Also](#see-also).

At a high level:

1. **Port behavior, not architecture** — identify inputs/outputs first.
2. **Test the agent prompt before writing the executor**.
3. **One end-to-end CLI/domain run before expanding scope** — core must produce correct output first.
4. **Spec before code** — any feature touching 2+ files gets a spec in `docs/` first.
5. **Ship one thing at a time**.

---

## Anti-Pattern: Rebuilding Article Writing

The hub page case study in the old codebase is the canonical mistake: creating a brand-new handler, 7 new step kinds, and ~2,000 lines of duplicated persistence logic for output that is just an MDX article.

When the output is an MDX article, the answer is almost always **reuse `write_article` with a different skill**, not build a new pipeline. See the [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) → "Building a Per-Article Fix Pipeline" and "Scenario: Adding a New Content-Writing Behavior" for the correct patterns.

---

## Pre-Change Checklist

**Ship gate** (never improvise ad-hoc compositions — extend the matching `package.json` script):

| Work kind | Gate | Composition / notes |
|-----------|------|---------------------|
| **Operator / CLI / domain Rust** (all product work) | `pnpm test:cli` | `test:rust` (`cargo test -p pageseeds-core`) → `check:task-store` → `check:cli-contract`. `pnpm test:all` is an alias of `test:cli`. Missing operator checks go into **`test:cli`**. |

### Domain + CLI
- [ ] Checked for reuse against the DRY catalog above
- [ ] Task lifecycle contract checked (if creating/queuing/spawning tasks)
- [ ] Ship with `pnpm test:cli`
- [ ] `cargo check -p pageseeds-core` / `cargo build -p pageseeds-cli`
- [ ] `pnpm run test:rust` passes — especially `all_task_types_have_non_fallback_handler` (uses `cargo nextest` when installed; tests that mutate process env must hold `test_support::ENV_LOCK`)
- [ ] New SQLite columns added via a new migration, not by altering existing ones
- [ ] Settings placed correctly: user preferences → `global_settings`; project config → `projects`
- [ ] No business logic added to `crates/pageseeds-cli` beyond arg parse / I/O
- [ ] No secrets or absolute machine paths in source code
- [ ] Subprocess only in operator-tier tools or the agent compatibility layer; commercial-surface tools stay pure Rust
- [ ] Reviewed `CONTRACTS.md` for affected implicit contracts
- [ ] New task types added to `config/task_definitions.rs` before wiring handlers
- [ ] Every new agentic step has specific input context, output contract, and a comment explaining why it cannot be deterministic
- [ ] Every new deterministic step does not contain a hard-coded heuristic that substitutes for judgment

---

## See Also

- [`docs/CLI_GETTING_STARTED.md`](./docs/CLI_GETTING_STARTED.md) — Install + first desk read
- [`docs/CLI_RELEASE.md`](./docs/CLI_RELEASE.md) — CLI version bump, `cli-v*` tags, GitHub release
- [`docs/CLI_COMMERCIAL.md`](./docs/CLI_COMMERCIAL.md) — Free vs paid tools
- [`docs/AGENT_DEVELOPMENT_PLAYBOOK.md`](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) — Scenario-based development guide
- [`docs/BUSINESS_PROCESSES.md`](./docs/BUSINESS_PROCESSES.md) — What the product does and how workflows connect
- [`docs/WORKFLOW_ENGINE.md`](./docs/WORKFLOW_ENGINE.md) — Handlers, steps, executor, async execution
- [`docs/DATA_PERSISTENCE.md`](./docs/DATA_PERSISTENCE.md) — SQLite/JSON architecture
- [`docs/AGENT_INTEGRATION.md`](./docs/AGENT_INTEGRATION.md) — LLM integration with Rig
- [`AI_QUICK_START.md`](./AI_QUICK_START.md) — Full directory map and quick orientation
- [`CONTRACTS.md`](./CONTRACTS.md) — Runtime invariants and hidden rules
