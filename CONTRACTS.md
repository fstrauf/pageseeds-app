# Implicit Contracts

This file documents runtime contracts, invariants, and hidden rules that are NOT enforceable by the compiler but WILL cause silent failures if violated. Read this before modifying `executor.rs`, `engine/workflows/`, or any content pipeline file. (Historical Tauri `commands/` IPC layer was removed in #184.)

---

## 0. `not_before` delayed-execution gate

Tasks may carry an optional RFC3339 `not_before` timestamp (e.g. +14d/+30d
outcome reviews). Until that time the task must not run automatically.

**Enforced at two layers** (issue #307):

1. **Queue lease filter** — `engine/queue.rs` `get_next_pending_item` only
   leases rows where `not_before IS NULL OR not_before <= now`.
2. **Executor root** — `execute_task_with_token(..., opts)` refuses execution
   when `not_before` parses as strictly after `Utc::now()` and
   `opts.ignore_not_before == false`. Error message includes the due timestamp
   and points at `--force`. Batch `get_ready_tasks` applies the same due check
   in-memory so the scheduler never picks a not-due task. Shared helper:
   `executor::task_is_due`.

Unparseable / missing `not_before` is treated as due (fail open). CLI
`execute-task --force` maps to `ExecuteOpts { ignore_not_before: true, .. }`;
batch/queue always pass `ExecuteOpts::default()`.

---

## 1. Task Status Values

**Canonical set** (defined in `config/mod.rs::STATUSES`):

| Value | Meaning | Set by |
|---|---|---|
| `"todo"` | Ready to run | Initial state; reset on failure |
| `"in_progress"` | Currently executing | `executor.rs` at task start |
| `"review"` | Awaiting user decision | `executor.rs` — keyword research only |
| `"done"` | Completed successfully | `executor.rs` — most task types |
| `"cancelled"` | User dismissed | Operator / CLI cancel path |

**Critical rule:** Tasks that finish with `"review"` are defined in `config/task_definitions.rs` via `review_on_success: true`. Currently:
- `research_keywords`
- `custom_keyword_research`
- `research_landing_pages`
- `reddit_opportunity_search`

All other task types transition `in_progress → done` on success, and `in_progress → todo` on failure.

```rust
// executor.rs
let new_status = completed_task_status(&task.task_type, all_ok);
```

**If you add a new task type that should go to `"review"`, set `review_on_success: true` in its `TaskDefinition`. Do not edit `executor.rs` directly.**

---

## 2. Task Phase Values

**Canonical set** (defined in `config/mod.rs::PHASES`):

```
"collection" | "investigation" | "research" | "implementation" | "verification"
```

Default phase per task type is set in `config::default_phase()`. Do not use phase strings not in this list.

---

## 3. Task Execution Modes

**Canonical set:**

| Value | Meaning |
|---|---|
| `"automatic"` | Runs in batch without user intervention |
| `"batchable"` | Can run in batch; user can also trigger manually |
| `"manual"` | User must trigger explicitly |
| `"spec"` | Requires a spec artifact before execution (write_article, optimize_article) |

**Do not use `"auto"` — use `"automatic"`**. The batch runner checks for `"automatic"` and `"batchable"` explicitly.

---

## 4. Workflow Step Kind Contract

Steps are defined by handlers in `engine/workflows/handlers.rs` and executed by `executor.rs`. Each step has a `kind` field:

| Kind | What it does | Produces | Consumes |
|---|---|---|---|
| `"agentic"` | Calls the LLM agent | Sets `latest_raw_output` | Nothing |
| `"normalizer"` | Parses `latest_raw_output` into structured JSON | Artifact | **Consumes + clears** `latest_raw_output` |
| `"deterministic"` | Runs a CLI step | Optional output | Nothing |
| `"manual"` | Marks task as requiring user action | Nothing | Nothing |
| `"reddit_search"` | Deterministic: Reddit API search + engagement/accessibility scoring | Persists raw posts to DB | Nothing |
| `"reddit_enrich"` | Agentic: relevance scoring, pain point extraction, reply drafting (batched, needs conn) | Updates DB rows | Posts from `reddit_search` |
| `"gsc_summarise"` | Deterministic: group gsc_collection.json by reason_code, count, pick examples | Writes `gsc_summary.json` | Nothing |
| `"gsc_investigate_agentic"` | Agentic: interpret gsc_summary.json patterns, recommend fixes | Investigation artifact | `gsc_summary.json` |
| `"collect_gsc_inspect"` | Deterministic: GSC URL Inspection API + classification + task spawning | `gsc_collection.json`, fix tasks | Nothing |
| `"gsc_sync_articles"` | Deterministic: fetch GSC analytics → update articles.json | Updated articles.json | Nothing |
| `"keyword_research_cli"` | Deterministic: Ahrefs keyword API calls + dedup + ranking | Keyword JSON artifact | Optional theme artifact |
| `"content_review_recommend"` | Hybrid: deterministic article scoring + single agentic recommendation call | `recommendations.json` | content_audit_runs (DB), articles.json |
| `"content_sync"` | Deterministic: validate articles.json ↔ content files | Validation report | Nothing |
| `"content_audit"` | Deterministic: 21-check article audit + health scoring | SQLite `content_audit_runs` (no JSON write) | articles.json |

**The `reddit_enrich` step requires database access and is handled inline in the executor outer loop** (not inside `run_step`). The same pattern applies to `reddit_search` data persistence. These steps return a placeholder `StepResult` from `run_step`; the real work runs in the outer loop keyed on `step.kind`.

**The agentic → normalizer ordering is mandatory.** The executor passes `latest_raw_output` to the normalizer step. If the normalizer runs without a preceding agentic step (e.g. steps are reordered), `latest_raw_output` is `None` and the normalizer records 0-length output without error.

```
// executor.rs lines ~141-151
if step.kind == "agentic" {
    latest_raw_output = result.output.clone();
} else if step.kind == "normalizer" {
    // consumes latest_raw_output, then clears it
    latest_raw_output = None;
}
```

**Step params consumed by executor:**

| Param key | Used by step kind | Purpose |
|---|---|---|
| `"skill"` | `"agentic"` | Loads the named SKILL.md as the prompt |
| `"normalizer_id"` | `"normalizer"` | Selects which normalizer to run |
| `"artifact_name"` | `"normalizer"` | Names the saved artifact file |
| `"runner"` | `"deterministic"` | Selects the CLI runner to invoke |

---

## 4.1 Deterministic vs Agentic Mode Contract

Use these modes intentionally, never as a convenience fallback:

| Mode | Use when | Must not be used for |
|---|---|---|
| Deterministic | Inputs/outputs are machine-checkable and repeatable | Interpreting ambiguous strategy text, broad cluster labels, or intent-heavy planning |
| Agentic | Judgment is required (theme curation, prioritization, intent interpretation) | Calling stable APIs that already have deterministic code paths |

### `research_keywords` (required two-mode behavior)

`research_keywords` is explicitly split into two paths:

1. **Deterministic-only path** — if task description already contains valid themes:
    - Step plan: `research_keywords_cli`
    - Theme source: parsed task description

2. **Agentic + deterministic path** — if task description has no valid themes:
    - Step plan: `research_theme_selection_agent` → `research_keywords_cli`
    - Agent step output contract: JSON with `themes[]`
    - Deterministic step consumes the persisted artifact key `research_theme_selection_agent`

Critical invariants:

- `research_keywords_cli` must **not** silently fall back to broad heading extraction from briefs.
- If neither explicit themes nor agentic theme artifact is present, task must fail with a clear message.
- Agentic theme selection exists to avoid generic drift (for example broad umbrellas like "Risk Management" or "Advanced Topics").

---

## 5. Auto-Spawned Follow-Up Tasks

Certain task types automatically create follow-up tasks when they complete successfully. **Do not create these manually — they will be duplicated.**

| Task type | Auto-spawns | Spawning function |
|---|---|---|
| `"content_review"` | Stores proposals; user spawns `fix_content_article` via picker | `build_and_store_proposals_artifact` + selection command |
| `"content_audit"` | None (deterministic helper; topic-health reducer + IHC retry only) | `post_actions` |
| `"collect_gsc"` | Fix tasks from `gsc_collection.json` artifact | `create_tasks_from_collection_after_exec()` in `executor.rs` |
| `"research_keywords"` | Adds self to follow-up list (for UI review picker) | Inline at `executor.rs` ~line 271 |

---

## 6. Handler Registry Order

Handlers in `engine/workflows/handlers.rs::default_handlers()` are matched **first-match-wins**. The order is load-bearing:

```
1. CollectionHandler        — matches: collect_gsc
2. InvestigationHandler     — matches: investigate_gsc
3. ResearchHandler          — matches: research_keywords, custom_keyword_research, research_landing_pages
4. ContentHandler           — matches: write_article, optimize_article
5. ContentReviewHandler     — matches: content_review, content_audit, content_review_recommend, content_sync
6. RedditHandler            — matches: reddit_search, reddit_reply
7. PerformanceHandler       — matches: gsc_performance
8. ImplementationHandler    — matches explicit list + ANYTHING starting with "fix_"
9. ManualFallbackHandler    — matches: everything (fallback)
```

**Rules:**
- `ManualFallbackHandler` MUST remain last — it matches unconditionally.
- `ImplementationHandler` uses `t.starts_with("fix_")` as a catch-all. Any handler that should match a `fix_*` task type must be inserted **before** `ImplementationHandler`.
- New handlers go BEFORE `ImplementationHandler` and BEFORE `ManualFallbackHandler`.

---

## 7. Content Pipeline Execution Order

The content operations in `content/` assume this execution order. Skipping or reordering steps produces incorrect output:

```
1. Locate    — resolve content directory (project override > heuristics)
2. Sync      — reconcile articles.json ↔ MDX files on disk
3. Validate  — check dates, structure, duplicates
4. Audit     — SEO health analysis
5. Publish   — preflight + apply
```

**Locate precedence:** config-aware resolution via `content::ops::resolve_content_dir` (`seo_workspace.json` `content_dir` → project override → heuristics). Publish-content (`publish_by_slugs`) uses the same resolver. Prefer `resolve_content_dir` / `content::locator` — do not re-implement candidate scanning.

**Sync side effect:** `content::ops::sync_articles()` only writes `articles.json` if the sync is clean. A failed sync leaves the file unchanged.

---

## 8. Reddit Enrichment Loop

When `executor.rs` executes a `"reddit_search"` step and it succeeds, it **immediately** triggers an inline enrichment loop before proceeding to the next step:

```
// executor.rs: after redis_search step succeeds
loop {
    let pending = COUNT(*) WHERE reply_text IS NULL AND reply_status != 'skipped';
    if pending == 0 { break; }
    exec_reddit_enrich(conn, project_id, project_path, agent_provider);
}
```

This is intentional but invisible. **Do not add a separate enrichment step to the reddit_search handler** — the enrichment will run twice.

---

## 9. Task Step Progress Status Values

During execution, each step in `ExecutionResult.steps[]` has a `status` field. These are different from task statuses:

| Value | Meaning |
|---|---|
| `"pending"` | Not yet started |
| `"running"` | Currently executing |
| `"ok"` | Completed successfully |
| `"failed"` | Step failed |
| `"skipped"` | Step was optional and skipped |

---

## 10. Thin Adapters (CLI Only)

**Historical (desktop removed #184):** a Tauri `commands/*.rs` IPC layer existed and was required to stay thin (lock → one domain call → map error). That layer is gone; do not reintroduce it.

**Current rule:** `pageseeds-cli` is the only adapter. Keep it thin — parse args → call `pageseeds-core` → print JSON/errors. Business logic, prompts, DB side-effects, and file I/O belong in domain modules under `pageseeds-core`, never in the CLI binary.

---

## 11. Secrets / Env Var Resolution Order

All secrets must be resolved through `config::env_resolver::EnvResolver`. The precedence chain is:

```
1. ~/.config/automation/secrets.env   (highest — always wins)
2. {repo}/.env.local
3. {repo}/.env
4. Shell environment variables
```

**Do not read `std::env::var()` directly in new code.** Use `EnvResolver::resolve_key()`.

---

## 12. SQLite Migration Rules

- Never alter existing migration blocks in `db/mod.rs`.
- New columns or tables always get a new `MIGRATION_VN` constant and are applied after all prior migrations.
- All migrations must be idempotent (`CREATE TABLE IF NOT EXISTS`, `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`).
- The migration version is tracked implicitly by the order of `execute_batch()` calls in `db::init()`.

---

## 13. Internal Link Integrity

**Invariant:** a `/blog/<slug>` link committed to a project repo must resolve to a live project article at write time. A slug that appears in `.github/automation/redirects.csv` as a redirect **source** is NOT a valid link target, even though its article row and file still exist.

Enforcement points (do not bypass or re-implement):

- **Valid target set is computed in exactly one place:** `engine/task_store.rs::load_valid_link_targets()` = `load_project_slug_set()` minus redirect sources (`content/redirects.rs::load_redirect_source_slugs()`). Any code that validates a link target must use it — never the raw slug set.
- **Slug resolution is exact-match-first:** `content/slug.rs::resolve_slug()` checks the slug as written before falling back to `normalize_url_slug()`. A verbatim-existing slug with a leading number (e.g. `5-best-coffees`) must never be "normalized" into a different URL.
- **`url_slug` must be single-segment.** Hub convention is `hub-{topic}` (e.g. `hub-coffee`), never path form `hub/{topic}` or `/hub/{topic}`. Path-form values are invalid as stored identities and are hygienized at link format/resolve boundaries (`hub/coffee` → `hub-coffee` via `normalize_url_slug` / `format_blog_link`) so internal links use the final form.
- **Link detection patterns live in `content/linking.rs`** (canonical + malformed regexes, `extract_blog_link_hrefs()`, `repair_blog_link_hrefs()`). Do not add new `/blog/` link regexes elsewhere.
- **Every agentic content write** (`write_article`, `create_hub_page`, `refresh_hub_page`, `optimize_*`, `create_content`) ends with the deterministic `content_link_verify` step: resolvable filename-form hrefs are auto-repaired; an unresolvable link fails the step and the file is left untouched (all-or-nothing). Link verify fails when no written file exists — it never passes vacuously.
- **Every new-article write** (`write_article`, `create_content`, `create_hub_page`, `refresh_hub_page`) runs the deterministic `content_write_verify` step between the write stage and link verify: it fails the task when no article file was written and registered (issue #13 contract — never Done with zero output). Provider file-IO capability is defined in exactly one place (`rig/provider.rs::provider_supports_file_io()`).
- **Exact `target_keyword` collision hard-fails registration (issue #272):** Path B `write-submit` (`submit_written_article`) and nested write registration (`ingest_content_write_files` → `content_write_verify`) refuse a second live catalog owner of a normalized exact keyword. **Order:** check collisions **before** keyword stamp/export (Path B before `register_submitted_article`; nested inside `ingest_content_write_files` before tagging the written basename). Keyword meta is applied only to the submitted/written basename — co-ingested orphans demote to `draft` without inheriting K. Nested may leave an orphan draft **without** K when fail-closed; it must not leave a twin row with K. Gate returns collider identity (id/slug/title/page_type) plus retarget-or-consolidate resolution text; it does **not** auto-redirect, rewrite inbound, or spawn `consolidate_cluster`. Self is excluded by slug/id so re-submit/re-verify does not false-positive; empty/missing keyword skips the gate. Lookup: `content/article_index.rs::find_target_keyword_collisions` (uses `keyword_match::normalize_keyword` only).
- **Nested content write host policy (issue #143):** agentic steps that declare `PromptSection::ContentDirectives` (ContentHandler write/optimize) require a file-IO host (`grok`/`kimi`). This is the **sole** policy for nested content write: under text-only providers (`openai`/`claude`/`ollama`), `exec_agentic` fails loud early with a Path B (`write-context` / `write-submit`) pointer. There is no executor-write fallback that persists agent chat text as MDX. Nested `execute-task` is the unattended fallback and must use grok/kimi; CLI Path B is preferred for outer-agent quality. Structured-extraction fix paths are not gated. If a file-IO agent still produces no file, `content_write_verify` fails the task (issue #13).
- **`consolidate_cluster` must rewrite inbound links** to every redirected slug (`merge_rewrite_inbound_links`) before `merge_validate_output` asserts none remain.
- **Path B content fix-submit residual link policy (issue #195):** `engine/fix_package.rs` hard-gates newly introduced unresolvable `/blog/` links on `kind=content` only (CTR leaves the link gate OFF). Enforcement:
  - **Patch-time:** `apply_content_patch` refuses `changes.internal_links[].target_slug` values that do not resolve via `load_valid_link_targets` + `resolve_slug` — no MDX write on failure (`Error::Validation`).
  - **Post-apply residual:** capture pre-apply MDX; after apply, hard-fail only unresolvable `slug_written` values present in post content and **not** already present in pre content (`internal_links_new_resolve`). Pre-existing broken links may appear as non-blocking check detail and must not alone set `ok: false` (partial SERP fixes on legacy posts).
  - **Full agent rewrite baseline:** when pre-apply content equals post-apply content (agent already overwrote the file before `fix-submit`), every current unresolvable `/blog/` link hard-fails — there is no residual baseline. The submit message documents this so agents fix invented links.
  - **Fail-closed catalog load:** content fix-submit and write-submit must not treat `load_valid_link_targets` failure as “no targets → auto-pass”; load errors hard-fail submit.

**BUSINESS RULE (issue #203 — Path B content closed-loop):** Path B content ships schedule the same +30d `content_outcome_review` as nested write/fix/consolidate:

| Surface | Spawns `content_outcome_review`? | Parent for idempotency |
|---|---|---|
| `write-submit` success | Yes (submitted slug) | Bound write task, else synthetic `path-b:{project}:{slug}` |
| `merge-submit` success | Yes (**keeper slug only**) | Bound consolidate, else synthetic `path-b-merge:{project}:{keep_slug}` |
| `fix-submit` `kind=content` success | Yes | Synthetic `path-b-fix-content:{project}:{slug}` |
| `fix-submit` `kind=ctr` success | **No** — `ctr_outcomes` change event only | — |

Shared helper: `post_actions::spawn_content_outcome_review_for_slug`. Do **not** call full `after_task_success` from Path B. Re-submit is idempotent via `content_outcome_review:{project}:{slug}` (project + slug, not parent_id — issue #302).

---

## 14. CLI machine contract (`pageseeds-cli`)

`pageseeds-cli` is a machine-facing binary for agents and CI. Streams and exit codes are stable; do not "improve" them without a semver callout.

| Case | stdout | stderr | exit |
|---|---|---|---|
| **Success payload** | single JSON value | empty | **0** |
| **Usage / domain hard error** | empty | `ERROR: …` | **1** |
| **Outcome envelope** (validation / task result) | JSON with `ok` / `success` fields | empty | **0** even when `ok`/`success` is **false** — caller inspects JSON |
| **License deny** (when #156 lands) | empty | `ERROR: …` including buy URL | **1** (same path as hard error) |
| **Help** (`-h`/`--help`, `help`, no args, or `<tool> --help`) | human help text | empty | **0** |

**Path B note (do not break):** `write-submit` and `merge-submit` validation failures print JSON with `ok: false` and still exit **0**. Only domain/usage failures (missing file, bad flags, missing project) use `ERROR:` + exit 1.

**Semver surface:** flags and subcommand names are a breaking-change surface; renames must be called out in release notes. Smoke check: `scripts/check-cli-contract.sh` (via `pnpm run check:cli-contract`). Operator ship gate: `pnpm test:cli`.

**Hard-error helper:** `exit(msg)` in `crates/pageseeds-cli/src/main.rs` is the sole hard-fail path (`eprintln!("ERROR: …")` + exit 1). License deny (#156) must reuse it with a buy URL in the message — do not invent a separate exit channel.
