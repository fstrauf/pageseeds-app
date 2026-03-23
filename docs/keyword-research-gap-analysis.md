# Keyword Research — App vs CLI Gap Analysis

Reference implementation: `pageseeds-cli` Python dashboard  
Target: `pageseeds-app` Rust/Tauri desktop app

---

## Architecture Side-by-Side

| Dimension | CLI (Python) | App (Rust) |
|---|---|---|
| Entry point | `[k]` Quick Add → `create_research_task("keywords")` | TaskCreate modal → `create_task(type="research_keywords")` |
| Task type (standard) | `research_keywords` | `research_keywords` |
| Task type (agentic) | `custom_keyword_research` (themes in metadata) | `custom_keyword_research` (themes in description) |
| Execution | `ResearchRunner._run_agentic_keyword_research()` | `exec_keyword_research_cli()` subprocess |
| Keyword source (standard) | Calls `seo-content-cli research-keywords` via agent | Calls `seo-content-cli research-keywords` directly |
| Keyword source (agentic) | Agent calls `pageseeds seo keyword-generator` + `batch-difficulty` via LLM shell | Agent called via `copilot`/`claude` CLI |
| Post-execution status | `"review"` for both paths | `"review"` only for `research_keywords` — not `custom_keyword_research` |
| Selection UI | Rich terminal table + `1,3,5` / `all` / `none` prompt | `KeywordPicker` component in TaskDetail sheet |
| Task creation | `create_task(type="write_article", ...)` | `create_article_tasks_from_keywords` command |

---

## What Works in the App

### Pathway A — Standalone SEO tool (`KeywordResearch.tsx`)
Fully native Rust. Calls Ahrefs directly (requires `CAPSOLVER_API_KEY`). No task integration — ad-hoc lookup only.

### Pathway B — `research_keywords` task (happy path)
1. User creates task with themes in the description field
2. `exec_keyword_research_cli()` shells out to `seo-content-cli research-keywords --analyze-difficulty --top-n 10`
3. Stdout (JSON) stored as artifact `research_keywords_cli` on the task with `content = stdout`
4. Executor transitions task to `"review"`
5. TaskDetail shows `KeywordPicker` which parses `artifact.content` as `KeywordResearchResult`
6. User selects keywords → `createArticleTasksFromKeywords` → `write_article` tasks created
7. Research task marked `"done"`

---

## Confirmed Gaps

### Gap 1: `custom_keyword_research` is broken end-to-end

The executor only transitions `research_keywords` to `"review"`:

```rust
// executor.rs
let new_status = if all_ok {
    if task.task_type == "research_keywords" { "review" } else { "done" }
```

`custom_keyword_research` goes straight to `"done"` — the user never sees the keyword picker.

TaskDetail compounds this:

```tsx
// TaskDetail.tsx
{task.type === 'research_keywords' && task.status === 'review' && (
  <KeywordPicker ... />
)}
```

`custom_keyword_research` tasks never render KeywordPicker regardless of status.

Additionally, `create_article_tasks_from_keywords` in Rust looks for artifact key `research_keywords_cli`:

```rust
// commands/tasks.rs
let artifact = task.artifacts.iter().find(|a| a.key == "research_keywords_cli");
```

But for `custom_keyword_research`, the handler produces artifacts named `research_agent_stage` and `research_normalize_stage` — the lookup always fails.

**Fix needed:** Extend `"review"` transition and `KeywordPicker` condition to cover `custom_keyword_research`. Change artifact lookup to check both `research_keywords_cli` and `research_normalize_stage`.

---

### Gap 2: `seo-content-cli` subprocess dependency

Pathway B shells out to the Python `seo-content-cli` binary, which must be installed separately on the user's PATH:

```rust
// executor.rs
let mut cmd = std::process::Command::new("seo-content-cli");
```

The Rust app already has native Ahrefs code in `seo/keywords.rs` (used by Pathway A), but Pathway B doesn't use it. The native Rust code solves CAPTCHAs via CapSolver, calls `stGetFreeKeywordIdeas` and `stGetFreeSerpOverviewForKeywordDifficultyChecker` directly.

Pathway A calls directly from `commands/seo.rs` → `seo/keywords.rs`.  
Pathway B does the same work via Python subprocess.

**Fix needed:** Implement a native Rust `exec_keyword_research_native()` step that calls `seo/keywords.rs` functions directly (batch keyword ideas → dedup against articles.json → batch difficulty), removing the Python dependency from Pathway B.

---

### Gap 3: Stdout parsing is fragile

`exec_keyword_research_cli` captures the full stdout of `seo-content-cli` and stores it as the artifact content:

```rust
output: Some(stdout),
```

`KeywordPicker` then does `JSON.parse(artifact.content)`. If `seo-content-cli` emits any progress/status text to stdout (e.g. "Fetching keywords…", "Done."), JSON.parse fails silently and the picker shows:

> "Could not parse keyword research output."

The Python CLI itself uses Rich console output to stderr and writes clean JSON to stdout, so this may not fire in practice — but there's no explicit stdout/stderr separation check in the Rust executor.

**Fix needed:** Either parse JSON from within the stdout string (extract the JSON block), or verify that `seo-content-cli research-keywords` always produces clean JSON on stdout.

---

### Gap 4: No theme input UI when creating `research_keywords` task

The CLI has a dedicated `create_custom_keyword_research_task()` path that prompts the user for themes, criteria, min_volume, max_kd, and exclusions — all stored in `task.metadata`.

In the app, `TaskCreate.tsx` collects only `type`, `title`, and `priority`. Themes must be entered manually in the `description` field and the executor parses them as one-per-line. There's no labelled "themes" input, no criteria/exclusion fields, and no documented hint to explain the description format.

**Fix needed:** Add a `research_keywords`-specific field in TaskCreate for themes (comma or newline separated), stored in description. Optionally expose min_volume/max_kd.

---

### Gap 5: No workspace pre-flight check

The executor calls `seo-content-cli` without first verifying that the project's `automation/articles.json` exists (required by the CLI's `--workspace-root` logic). When it's missing, the CLI exits non-zero and the error message is a raw Python traceback, not a user-friendly hint.

The Python CLI has `check_project_setup` guards and the `seo-local-setup` SKILL to create the workspace first.

**Fix needed:** Before calling `seo-content-cli`, check for `{automation_dir}/articles.json` and return an actionable error if missing, pointing to the workspace setup step.

---

### Gap 6: Opportunity scoring differs from CLI reference

The CLI scores keywords:
- **High** — KD ≤ 10 AND volume `1000+`
- **Medium** — KD ≤ max_kd AND volume `100+` or `1000+`
- **Low** — everything else

And pre-selects `high` keywords by default.

`KeywordPicker` pre-selects keywords where `kd == null || kd < 50` — no volume component in the pre-selection. The opportunity bucketing labels (Very Easy / Easy / Medium / Hard / Very Hard) are KD-only and don't map to the three-tier opportunity model.

**Fix needed (minor):** Update `KeywordPicker` default selection to pre-check keywords with KD < 30 (easier) or add a combined score column if volume data is available.

---

### Gap 7: No optimization candidate flow

The Python CLI also surfaces `optimize_candidates` from the research output — existing pages that could be improved for a target keyword — and lets the user create `optimize_article` tasks for them.

The app's `KeywordResearchResult` type has no `optimize_candidates` field and `KeywordPicker` only handles new article tasks.

**Impact:** Low — this is a secondary feature and the data doesn't come through `seo-content-cli research-keywords` output anyway.

---

## Priority Order for Fixes

| # | Gap | Impact | Effort |
|---|---|---|---|
| 1 | `custom_keyword_research` broken end-to-end | High — entire task type unusable | Low — 3 targeted changes |
| 2 | Theme input UI | Medium — UX friction, user must know description format | Low — add field to TaskCreate |
| 3 | Workspace pre-flight check | Medium — confusing errors when articles.json missing | Low — `Path::exists()` check |
| 4 | Stdout parsing fragility | Medium — silent failure if CLI emits non-JSON | Low — JSON extraction helper |
| 5 | Opportunity scoring pre-selection | Low — cosmetic difference | Low |
| 6 | Native Rust pipeline (remove Python dependency) | High — but only when seo-content-cli not installed | High — requires porting the full pipeline |
| 7 | Optimize candidates flow | Low | Medium |

---

## Files to Change per Fix

| Fix | Files |
|---|---|
| Fix 1 — custom_keyword_research review flow | `src-tauri/src/engine/executor.rs` (extend review transition), `src/components/tasks/TaskDetail.tsx` (extend condition), `src-tauri/src/commands/tasks.rs` (extend artifact lookup) |
| Fix 2 — theme input UI | `src/components/tasks/TaskCreate.tsx` |
| Fix 3 — workspace pre-flight | `src-tauri/src/engine/executor.rs` (`exec_keyword_research_cli`) |
| Fix 4 — stdout JSON extraction | `src-tauri/src/engine/executor.rs` (`exec_keyword_research_cli`) |
| Fix 5 — opportunity scoring | `src/components/tasks/KeywordPicker.tsx` |
| Fix 6 — native pipeline | `src-tauri/src/engine/executor.rs`, `src-tauri/src/seo/keywords.rs`, new dedup + difficulty batch fn |
