# Implicit Contracts

This file documents runtime contracts, invariants, and hidden rules that are NOT enforceable by the compiler but WILL cause silent failures if violated. Read this before modifying `executor.rs`, `engine/workflows/`, `commands/`, or any content pipeline file.

---

## 1. Task Status Values

**Canonical set** (defined in `config/mod.rs::STATUSES`):

| Value | Meaning | Set by |
|---|---|---|
| `"todo"` | Ready to run | Initial state; reset on failure |
| `"in_progress"` | Currently executing | `executor.rs` at task start |
| `"review"` | Awaiting user decision | `executor.rs` — keyword research only |
| `"done"` | Completed successfully | `executor.rs` — most task types |
| `"cancelled"` | User dismissed | Frontend UI only |

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

Default phase per task type is set in `config::default_phase()`. Do not use phase strings not in this list — they will not appear in the UI phase filter and will be silently ignored.

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
| `"content_review_recommend"` | Hybrid: deterministic article scoring + single agentic recommendation call | `recommendations.json` | `content_audit.json`, articles.json |
| `"content_review_apply_execute"` | Agentic: reads recommendations artifact, applies changes to MDX files | Updated content files | `recommendations.json` |
| `"content_sync"` | Deterministic: validate articles.json ↔ content files | Validation report | Nothing |
| `"content_audit"` | Deterministic: 13-rule article check + health scoring | `content_audit.json` | articles.json |

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
| `"content_review"` | `content_review_apply` task | `create_content_review_apply_task()` in `executor.rs` |
| `"content_audit"` | `content_review_apply` task | same |
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
5. ContentReviewHandler     — matches: content_review, content_audit, content_review_apply, content_review_recommend, content_review_apply_execute, content_sync
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

**Locate precedence:** `project.content_dir` (if set) → heuristics scanning candidate paths. Always use `content::locator::find_content_dir()` — do not re-implement candidate scanning.

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

## 10. Commands Layer Must Remain Thin

**Rule:** `commands/*.rs` files are IPC adapters only. Each command does exactly:
1. Acquire state lock
2. Call one module function
3. Return `result.map_err(|e| e.to_string())`

**Current violations** (known technical debt — do not copy this pattern):
- `commands/reddit.rs::draft_reddit_reply` — contains inline prompt engineering (~100 lines). Should move to `reddit/prompts.rs`.
- `commands/reddit.rs::post_to_reddit` — writes to DB AND creates history file side-effects inline. Should move to `reddit/history.rs`.

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
