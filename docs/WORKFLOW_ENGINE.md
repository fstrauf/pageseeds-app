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
4. ContentHandler         — write_article, optimize_article, review_article_quality
5. ContentReviewHandler   — content_review, content_audit
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
| `research_seed_validation` | `ResearchSeedValidation` | `exec_research_seed_validation` | Validated seeds with domain relevance |
| `keyword_research_native` | `KeywordResearchNative` | `exec_keyword_research_native` | `{"difficulty": {...}}` |
| `research_final_selection` | `ResearchFinalSelection` | `exec_research_final_selection` | `{"landing_page_candidates": [...]}` |

**Research Flow:**
1. Seed validation → LLM validates themes and proposes seed phrasings (agentic)
2. Keyword research → uses the SEO data provider to find keywords with volume/KD data (deterministic)
3. Final selection → selects best candidates (deterministic), then one batched LLM relevance check drops off-domain candidates before winnability enrichment (agentic, non-fatal)

**Content Write Flow** (`write_article`, `optimize_article`, `create_content`, `optimize_content`, `create_hub_page`, `refresh_hub_page`):
1. `content_write_stage` → agentic: writes the MDX file directly into the repo (skill: `content-write` or `hub-write`). New-article tasks get an exact target path directive; for text-only providers (Claude/OpenAI/Ollama) the executor persists the returned MDX to that path itself.
2. `content_write_verify` (`ContentWriteVerify`, new-article tasks only) → deterministic: fails the task when the write stage produced no registered article file (issue #13 safety net).
3. `content_link_verify` (`LinkIntegrityVerify`) → deterministic: every `/blog/` link in the written file must resolve to the project slug set (minus redirected slugs). Filename-form hrefs (`/blog/248_roast_profile_management`) are auto-repaired in place; any unresolvable link fails the step with a per-link report, and the file is left untouched (all-or-nothing). Fails when no written file exists.

**Consolidate Cluster Flow** (`consolidate_cluster`):
1. `merge_load_plan` → `merge_preflight` → `merge_extract_sections` → `merge_draft_patch` (agentic) → `merge_apply_patch`
2. `merge_generate_redirects` → appends source→keeper rules to `.github/automation/redirects.csv`
3. `merge_rewrite_inbound_links` (`MergeRewriteInboundLinks`) → deterministic: rewrites every `/blog/` link pointing at a redirected slug to the keeper URL across all MDX files
4. `merge_validate_output` → validates keeper + redirect map, and asserts zero remaining links to redirected slugs
5. `merge_sync_articles`

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
│   ├── link_verify.rs            # Post-write /blog/ link verification + repair
│   └── hub_page.rs               # Legacy hub creation (deprecated)
├── content_audit.rs              # 21-check deterministic audit
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

| `collect_gsc` | Fix tasks (`fix_technical`, `fix_indexing`, etc.) | Reads `gsc_collection.json` artifact |
| `write_article` | `cluster_and_link` | Optional, if linking module enabled |
| `content_review` | `fix_content_article` tasks (one per article) | Reads `recommendations.json` and creates individual tasks |

**Content Review Auto-Creation:**
When `content_review` completes successfully, the system automatically:
1. Reads `recommendations.json` (generated by the review step)
2. Creates one `fix_content_article` task per article in the recommendations
3. Each task contains only that article's specific recommendations
4. Tasks are `Batchable` priority (can run in batch) and sorted by issue count (more issues = higher priority)

**Idempotency:** Each fix task uses idempotency key `fix_content_article:{project_id}:{article_id}` to prevent duplicates if the review is re-run.

---

## Content Quality Gate (`review_article_quality`)

Every article produced by `write_article`, `create_hub_page`, or `refresh_hub_page` is now automatically followed by a `review_article_quality` task. This implements the "useful + visual + SEO basics + cluster fit" quality bar from the SEO baseline strategy.

### Flow

1. **`content_quality_context`** (deterministic)
   - Loads the MDX file written by the parent task.
   - Extracts frontmatter fields (title, description, H1, target keyword, slug, canonical, image).
   - Counts words and internal `/blog/...` links.
   - Returns a structured JSON context artifact.

2. **`content_quality_review`** (agentic)
   - Runs the `content-quality-review` skill via Rig `Extractor<ContentQualityReview>`.
   - Scores the article on four criteria (1–100):
     - `usefulness_score` — original insight, examples, data
     - `image_score` — at least one relevant, genuinely useful image
     - `seo_score` — title, meta description, H1, slug, canonical, internal links
     - `cluster_fit_score` — maps to a pillar/cluster and references related content
   - Sets `overall_pass` only if all four scores are ≥ 60 and no critical SEO field is missing.
   - Persists the result to `article_quality_reviews` and syncs `articles.quality_score / quality_reviewed_at / quality_pass`.

### UI / Downstream Effects

- The task is `AutoEnqueue` and `ArtifactReview`, so it runs automatically and surfaces the structured review for inspection.
- Quality failures can be read from `articles.quality_pass` and `article_quality_reviews`.
- The quality gate runs *before* `cluster_and_link`, so only reviewed (not necessarily passing) articles enter the linking stage.

---

## Topic Health Reducer

After `content_review` or `content_audit` completes, a deterministic reducer aggregates audit signals by `target_keyword` and updates `research_shortlist.health_status`.

### Health Status Thresholds

| Status | Rule | Meaning |
|--------|------|---------|
| `promising` | avg quality ≥ 70 AND (clicks > 0 OR impressions ≥ 1000) | Strong signal; produce more tangential content |
| `depleted` | avg quality < 50 AND impressions < 100 AND clicks = 0 | Weak signal; deprioritize new content |
| `unproven` | everything else | Insufficient evidence; keep in pool |

The reducer also writes a composite `signal_score`:

```
signal_score = avg_quality + (clicks * 10) + (impressions / 100)
```

### Consumption

- Keyword research reads pending shortlist entries with `list_pending_excluding_depleted`, so depleted themes are filtered out of new article production.
- The `research_shortlist.signal_score` and `health_status` columns feed future prioritization UIs.

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
               WorkflowStep::new("persist", StepKind::Deterministic)
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

- [AGENTS.md](../AGENTS.md) — Task lifecycle contract and core rules
- [Data Persistence](./DATA_PERSISTENCE.md) — Where workflow state lives
- [CONTRACTS.md](../CONTRACTS.md) — Runtime invariants and hidden rules
