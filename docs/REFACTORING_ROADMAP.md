# PageSeeds Codebase Refactoring Roadmap

> **Goal:** Restructure the codebase for maintainability without changing a single line of business logic.
>
> **Constraint:** Every change must be verifiable via compilation, existing tests, golden tests, or shadow tests.
>
> **Estimated effort:** 4–6 weeks of incremental PRs, one concern at a time.

---

## Table of Contents

1. [Pre-Work: Baseline & Safeguards](#phase-0-pre-work-baseline--safeguards)
2. [Phase 1: Split Engine/Exec Megafunctions](#phase-1-split-engineexec-megafunctions)
3. [Phase 2: Thin the Command Layer](#phase-2-thin-the-command-layer)
4. [Phase 3: Collapse Boilerplate](#phase-3-collapse-boilerplate)
5. [Phase 4: Frontend Data Layer & Component Splits](#phase-4-frontend-data-layer--component-splits)
6. [Phase 5: Cleanup & Pruning](#phase-5-cleanup--pruning)
7. [Appendix A: Verification Scripts](#appendix-a-verification-scripts)
8. [Appendix B: Golden Test Templates](#appendix-b-golden-test-templates)
9. [Appendix C: Troubleshooting](#appendix-c-troubleshooting)

---

## Phase 0: Pre-Work — Baseline & Safeguards

**Do not skip this.** Every later phase depends on having a verified baseline.

### 0.1 Lock the Baseline

```bash
# Ensure everything is green before starting
cd /Users/fstrauf/01_code/pageseeds-app

cargo test --lib
cargo test --bin
cargo check

pnpm test
pnpm exec tsc -b
pnpm run lint
pnpm run build

git add -A
git commit -m "refactor: lock pre-cleanup baseline"
```

### 0.2 Create the Golden Test Directory

```bash
mkdir -p src-tauri/tests/golden
mkdir -p src-tauri/tests/fixtures
```

### 0.3 Record Golden Snapshots for High-Risk Functions

For each function listed below, add a **recording test** (run once), then a **comparison test** (run forever after).

**Functions to record:**
- `engine::exec::keywords::exec_keyword_research_native`
- `engine::exec::content::exec_content_review_recommend`
- `engine::exec::reddit::exec_reddit_search`
- `engine::exec::reddit::exec_reddit_enrich`
- `engine::exec::ctr_audit::exec_ctr_build_context`
- `commands::reddit::draft_reddit_reply` (via integration test)
- `commands::executor::execute_queue_internal` (via integration test)

**Template for recording test** (add to the file containing the function, inside `#[cfg(test)]`):

```rust
#[test]
#[ignore = "Run manually: cargo test record_goldens -- --ignored"]
fn record_goldens() {
    let task = load_fixture("standard_research_task.json");
    let result = exec_keyword_research_native(&task, "/fixtures/project", "kimi");
    
    let snapshot = serde_json::json!({
        "success": result.success,
        "message": result.message,
        "output": result.output,
    });
    
    std::fs::write(
        "tests/golden/keyword_research_native.json",
        serde_json::to_string_pretty(&snapshot).unwrap()
    ).unwrap();
}
```

**Template for comparison test** (add to `src-tauri/tests/golden_tests.rs`):

```rust
#[test]
fn keyword_research_native_matches_golden() {
    let task = load_fixture("standard_research_task.json");
    let result = engine::exec::keywords::exec_keyword_research_native(
        &task, "/fixtures/project", "kimi"
    );
    
    let golden: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("tests/golden/keyword_research_native.json").unwrap()
    ).unwrap();
    
    let actual = serde_json::json!({
        "success": result.success,
        "message": result.message,
        "output": result.output,
    });
    
    assert_eq!(actual, golden, "Behavior changed during refactor!");
}
```

### 0.4 Create the Verify Script

Save [Appendix A: verify_move.py](#appendix-a-verification-scripts) to `scripts/verify_move.py`.

```bash
chmod +x scripts/verify_move.py
```

### 0.5 Create Fixture Files

Create minimal fixture tasks that exercise the happy path + one edge case for each major function:

```bash
# Example fixture structure
src-tauri/tests/fixtures/
  research_task_standard.json
  research_task_empty_themes.json
  reddit_task_standard.json
  content_review_task_standard.json
```

**Rule:** Fixtures must be checked into git. They are your contract.

---

## Phase 1: Split Engine/Exec Megafunctions

**Goal:** Turn `engine/exec/*.rs` files into directories with focused submodules.
**Risk:** Near zero (pure moves with re-exports).
**Order:** Start with the smallest, work up to the largest.

### 1.1 `engine/exec/social.rs` (856 lines) — Warm-up

**Why first:** Smallest exec module. Good practice run.

**Steps:**

1. Create directory:
   ```bash
   mkdir -p src-tauri/src/engine/exec/social
   ```

2. Rename file:
   ```bash
   mv src-tauri/src/engine/exec/social.rs src-tauri/src/engine/exec/social/mod.rs
   ```

3. In `social/mod.rs`, identify natural split points:
   - `exec_social_extract_article`, `exec_social_collect_sources` → `extract.rs`
   - `exec_social_generate_posts`, `parse_*` helpers → `generate.rs`
   - `exec_social_build_visuals`, `exec_social_rebuild_visual` → `visuals.rs`
   - `exec_social_save_template`, `exec_social_design_template` → `templates.rs`
   - Remaining orchestration stays in `mod.rs`

4. For each extracted file, copy the function bodies **exactly**.

5. In `social/mod.rs`, add:
   ```rust
   mod extract;
   mod generate;
   mod visuals;
   mod templates;
   
   pub use extract::*;
   pub use generate::*;
   pub use visuals::*;
   pub use templates::*;
   ```

6. Verify:
   ```bash
   cargo check
   python scripts/verify_move.py \
     src-tauri/src/engine/exec/social.rs.bak \
     src-tauri/src/engine/exec/social/generate.rs \
     exec_social_generate_posts
   cargo test
   ```

7. Commit:
   ```bash
   git add -A
   git commit -m "refactor: split engine/exec/social.rs into submodules"
   ```

### 1.2 `engine/exec/gsc.rs` (957 lines)

**Split plan:**

| New File | Functions |
|----------|-----------|
| `gsc/sync.rs` | `exec_gsc_sync_articles` |
| `gsc/collect.rs` | `exec_collect_gsc`, `resolve_site_config` |
| `gsc/investigate.rs` | `exec_gsc_summarise`, `exec_gsc_investigate` |
| `gsc/task_spawner.rs` | `create_tasks_from_collection`, `create_tasks_from_collection_after_exec` |
| `gsc/mod.rs` | `normalize_url_for_comparison`, re-exports |

**Steps:** Same as 1.1.

**Golden test to run:** `gsc_collect_matches_golden` (if you have a fixture for it).

### 1.3 `engine/exec/ctr_audit.rs` (905 lines)

**Split plan:**

| New File | Functions |
|----------|-----------|
| `ctr_audit/context.rs` | `exec_ctr_build_context` |
| `ctr_audit/analyze.rs` | `exec_ctr_analyze`, `load_skill_with_fallback` |
| `ctr_audit/task_spawner.rs` | `create_ctr_fix_tasks`, `filter_recommendations_by_fix_type` |

**Shadow test:** Keep `exec_ctr_build_context_legacy` in `mod.rs` for one commit:

```rust
#[cfg(test)]
fn exec_ctr_build_context_legacy(task: &Task, ...) -> StepResult {
    // exact original code
}

#[test]
fn ctr_build_context_shadow() {
    let task = load_fixture("ctr_task.json");
    let new = exec_ctr_build_context(&task, ...);
    let old = exec_ctr_build_context_legacy(&task, ...);
    assert_eq!(new.success, old.success);
    assert_eq!(new.output, old.output);
}
```

Run `cargo test`, then delete `_legacy` in the next commit.

### 1.4 `engine/exec/research.rs` (875 lines)

**Split plan:**

| New File | Functions |
|----------|-----------|
| `research/prompts.rs` | `build_research_prompts`, `build_coverage_summary` |
| `research/autocomplete.rs` | `exec_research_autocomplete`, `exec_research_final_selection`, `select_keywords_deterministic` |
| `research/landing_page.rs` | `exec_landing_page_spec_write`, `parse_landing_page_meta`, `build_spec_markdown` |

### 1.5 `engine/exec/reddit.rs` (1,428 lines)

**Split plan:**

| New File | Functions |
|----------|-----------|
| `reddit/config.rs` | `RedditSearchParams`, all `extract_*` functions, `exec_reddit_config_parse` |
| `reddit/search.rs` | `exec_reddit_search`, `compute_scores` |
| `reddit/enrich.rs` | `exec_reddit_enrich`, `persist_reddit_opportunities` |
| `reddit/reply.rs` | `exec_reddit_fetch_results`, `create_reddit_reply_tasks_from_opportunities`, `exec_reddit_post_reply` |

**Verification:**
```bash
# Verify each moved function
for func in exec_reddit_search exec_reddit_enrich exec_reddit_post_reply; do
  python scripts/verify_move.py \
    src-tauri/src/engine/exec/reddit.rs.bak \
    src-tauri/src/engine/exec/reddit/search.rs \
    $func
done
```

### 1.6 `engine/exec/content.rs` (1,928 lines)

**Split plan:**

| New File | Functions |
|----------|-----------|
| `content/review.rs` | `exec_content_review_apply`, `exec_content_review_recommend`, `build_review_context`, `build_review_prompt`, `select_priority_articles` |
| `content/sync.rs` | `exec_content_sync`, `exec_format_validation`, `exec_format_fix` |
| `content/cluster_link.rs` | `exec_cluster_link_scan`, `exec_cluster_link_strategy`, `exec_cluster_link_apply`, `create_cluster_and_link_task` |
| `content/task_spawner.rs` | `create_content_review_apply_task`, `sync_article_review_state_to_repo`, `mark_articles_in_review`, `mark_fix_content_article_reviewed` |

**Important:** `build_review_prompt` contains a large inline prompt. Verify with `verify_move.py` that the prompt string is character-identical.

### 1.7 `engine/exec/keywords.rs` (2,474 lines) — The Big One

**Split plan:**

| New File | Functions |
|----------|-----------|
| `keywords/theme_extraction.rs` | `derive_themes_from_project`, `extract_from_brief`, `extract_from_summary`, `extract_from_articles`, `find_file_by_suffix`, `clean_theme_str`, `parse_desc_themes` |
| `keywords/research_pipeline.rs` | `exec_keyword_research_native`, `Candidate`, `smart_sample_candidates`, `best_serp_metric` |
| `keywords/coverage_filter.rs` | `load_coverage_clusters`, `score_coverage_gap`, `filter_by_coverage_gap`, `fuzzy_word_match` |
| `keywords/auto_spawn.rs` | `auto_create_article_tasks_from_research`, `extract_keywords_with_metrics`, `calculate_opportunity_score` |
| `keywords/tests.rs` | Move the ~700-line `#[cfg(test)]` module here |

**Critical verification:**
```bash
# The test module is huge — verify it moved completely
python scripts/verify_move.py \
  src-tauri/src/engine/exec/keywords.rs.bak \
  src-tauri/src/engine/exec/keywords/tests.rs \
  test_research_pipeline_basic
```

**Golden test:** This is the most important one. Ensure `keyword_research_native_matches_golden` passes before and after.

### Phase 1 Completion Checklist

- [ ] All 7 exec modules split into directories with `mod.rs` re-exports
- [ ] `cargo check` passes with zero changes to any file outside `engine/exec/`
- [ ] All 173 Rust tests pass
- [ ] All golden tests pass
- [ ] No business logic changes (verified by `verify_move.py` or shadow tests)
- [ ] Each split committed separately

---

## Phase 2: Thin the Command Layer

**Goal:** Move business logic out of `commands/` into domain modules. Commands become one-line wrappers.
**Risk:** Low. We're adding indirection, not changing logic.

### 2.1 Extract `commands/reddit.rs` Workflows

**Extract to `reddit/` domain:**

| Function | Move To |
|----------|---------|
| `draft_reddit_reply` logic | `reddit/draft.rs` → `pub async fn draft_reply(...)` |
| `validate_reddit_reply` logic | `reddit/validation.rs` → `pub fn validate_reply(...)` |
| `create_reddit_reply_tasks` logic | `engine::spawner` or `reddit/spawner.rs` |

**Wrapper pattern in `commands/reddit.rs`:**

```rust
#[tauri::command]
pub async fn draft_reddit_reply(...) -> Result<String, String> {
    reddit::draft::draft_reply(...).await
}
```

**Integration test:**
```rust
#[test]
fn draft_reddit_reply_command_matches_golden() {
    // Invoke through the Tauri command layer
    let result = invoke_command("draft_reddit_reply", &params);
    assert_eq!(result, load_golden("reddit_draft.json"));
}
```

### 2.2 Extract `commands/content.rs` Orchestration

| Function | Move To |
|----------|---------|
| `fix_content_dates` inline SQL | `content::dates::fix_and_export` |
| `analyze_article_readability` file I/O chain | `content::ops::analyze_article` |
| `analyze_keyword_density` file I/O chain | `content::ops::analyze_article` (shared helper) |
| `resolve_year_mismatch_agent` | `content::dates::resolve_year_mismatch` |

### 2.3 Extract Queue Runtime from `commands/executor.rs`

1. Create `engine/queue_runner.rs`
2. Move `execute_queue_internal` and its types **exactly** as-is
3. In `commands/executor.rs`:
   ```rust
   pub async fn execute_queue_internal(...) {
       engine::queue_runner::execute_queue_internal(...).await
   }
   ```
4. Move `QueueItem`, `QueueProgressEvent` to `models/task.rs` or `engine/queue_runner.rs`
5. Verify with integration test that calls the `execute_queue` command

### 2.4 Extract Shared Auth Resolution

**Duplicate pattern:** GSC token resolution in `commands/engine.rs` (`execute_task` and `run_batch`).

1. Add to `gsc/auth.rs`:
   ```rust
   pub async fn resolve_or_fetch_token(
       state: &GscState,
       project_path: &str,
   ) -> Result<String, Error> {
       // exact logic from execute_task, deduplicated
   }
   ```
2. Replace both occurrences in `commands/engine.rs` with one-liners

**Duplicate pattern:** Legacy agent provider fallback.

1. Add to `db/global_settings.rs` or `config/mod.rs`:
   ```rust
   pub fn resolve_agent_provider(
       conn: &Connection,
       project: &Project,
   ) -> Result<String, Error> {
       if let Some(legacy) = &project.agent_provider {
           return Ok(legacy.clone());
       }
       global_settings::get_agent_provider(conn)
   }
   ```
2. Replace occurrences in `reddit.rs`, `content.rs`, `settings.rs`

### Phase 2 Completion Checklist

- [ ] `commands/reddit.rs` functions are wrappers (≤5 lines each)
- [ ] `commands/content.rs` no longer has inline SQL
- [ ] `commands/executor.rs` no longer contains the queue runtime
- [ ] `commands/engine.rs` GSC auth deduplicated
- [ ] `cargo test` passes
- [ ] Integration tests through command layer pass
- [ ] Each extraction committed separately

---

## Phase 3: Collapse Boilerplate

**Goal:** Reduce the 25 identical patterns in `step_registry.rs` and the duplicated JSON I/O across exec modules.
**Risk:** Low-medium. Use one-at-a-time conversion with tests.

### 3.1 Extract Shared JSON I/O Helpers

Create `engine/exec/common.rs`:

```rust
use std::path::Path;
use serde::{de::DeserializeOwned, Serialize};
use crate::engine::workflows::StepResult;

pub fn read_json<T: DeserializeOwned>(path: &Path, context: &str) -> Result<T, StepResult> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return Err(StepResult {
            success: false,
            message: format!("{}: failed to read {}: {}", context, path.display(), e),
            output: None,
        }),
    };
    match serde_json::from_str(&content) {
        Ok(v) => Ok(v),
        Err(e) => Err(StepResult {
            success: false,
            message: format!("{}: invalid JSON in {}: {}", context, path.display(), e),
            output: None,
        }),
    }
}

pub fn write_json<T: Serialize>(path: &Path, value: &T, context: &str) -> Result<(), StepResult> {
    let json = match serde_json::to_string_pretty(value) {
        Ok(j) => j,
        Err(e) => return Err(StepResult {
            success: false,
            message: format!("{}: failed to serialize: {}", context, e),
            output: None,
        }),
    };
    match std::fs::write(path, json) {
        Ok(()) => Ok(()),
        Err(e) => Err(StepResult {
            success: false,
            message: format!("{}: failed to write {}: {}", context, path.display(), e),
            output: None,
        }),
    }
}
```

**Conversion rule:** Replace one usage at a time. Run `cargo test` after each.

### 3.2 Macro-ify Step Registry

In `engine/step_registry.rs`, add:

```rust
macro_rules! register_blocking {
    ($registry:ident, $kind:expr, $module:path, $fn:path) => {
        $registry.insert($kind, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let agent_provider = ctx.agent_provider.to_string();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    $module::$fn(&task, &project_path, &agent_provider)
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: e.to_string(),
                    output: None,
                })
            })
        }))
    };
}
```

**Conversion:** Replace one registration at a time.

**Before:**
```rust
handlers.insert(StepKind::KeywordResearch, Box::new(|_step, ctx| {
    // 10 lines of boilerplate
}));
```

**After:**
```rust
register_blocking!(handlers, StepKind::KeywordResearch, 
    crate::engine::exec::keywords, exec_keyword_research_native);
```

**Verify:** `cargo test` after each single replacement. If tests fail, you know exactly which macro application is wrong.

### Phase 3 Completion Checklist

- [ ] `engine/exec/common.rs` exists with `read_json` / `write_json`
- [ ] At least 10 raw JSON read/write patterns replaced with helpers
- [ ] `step_registry.rs` uses `register_blocking!` for all sync handlers
- [ ] `cargo test` passes after every single replacement
- [ ] No golden tests broken

---

## Phase 4: Frontend Data Layer & Component Splits

**Goal:** Decouple 38 components from direct `tauri.ts` calls. Extract domain hooks and split god components.
**Risk:** Low. TypeScript compiler catches prop mismatches.

### 4.1 Create Domain Hooks

Create `src/hooks/useTasks.ts`:

```typescript
import { useCallback } from 'react'
import { listTasks, getTask, deleteTask, updateTaskStatus, createTask } from '@/lib/tauri'
import type { Task } from '@/lib/types'
import { useQuery } from './useQuery'

export function useTasks(projectId: string, status?: string, phase?: string) {
  const { data, refetch } = useQuery(
    ['tasks', projectId, status, phase],
    () => listTasks(projectId, status, phase),
  )

  const remove = useCallback(async (id: string) => {
    await deleteTask(id)
    refetch()
  }, [refetch])

  return {
    tasks: data ?? [],
    refetch,
    remove,
  }
}
```

**Verification:** Replace `listTasks` in `TaskBoard.tsx` with `useTasks`. Run:
```bash
pnpm exec tsc -b
pnpm test
```

Repeat for:
- `useProjectOverview(projectId)`
- `useArticles(projectId)`
- `useCtrHealth(projectId)`
- `useProjectSettings(projectId)`
- `useQueueManager()` (extract orchestration from `queueStore`)

### 4.2 Purify `queueStore`

**Current problem:** `queueStore.ts` (518 lines) contains business logic.

**Split:**
- `queueStore.ts` — pure state only (`items`, `isRunning`, `isPaused`, `isVisible`)
- `hooks/useQueueManager.ts` — event listeners, auto-start, DB mutations

**Rule:** Store actions become simple setters. The hook subscribes to events and calls store setters.

### 4.3 Split `Overview.tsx` (1,071 lines)

Extract to new components (copy-paste JSX, pass same props):

| New Component | Lines Extracted |
|---------------|-----------------|
| `components/overview/QuickActionsPanel.tsx` | ~200 |
| `components/overview/CtrHealthCard.tsx` | ~180 |
| `components/overview/LandingPageDialog.tsx` | ~100 |
| `components/overview/ProjectStats.tsx` | ~50 |

**Pattern:** In `Overview.tsx`, replace inline JSX with:
```tsx
<QuickActionsPanel project={project} onCreateTask={handleCreateTask} />
```

**Verification:**
```bash
pnpm exec tsc -b
pnpm run lint
pnpm test
```

### 4.4 Split `TaskBoard.tsx` (644 lines)

| New Component | Extracted Logic |
|---------------|-----------------|
| `components/tasks/TaskSelectionToolbar.tsx` | `checkedIds`, bulk actions |
| `components/tasks/TaskFilters.tsx` | `statusFilter`, `phaseFilter` |
| `components/tasks/TaskTable.tsx` | Table rendering |

### 4.5 Split `Settings.tsx` (590 lines)

| New Component | Extracted Logic |
|---------------|-----------------|
| `components/settings/SecretsCard.tsx` | Secrets management |
| `components/settings/AgentConfigCard.tsx` | Agent provider selection |
| `components/settings/SeoProviderCard.tsx` | SEO provider selection |
| `components/settings/ProjectFilesCard.tsx` | Config file status |

### Phase 4 Completion Checklist

- [ ] `useTasks`, `useArticles`, `useProjectOverview` hooks exist
- [ ] `queueStore` contains no business logic (only state)
- [ ] `Overview.tsx` < 400 lines
- [ ] `TaskBoard.tsx` < 400 lines
- [ ] `Settings.tsx` < 300 lines
- [ ] `pnpm exec tsc -b` passes
- [ ] `pnpm test` passes
- [ ] `pnpm run build` passes

---

## Phase 5: Cleanup & Pruning

### 5.1 Prune Stale Documentation

**Archive or delete:**
- `docs/content-review-revisit-spec.md` (59 lines — likely complete)
- `docs/ctr-pipeline-implementation-plan.md` (334 lines — likely complete)
- `docs/config-consolidation-plan.md` (273 lines — likely complete)
- `docs/agent-dx-improvement-plan.md` (312 lines — likely complete)
- `docs/keyword-research-improvement-plan.md`
- `docs/keyword-research-v3-spec.md`
- `docs/reddit-agentic-config-plan.md`
- `docs/seo-analysis-upgrade-spec.md`
- `docs/seo-improvement-workflow-spec.md`
- `docs/seo-machine-integration-spec.md`
- `docs/seo-workflow-implementation-todo.md`
- `docs/task-queue-v2-spec.md`
- `docs/universal-agent-wrapper-spec.md`
- `SEO_MACHINE_PORTING_PLAN.md` (841 lines)

**Keep:**
- `AGENTS.md`
- `README.md`
- `CONTRACTS.md`
- `STYLE_GUIDE.md`
- `docs/README.md`
- `docs/dev-process.md`
- `docs/WORKFLOW_ENGINE.md`
- `docs/DATA_PERSISTENCE.md`
- `docs/BUSINESS_PROCESSES.md`
- `docs/TASK_QUEUE.md`

**Action:**
```bash
mkdir -p docs/archive
mv docs/*-spec.md docs/archive/ 2>/dev/null || true
mv docs/*-plan.md docs/archive/ 2>/dev/null || true
mv SEO_MACHINE_PORTING_PLAN.md docs/archive/
```

### 5.2 Prune Scripts

**Archive:**
- `scripts/test_kimi_exact_call.sh`
- `scripts/test_kimi_full_output.sh`
- `scripts/test_kimi_isolated.sh`
- `scripts/test_kimi_json.sh`
- `scripts/test_kimi_real_config.sh`
- `scripts/check_file_logs.sh`
- `scripts/check_queue_logs.sh`

**Keep:**
- `scripts/sync-bindings.sh`
- `scripts/check-bindings.sh`
- `scripts/generate-types.sh`
- `scripts/pre-release-checks.sh`
- `scripts/test-queue-system.sh`
- `scripts/test-reddit-flow.sh`
- `scripts/verify_move.py` (new)

### 5.3 Split DB Migrations

`db/mod.rs` is 844 lines with 28 migrations. When it hits 1,000 lines, split:

```
src-tauri/src/db/
  mod.rs
  migrations/
    V01__initial.sql
    V07__idempotency.sql
    V08__image_generation_prompt.sql
    ...
```

**Safe approach:** Keep `mod.rs` as a loader, move SQL strings to files. No schema changes.

### 5.4 Dead Code Removal

After all phases complete, run:

```bash
cargo clippy -- -W dead_code
```

Address warnings for:
- `engine/exec/social_simple.rs` (if unused)
- `engine/exec/reddit_test.rs` (if superseded)
- Any unused imports from the refactor

### Phase 5 Completion Checklist

- [ ] Stale specs archived
- [ ] Debug scripts archived
- [ ] `cargo clippy` clean (no dead code warnings)
- [ ] All 5 phases committed and pushed

---

## Appendix A: Verification Scripts

### `scripts/verify_move.py`

```python
#!/usr/bin/env python3
"""Verify that a function extracted by an LLM matches the original."""
import sys
import re

def extract_function(path, func_name):
    with open(path) as f:
        content = f.read()
    
    # Find fn NAME( ... }  (naive but effective for our codebase)
    # Handles multi-line functions with proper Rust indentation
    pattern = rf'(?:pub\s+)?\(?:crate\s+\)\s*fn\s+{re.escape(func_name)}\s*\([^)]*\)(?:\s*->\s*[^{{]+)?\s*\{{'
    match = re.search(pattern, content)
    if not match:
        # Fallback: simpler pattern
        pattern = rf'fn\s+{re.escape(func_name)}\s*\('
        match = re.search(pattern, content)
        if not match:
            raise ValueError(f"Could not find {func_name} in {path}")
    
    start = match.start()
    brace_count = 0
    in_string = False
    string_char = None
    i = start
    
    while i < len(content):
        ch = content[i]
        if not in_string:
            if ch in ('"', "'"):
                in_string = True
                string_char = ch
            elif ch == '{':
                brace_count += 1
            elif ch == '}':
                brace_count -= 1
                if brace_count == 0:
                    return content[start:i+1]
        else:
            if ch == string_char and content[i-1] != '\\':
                in_string = False
        i += 1
    
    raise ValueError(f"Could not find matching braces for {func_name}")

def normalize(text):
    """Remove comments and normalize whitespace."""
    # Remove // comments
    lines = []
    for line in text.split('\n'):
        # Keep the line if it's not a pure comment
        stripped = line.strip()
        if stripped.startswith('//'):
            continue
        # Remove inline // comments
        if '//' in line:
            line = line[:line.index('//')]
        lines.append(line.rstrip())
    
    # Remove blank lines and normalize indentation
    lines = [line for line in lines if line.strip()]
    return '\n'.join(lines)

def main():
    if len(sys.argv) != 4:
        print(f"Usage: {sys.argv[0]} <old_file> <new_file> <function_name>")
        sys.exit(1)
    
    old_path, new_path, func_name = sys.argv[1:4]
    
    try:
        old = extract_function(old_path, func_name)
        new = extract_function(new_path, func_name)
    except ValueError as e:
        print(f"❌ {func_name}: {e}")
        sys.exit(1)
    
    old_norm = normalize(old)
    new_norm = normalize(new)
    
    if old_norm == new_norm:
        print(f"✅ {func_name}: identical after normalization")
        sys.exit(0)
    else:
        print(f"❌ {func_name}: bodies differ!")
        import difflib
        diff = difflib.unified_diff(
            old_norm.split('\n'), 
            new_norm.split('\n'),
            fromfile=f"{old_path}:{func_name}",
            tofile=f"{new_path}:{func_name}",
            lineterm=''
        )
        print('\n'.join(diff))
        sys.exit(1)

if __name__ == '__main__':
    main()
```

### Usage

```bash
# After moving exec_reddit_search to reddit/search.rs:
python scripts/verify_move.py \
  src-tauri/src/engine/exec/reddit.rs.bak \
  src-tauri/src/engine/exec/reddit/search.rs \
  exec_reddit_search
```

---

## Appendix B: Golden Test Templates

### Rust: Recording Test

Add to the source file (inside `#[cfg(test)]`):

```rust
#[test]
#[ignore = "Run manually to regenerate golden files"]
fn record_golden_keyword_research() {
    let task = load_fixture("research_task_standard.json");
    let result = exec_keyword_research_native(&task, "/fixtures/project", "kimi");
    
    let snapshot = serde_json::json!({
        "success": result.success,
        "message": result.message,
        "output": result.output,
    });
    
    std::fs::write(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/keyword_research_native.json"),
        serde_json::to_string_pretty(&snapshot).unwrap()
    ).unwrap();
}
```

### Rust: Comparison Test

Add to `src-tauri/tests/golden_tests.rs`:

```rust
use pageseeds::engine::exec;

#[test]
fn keyword_research_native_matches_golden() {
    let task = load_fixture("research_task_standard.json");
    let result = exec::keywords::exec_keyword_research_native(
        &task, "/fixtures/project", "kimi"
    );
    
    let golden_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/keyword_research_native.json");
    let golden: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(golden_path).unwrap()
    ).unwrap();
    
    let actual = serde_json::json!({
        "success": result.success,
        "message": result.message,
        "output": result.output,
    });
    
    assert_eq!(actual, golden, "Refactor changed behavior. Check diff.");
}
```

### TypeScript: Hook Shadow Test

```typescript
// hooks/useTasks.test.ts
import { renderHook, waitFor } from '@testing-library/react'
import { useTasks } from './useTasks'

// Temporary: inline the old implementation for comparison
function useTasksLegacy(projectId: string) {
  // paste exact original useState/useEffect/useQuery from TaskBoard
}

it('produces identical output to legacy', async () => {
  const { result: newResult } = renderHook(() => useTasks('proj-123'))
  const { result: oldResult } = renderHook(() => useTasksLegacy('proj-123'))
  
  await waitFor(() => {
    expect(newResult.current.tasks).toEqual(oldResult.current.tasks)
  })
})
```

---

## Appendix C: Troubleshooting

### "cargo check passes but tests fail after move"

**Cause:** You moved a function but forgot to move a helper type or constant it depends on.

**Fix:** Check the original file for `struct`, `enum`, or `const` items defined near the function. Move them too.

### "verify_move.py says functions differ but they look the same"

**Cause:** LLM changed `"` to `'` in a string, or reformatted a `format!` call.

**Fix:** Run `cargo fmt` on both files, then compare again. If still different, the LLM changed logic — reject it.

### "Golden test fails after refactor"

**Cause:** The function output depends on something that moved incorrectly (e.g., a path resolution changed).

**Fix:** 
1. Run the recording test again to see the new output
2. `git diff` the golden file
3. If the diff is non-empty and unexpected, the refactor changed behavior — revert and investigate

### "Step registry macro doesn't compile"

**Cause:** The macro doesn't handle lifetimes or generic return types correctly.

**Fix:** Convert one registration manually, verify it compiles, then generalize the macro. Don't batch-convert.

### "Frontend TypeScript errors after hook extraction"

**Cause:** The hook returns a slightly different shape than the inline code.

**Fix:** Ensure the hook returns exactly the same fields. Run `pnpm exec tsc -b` after every extraction.

---

## Summary: The Weekly Rhythm

| Week | Focus | Deliverable |
|------|-------|-------------|
| **Week 1** | Phase 0 + Phase 1 (small exec modules) | `social/`, `gsc/`, `ctr_audit/`, `research/` split |
| **Week 2** | Phase 1 (large exec modules) | `reddit/`, `content/`, `keywords/` split |
| **Week 3** | Phase 2 | Commands are thin wrappers, queue runner extracted |
| **Week 4** | Phase 3 | `register_blocking!` macro, JSON helpers |
| **Week 5** | Phase 4 | Domain hooks, `Overview.tsx` split, `TaskBoard.tsx` split |
| **Week 6** | Phase 5 | Docs archived, scripts pruned, dead code removed |

**Rule:** One commit per file moved. One PR per phase. No mixing concerns.
