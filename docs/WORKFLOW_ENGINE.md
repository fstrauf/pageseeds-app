# Workflow Engine

The workflow engine orchestrates task execution. It is **not** a general-purpose workflow system — it is purpose-built for SEO content operations.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           WORKFLOW ENGINE                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│   Task Created                                                          │
│        │                                                                │
│        ▼                                                                │
│   ┌─────────┐    Route by task_type    ┌─────────────┐                  │
│   │ Handler │ ────────────────────────▶│ Step Plan   │                  │
│   │Registry │                          │ (ordered    │                  │
│   └─────────┘                          │  vec)        │                  │
│        │                               └─────────────┘                  │
│        │                                      │                         │
│        │◀────────────────────────────────────┘                         │
│        │      Execute steps sequentially                               │
│        │                                                                │
│        ▼                                                                │
│   ┌─────────────┐   Emit events   ┌─────────────┐                      │
│   │   Executor  │────────────────▶│  Frontend   │                      │
│   │  (run_step) │                  │  (events)   │                      │
│   └─────────────┘                  └─────────────┘                      │
│        │                                                                │
│        ▼                                                                │
│   Status: done / review / todo                                         │
│        │                                                                │
│        ▼                                                                │
│   Auto-spawn follow-up tasks?                                          │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Core Components

### 1. Handlers (`engine/workflows/handlers.rs`)

Handlers are **planners**, not executors. They return a sequence of steps; the executor runs them.

```rust
pub trait WorkflowHandler: Send + Sync {
    fn can_handle(&self, task: &Task) -> bool;
    fn plan(&self, task: &Task, ctx: &HandlerContext) -> Vec<WorkflowStep>;
}
```

**Handler Registry Order (First-Match-Wins):**
```
1. CollectionHandler      — collect_gsc
2. InvestigationHandler   — investigate_gsc
3. ResearchHandler        — research_keywords, custom_keyword_research
4. ContentHandler         — write_article, optimize_article
5. ContentReviewHandler   — content_review, content_audit, content_review_apply
6. RedditHandler          — reddit_search, reddit_reply
7. PerformanceHandler     — gsc_performance
8. ImplementationHandler  — fix_* (catch-all prefix match)
9. ManualFallbackHandler  — (must be LAST, matches everything)
```

**Rule:** New handlers go BEFORE ImplementationHandler and BEFORE ManualFallbackHandler.

---

### 2. Workflow Steps (`engine/workflows/mod.rs`)

Steps are declarative. The executor dispatches based on `kind`.

```rust
pub struct WorkflowStep {
    pub name: String,
    pub kind: StepKind,      // Deterministic | Agentic | Manual | RedditSearch | ...
    pub params: HashMap<String, String>,
}
```

### Step Kind Contract

| Kind | What It Does | Produces | Special Rules |
|------|--------------|----------|---------------|
| `Agentic` | Calls LLM agent | Sets `latest_raw_output` | — |
| `Deterministic` | Runs Rust code | Optional output | No side effects (ideally) |
| `Manual` | Marks user action required | Nothing | Blocks execution |
| `RedditSearch` | Reddit API + scoring | DB records | Triggers inline enrichment |
| `RedditEnrich` | AI scoring + reply drafting | Updates DB rows | Requires DB connection |
| `ContentReviewRecommend` | Article selection + agent | recommendations.json | Hybrid: det + agentic |

**Research Workflow Steps (Hybrid Flow):**

| Step Name | Kind | Handler | Output |
|-----------|------|---------|--------|
| `research_autocomplete` | `ResearchAutocomplete` | `exec_research_autocomplete` | Autocomplete suggestions per theme |
| `research_seed_validation` | `ResearchSeedValidation` | `exec_research_seed_validation` | Validated seeds with domain relevance |
| `keyword_research_native` | `KeywordResearchNative` | `exec_keyword_research_native` | `{"difficulty": {...}}` |
| `research_final_selection` | `ResearchFinalSelection` | `exec_research_final_selection` | `{"landing_page_candidates": [...]}` |

**Research Flow:**
1. Autocomplete → gathers search suggestions per theme (deterministic)
2. Seed validation → LLM filters suggestions for domain relevance (agentic)
3. Keyword research → uses Ahrefs API tools to find keywords with volume/KD data (deterministic)
4. Final selection → selects best candidates (deterministic)

**Data Flow:** Step output flows to the next step via `latest_raw_output` when the step kind is in the `latest_raw` carry list.

---

### 3. Executor (`engine/executor.rs`)

The executor is an **orchestrator only**. Business logic lives in `engine/exec/` modules.

**Key Functions:**
```rust
// Main entry point
pub async fn execute_task(conn: &Connection, task_id: &str) -> Result<ExecutionResult>

// Step dispatcher
async fn run_step(step: &WorkflowStep, ctx: &mut StepContext) -> StepResult
```

**Status Transitions:**
```
all_ok = true:
  research_keywords | custom_keyword_research ──▶ "review"
  all other task_types ─────────────────────────▶ "done"

all_ok = false:
  ──────────────────────────────────────────────▶ "todo" (reset for retry)
```

---

### 4. Execution Modules (`engine/exec/`)

Business logic is split by domain:

```
engine/exec/
├── mod.rs                        # Re-exports
├── keywords.rs                   # Keyword research
├── content/
│   ├── mod.rs                    # Content review/apply
│   ├── cluster_link.rs           # Internal link graph
│   └── hub_page.rs               # Legacy hub creation (deprecated)
├── content_audit.rs              # 13-rule audit
├── reddit.rs                     # Search + enrichment
├── gsc.rs                        # GSC collection + sync
├── social/                       # Social media campaign steps
├── ctr_audit.rs                  # CTR audit + fix pipeline
├── cannibalization_audit.rs      # Cannibalization detection
├── consolidate_cluster.rs        # Merge + redirect workflow
├── territory_research.rs         # Territory strategy
└── utils.rs                      # Shared helpers
```

**Rule:** The executor calls these; they don't call each other.

---

## Deterministic vs Agentic

Every step must be explicitly one or the other.

| Mode | Use When | Never For |
|------|----------|-----------|
| **Deterministic** | Machine-checkable, repeatable logic (API calls, filtering, sorting) | Interpreting ambiguous text or intent |
| **Agentic** | Judgment required (theme curation, prioritization, prose generation) | Stable API calls that have deterministic paths |

**The Hybrid Pattern (canonical):**
```
1. Deterministic step: collect data, compute metrics, filter, rank, group
                    ↓
2. Agentic step: interpret, recommend, write prose using structured output from step 1
```

**Example — content_review:**
- Step 1-3: Deterministic (sync → GSC fetch → audit)
- Step 4: Agentic (recommendations based on structured audit data)

**External API calls are deterministic** — the API does the computation. The step that *interprets* API results may be agentic.

---

## Step Parameters

Keys consumed by executor dispatch:

| Param | Used By | Purpose |
|-------|---------|---------|
| `"skill"` | `Agentic` | Names the SKILL.md file to load as prompt |
| `"artifact_name"` | any | Names the output artifact persisted to SQLite |

---

## Auto-Spawned Follow-Up Tasks

Certain task types automatically create follow-up tasks on success. **Do not create these manually.**

| Parent Task | Auto-Spawns | Spawning Logic |
|-------------|-------------|----------------|
| `content_review` | `content_review_apply` | `engine/spawner.rs` via `spawn_follow_up()` |
| `content_audit` | `content_review_apply` | Same |
| `collect_gsc` | Fix tasks (`fix_technical`, `fix_indexing`, etc.) | Reads `gsc_collection.json` artifact |
| `write_article` | `cluster_and_link` | Optional, if linking module enabled |
| `content_review` | `fix_content_article` tasks (one per article) | Reads `recommendations.json` and creates individual tasks |

**Content Review Auto-Creation:**
When `content_review` completes successfully, the system automatically:
1. Reads `recommendations.json` (generated by the review step)
2. Creates one `fix_content_article` task per article in the recommendations
3. Each task contains only that article's specific recommendations
4. Tasks are `Batchable` priority (can run in batch) and sorted by issue count (more issues = higher priority)

This replaces the previous monolithic `content_review_apply` approach. Instead of one task that fixes all articles, you get individual tasks that can run independently.

**Idempotency:** Each fix task uses idempotency key `content_review_apply:{project_id}:{article_id}` to prevent duplicates if the review is re-run.

---

## Async Execution Pattern

SQLite connections are `!Send`, so tasks run in dedicated threads:

```rust
#[tauri::command]
pub async fn execute_task(...) -> Result<...> {
    let db_path = state.db_path.clone();
    
    tokio::task::spawn_blocking(move || {
        // 1. Open dedicated SQLite connection
        let db = rusqlite::Connection::open(&db_path)?;
        db.busy_timeout(Duration::from_secs(10))?;
        
        // 2. Create local Tokio runtime for async execution
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            executor::execute_task(&db, &task_id).await
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
```

**Why:**
- SQLite connections cannot be sent between threads
- Tauri runtime is multi-threaded async
- Each task gets its own OS thread + connection + local runtime

**Key Rules:**
1. Never use `Handle::current().block_on()` in async context (causes panic)
2. Always use `.await` for async operations
3. One connection per task

---

## Execution Modes

| Mode | Behavior | UI Treatment |
|------|----------|--------------|
| `"automatic"` | Runs in batch without user intervention | No confirm needed |
| `"batchable"` | Can run in batch; user can also trigger manually | Shows "Add to Queue" |
| `"manual"` | User must trigger explicitly | Shows "Run" button only |
| `"spec"` | Requires a spec artifact before execution | Shows "Upload Spec" |

Batch runner checks: `"automatic"` OR `"batchable"`.

---

## Events

The executor emits Tauri events for live UI updates:

```rust
app_handle.emit("queue:task-started", QueueProgressEvent { ... });
app_handle.emit("queue:task-step-progress", StepProgressEvent { ... });
app_handle.emit("queue:task-completed", QueueProgressEvent { ... });
app_handle.emit("queue:task-failed", QueueProgressEvent { ... });
```

The frontend `useQueueRunner` hook consumes these to drive the TaskRunner UI.

---

## Adding a New Workflow

> **Before adding:** Check the "DRY: Core Reusable Functions" catalog in [`AGENTS.md`](../AGENTS.md). If the workflow produces an MDX article, adds internal links, audits content, or exports `articles.json`, reuse the existing handler/step — do not create a new one.

1. **Create handler** in `engine/workflows/handlers.rs`:
   ```rust
   pub struct MyHandler;
   impl WorkflowHandler for MyHandler {
       fn can_handle(&self, task: &Task) -> bool {
           task.task_type == "my_new_type"
       }
       fn plan(&self, task: &Task, ctx: &HandlerContext) -> Vec<WorkflowStep> {
           vec![
               WorkflowStep::new("collect_data", StepKind::Deterministic),
               WorkflowStep::new("analyze", StepKind::Agentic)
                   .with_param("skill", "analyze_data"),
               WorkflowStep::new("normalize", StepKind::Normalizer)
                   .with_param("normalizer_id", "my_normalizer")
                   .with_param("artifact_name", "my_result"),
           ]
       }
   }
   ```

2. **Register handler** in `default_handlers()`:
   ```rust
   vec![
       // ... existing handlers ...
       Box::new(MyHandler),        // Add here
       Box::new(ImplementationHandler),
       Box::new(ManualFallbackHandler),
   ]
   ```

3. **Add execution logic** in `engine/exec/my_domain.rs`:
   ```rust
   pub async fn exec_collect_data(ctx: &StepContext) -> StepResult { ... }
   ```

4. **Wire in executor** by adding match arm in `run_step()`.

5. **Add types** to `models/` and `src/lib/types.ts`.

---

## See Also

- [Task Queue](./TASK_QUEUE.md) — How tasks are scheduled and executed
- [Data Persistence](./DATA_PERSISTENCE.md) — Where workflow state lives
- [CONTRACTS.md](../CONTRACTS.md) — Runtime invariants and hidden rules
