# Development Process

How to add a new domain end-to-end in the PageSeeds app.

---

## The Three-Layer Split

| Layer | Path | Rule |
|-------|------|------|
| **Commands** | `src-tauri/src/commands/{domain}.rs` | IPC boundary only. Validate, lock DB, delegate, return. |
| **Domain** | `src-tauri/src/{domain}/` | Business logic, external APIs, DB helpers, parsers. |
| **Exec** | `src-tauri/src/engine/exec/{domain}.rs` | Step executors called by the workflow engine. |
| **Handlers** | `src-tauri/src/engine/workflows/handlers.rs` | Orchestration: return `Vec<WorkflowStep>`, never execute. |

Reference implementation: `social/` (`src-tauri/src/social/`, `src-tauri/src/engine/exec/social.rs`, `commands/social.rs`).

---

## Decision Tree

```
I have new logic — where does it go?
│
├─ Is it reading request inputs and returning a Tauri response?
│  └─→ commands/{domain}.rs (thin wrapper)
│
├─ Is it building a step graph for a task type?
│  └─→ engine/workflows/handlers.rs
│
├─ Is it executing a single workflow step?
│  └─→ engine/exec/{domain}.rs
│
└─ Everything else (API clients, parsers, DB access, algorithms)
   └─→ {domain}/
```

---

## Worked Example: Adding a `summarize_content` Task

### 1. Add the domain module

Create `src-tauri/src/summarizer/mod.rs`:

```rust
pub fn summarize_article(path: &std::path::Path) -> crate::error::Result<String> {
    let text = std::fs::read_to_string(path)?;
    // deterministic extraction / truncation
    Ok(text.lines().take(3).collect::<Vec<_>>().join("\n"))
}
```

Declare it in `src-tauri/src/lib.rs`:

```rust
mod summarizer;
```

### 2. Add the step executor

Create `src-tauri/src/engine/exec/summarizer.rs`:

```rust
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

pub fn exec_summarize_content(task: &Task, project_path: &str) -> StepResult {
    let path = std::path::Path::new(project_path).join("content").join("article.md");
    match crate::summarizer::summarize_article(&path) {
        Ok(summary) => StepResult {
            success: true,
            message: "Summary generated".to_string(),
            output: Some(summary),
        },
        Err(e) => StepResult {
            success: false,
            message: e.to_string(),
            output: None,
        },
    }
}
```

Declare it in `src-tauri/src/engine/exec/mod.rs`:

```rust
pub mod summarizer;
```

### 3. Register the step kind

Add `SummarizeContent` to `engine/workflows/step_kind.rs` (all three places: enum, `as_str`, `from_str`).

Add the handler to `engine/step_registry.rs`:

```rust
handlers.insert(StepKind::SummarizeContent, Box::new(|_step, ctx| {
    let task = ctx.task;
    let project_path = ctx.project_path;
    Box::pin(async move {
        crate::engine::exec::summarizer::exec_summarize_content(task, project_path)
    })
}));
```

### 4. Add the workflow handler

In `engine/workflows/handlers.rs`, add `summarize_content` to the appropriate handler (or create a new one):

```rust
"summarize_content" => vec![
    WorkflowStep::new("summarize_content_run", StepKind::SummarizeContent),
],
```

### 5. Add the Tauri command (thin wrapper)

In `commands/summarizer.rs`:

```rust
#[tauri::command]
pub fn summarize_article(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id)?;
    crate::summarizer::summarize_article(std::path::Path::new(&project.path))
        .map_err(|e| e.to_string())
}
```

Register it in `lib.rs` and add the `invoke()` wrapper in `src/lib/tauri.ts`.

### 6. Build the UI

Create `src/components/summarizer/Summarizer.tsx` and call the new `summarizeArticle()` wrapper.

---

## Checklist Before Opening a PR

1. `cargo check` passes
2. `cargo test --lib` passes
3. `./scripts/sync-bindings.sh` was run if a `#[ts(export)]` model changed
4. `./scripts/check-bindings.sh` passes
5. No business logic in `commands/*.rs`
6. Every agentic step has a comment explaining why it cannot be deterministic
