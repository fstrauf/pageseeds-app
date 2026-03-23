# Keyword Research — Fix Plan

Source: [keyword-research-gap-analysis.md](keyword-research-gap-analysis.md)

Ordered by impact vs. effort. Fixes 1–5 are targeted and low-effort; Fix 6 is a larger native pipeline port.

---

## Fix 1 — `custom_keyword_research` broken end-to-end

**Status:** Not started  
**Impact:** High — entire task type is unusable  
**Effort:** Low  

Three hardcoded `research_keywords` checks need to be widened to cover both types.

### 1a. Executor — extend `"review"` transition

**File:** `src-tauri/src/engine/executor.rs`

```rust
// Before
let new_status = if all_ok {
    if task.task_type == "research_keywords" { "review" } else { "done" }

// After
let new_status = if all_ok {
    if matches!(task.task_type.as_str(), "research_keywords" | "custom_keyword_research") {
        "review"
    } else {
        "done"
    }
```

### 1b. Executor — artifact key for `custom_keyword_research`

`custom_keyword_research` produces an artifact named `research_normalize_stage` (from the normalizer step), not `research_keywords_cli`. The artifact lookup in `commands/tasks.rs` must check both.

**File:** `src-tauri/src/commands/tasks.rs`

```rust
// Before
let artifact = task.artifacts.iter().find(|a| a.key == "research_keywords_cli");

// After
let artifact = task.artifacts.iter().find(|a| {
    matches!(a.key.as_str(), "research_keywords_cli" | "research_normalize_stage")
});
```

### 1c. Frontend — show `KeywordPicker` for both types

**File:** `src/components/tasks/TaskDetail.tsx`

```tsx
// Before
{task.type === 'research_keywords' && task.status === 'review' && (

// After
{(['research_keywords', 'custom_keyword_research'] as const).includes(task.type as never)
  && task.status === 'review' && (
```

---

## Fix 2 — Theme input UI in TaskCreate

**Status:** Not started  
**Impact:** Medium — UX friction; users must know the undocumented description format  
**Effort:** Low  

**File:** `src/components/tasks/TaskCreate.tsx`

- When `task_type === "research_keywords"` or `"custom_keyword_research"` is selected, show a labelled `Textarea` for themes (one per line or comma-separated) instead of the generic description field.
- Hint text: "Enter keyword themes, one per line (e.g. coffee brewing, espresso guides)"
- Write the value to the `description` field as newline-joined strings — the executor already parses it this way.
- Optionally expose `min_volume` and `max_kd` as numeric inputs stored in `description` as `min_volume:N` / `max_kd:N` prefixed lines (or defer to a later task metadata field).

---

## Fix 3 — Workspace pre-flight check before calling `seo-content-cli`

**Status:** Not started  
**Impact:** Medium — raw Python tracebacks surfaced to user when workspace not set up  
**Effort:** Low  

**File:** `src-tauri/src/engine/executor.rs` — `exec_keyword_research_cli()`

Add before the `Command::new("seo-content-cli")` call:

```rust
let articles_json = paths.automation_dir.join("articles.json");
if !articles_json.exists() {
    return StepResult {
        success: false,
        message: format!(
            "Workspace not initialised: {} not found. \
             Run 'Init Workspace' from Project Settings first.",
            articles_json.display()
        ),
        output: None,
    };
}
```

---

## Fix 4 — Extract JSON from stdout robustly

**Status:** Not started  
**Impact:** Medium — silent "could not parse" failures if CLI emits non-JSON lines to stdout  
**Effort:** Low  

**File:** `src-tauri/src/engine/executor.rs` — `exec_keyword_research_cli()`

After capturing stdout, attempt to extract the JSON block before storing it as the artifact:

```rust
fn extract_json_from_output(raw: &str) -> Option<&str> {
    // Find first '{' and last '}' to extract the outermost JSON object
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end > start { Some(&raw[start..=end]) } else { None }
}
```

Use this in the success branch of `exec_keyword_research_cli`:

```rust
let json_content = extract_json_from_output(&stdout)
    .unwrap_or(&stdout)
    .to_string();

StepResult {
    success: true,
    message: ...,
    output: Some(json_content),
}
```

---

## Fix 5 — Pre-selection scoring in `KeywordPicker`

**Status:** Not started  
**Impact:** Low — cosmetic difference from CLI reference scoring  
**Effort:** Low  

**File:** `src/components/tasks/KeywordPicker.tsx`

CLI pre-selects keywords with opportunity score `"high"` (KD ≤ 10 AND volume `1000+`) or `"medium"` (KD ≤ 40 AND volume `100+`). Current app pre-selects based on KD < 50 only.

```tsx
// Before
() => new Set(rows.filter(r => r.difficulty == null || r.difficulty < 50).map(r => r.keyword))

// After — match CLI scoring: prefer KD < 30; skip KD ≥ 50 regardless
() => new Set(
  rows
    .filter(r => r.difficulty == null || r.difficulty < 30)
    .map(r => r.keyword)
)
```

Also add an "Opportunity" badge column showing `High` / `Medium` / `Low` derived from `(kd, volume)` to match CLI display.

---

## Fix 6 — Native Rust keyword pipeline (remove Python dependency)

**Status:** Not started  
**Impact:** High (reliability) — removes hard dependency on `seo-content-cli` being installed  
**Effort:** High  

This replaces `exec_keyword_research_cli()` with a fully native Rust implementation. The Python CLI's pipeline is:

1. `seo-cli keyword-generator` for each theme → collect `all` keyword list
2. `filter-new-keywords` — dedupe against `articles.json` `target_keyword` values (fuzzy threshold 0.92)
3. `seo-cli batch-keyword-difficulty` on top-N candidates
4. Sort by opportunity score

The Rust app already has `seo/keywords.rs` with `get_keyword_ideas()` and `get_keyword_difficulty()` working against the same Ahrefs APIs. What needs to be built:

### 6a. Async-compatible execution from executor

The executor is synchronous (`fn exec_keyword_research_cli`), but `get_keyword_ideas` and `get_keyword_difficulty` are `async`. Use `tokio::task::block_in_place` or a `tokio::runtime::Handle::current().block_on(...)` call to bridge the sync context.

### 6b. Dedup against `articles.json`

Parse `{automation_dir}/articles.json` and collect all `target_keyword` values. Implement simple lowercase+whitespace-normalised exact match (or optionally Levenshtein for fuzzy at ≥0.92 similarity — can use the `strsim` crate).

### 6c. Batch difficulty

Call `get_keyword_difficulty()` sequentially for top-N candidates (same as CLI — rate-limited by CapSolver solves). Collect into `KeywordResearchResult` shape.

### 6d. Output shape

Write a `KeywordResearchResult` JSON struct directly as the step output so `KeywordPicker` can consume it identically.

### Migration path

- Add `exec_keyword_research_native()` alongside the existing `exec_keyword_research_cli()`.
- In `ResearchHandler::plan()`, emit a flag checked at runtime: if `seo-content-cli` is not found on PATH, fall back to native. This makes the transition zero-downtime.
- Once validated, remove the Python subprocess path.

---

## Deferred — Optimization candidates flow

Surfaces existing pages as `optimize_article` task candidates from the `optimize_candidates` JSON key.  
Requires: `KeywordResearchResult` type extension, a second picker section in `KeywordPicker`, and the `optimize_article` task creation path in `create_article_tasks_from_keywords`.

**Hold until:** Fix 1–5 are shipped and the core flow is stable.
