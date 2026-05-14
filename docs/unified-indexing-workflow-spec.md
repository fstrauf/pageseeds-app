# Unified Indexing Health Workflow Spec

> **Status:** Draft  
> **Scope:** Connect `indexing_diagnostics`, `cannibalization_audit`, `content_audit`, `gsc_indexing_recovery`, and `fix_indexing` into a single orchestrated workflow that knows what data it needs, checks freshness, and either runs prerequisites or prompts the user when manual steps are required.

---

## 1. Problem Statement

We have five separate workflows that each touch the same problem — Google refusing to index pages — but they run in isolation:

| Workflow | Detects | Fixes | Blind Spot |
|----------|---------|-------|------------|
| `indexing_diagnostics` | `not_indexed_crawled` | Spawns generic `fix_indexing` | No cluster context; agent guesses at root cause |
| `gsc_indexing_recovery` | Zero-link not-indexed URLs | Adds internal links | **Skips URLs with `>=1` incoming link** — the exact pages that are long and well-linked but still not indexed |
| `cannibalization_audit` | TF-IDF clusters, exact keyword dupes, hub gaps | Spawns `consolidate_cluster` or `write_article` (hub) | **Filters out articles with no GSC data** — so `not_indexed_crawled` pages are excluded from clustering entirely |
| `content_audit` | Per-article quality (word count, H1, links, readability) | Spawns `fix_content_article` | No cross-article title/H1 distinctiveness check |
| `fix_indexing` | Single URL context | Agent edits MDX | No structured comparison against similar articles on the site |

**The result:** For pages like `selling-covered-calls-dividend-stocks` and `bear-put-spread-strategy` — which are long, well-linked, and technically sound — every pipeline either skips them or fires a generic agent prompt that lacks the context to fix the real issue (topical overlap / cannibalization).

---

## 2. Goals

1. **One entry point:** A single `indexing_health_campaign` task that coordinates the full pipeline.
2. **Self-aware prerequisites:** The workflow checks whether prerequisite data exists and is fresh. If not, it either auto-runs the prerequisite or returns a clear message to the user.
3. **Connected context:** When fixing a `not_indexed_crawled` URL, the agent receives structured cluster context — sibling article titles, H1s, shared headings, and exact keyword duplicates.
4. **Agentic distinctiveness judgment:** A dedicated agentic step that compares the target article against its cluster siblings and decides whether the page is sufficiently distinct, should be merged, or needs a rewrite of its title/H1/intro.
5. **No duplication:** Reuse existing `StepKind` variants, handlers, and post-actions. The unified workflow is an **orchestrator**, not a replacement.
6. **User-in-the-loop for heavy ops:** Auto-run lightweight prerequisites (`content_audit`, `link_scan`). Prompt the user before running expensive or destructive ops (`cannibalization_audit`, `consolidate_cluster`).

---

## 3. Non-Goals

- **Not a new handler family** for every sub-step. Reuse `ImplementationHandler`, `CannibalizationAuditHandler`, etc.
- **Not automatic merging without user approval.** `consolidate_cluster` remains user-enqueued.
- **Not a rewrite of existing step implementations.** This spec adds orchestration steps and one new agentic step (`IndexingDistinctivenessReview`).

---

## 4. Prerequisite Data Model

The unified workflow depends on four artifact files. Before running fixes, it validates each:

| Artifact | Source | Freshness Threshold | Auto-Runnable? |
|----------|--------|---------------------|----------------|
| `gsc_collection.json` | `collect_gsc` or `indexing_diagnostics` | 7 days | Yes (`indexing_diagnostics`) |
| `link_scan.json` | `cluster_and_link` or `gsc_recovery_prepare` | 7 days | Yes (`gsc_recovery_prepare` refreshes it) |
| `cannibalization_strategy.json` | `cannibalization_audit` | 30 days | **No — requires user review** (`CannibalizationPicker`) |
| `content_audit.json` | `content_audit` | 14 days | Yes (`content_audit` is fast) |

**Freshness check logic:**
- Read `generated_at` (or file mtime as fallback) from each artifact.
- If missing or stale, log which prerequisite is needed.
- If auto-runnable: enqueue the prerequisite task and pause the parent with status `pending_prerequisite`.
- If manual: set the parent to `review` with a `prerequisite_review` surface telling the user exactly what to run.

---

## 5. Unified Workflow Architecture

### 5.1 New Task Type

```rust
TaskDefinition {
    task_type: "indexing_health_campaign",
    phase: "investigation",
    run_policy: TaskRunPolicy::UserEnqueue,
    review_surface: TaskReviewSurface::ArtifactReview,
    follow_up_policy: FollowUpPolicy::BackendAuto,
    handler_family: HandlerFamily::Implementation,
}
```

### 5.2 Handler Plan

```rust
"indexing_health_campaign" => vec![
    // Step 1 (deterministic): Check all prerequisite artifacts for freshness.
    // Returns a structured report of what's fresh, what's stale, what's missing.
    // If any auto-runnable prerequisite is stale, spawns it and pauses.
    WorkflowStep::new("ihc_check_prerequisites", StepKind::IhcCheckPrerequisites),

    // Step 2 (deterministic): Load GSC indexing status + drift.
    // Reuses exec_gsc_drift logic. Produces the list of not_indexed URLs.
    WorkflowStep::new("ihc_drift_analysis", StepKind::GscRecoveryDrift),

    // Step 3 (deterministic): Load cannibalization clusters and match each
    // not-indexed URL to its cluster. Build per-target context artifacts.
    WorkflowStep::new("ihc_build_target_context", StepKind::IhcBuildTargetContext),

    // Step 4 (deterministic): Run or load content_audit.json.
    // Fast; runs inline if stale.
    WorkflowStep::new("ihc_content_audit", StepKind::ContentAuditRun),

    // Step 5 (agentic): For each not-indexed URL that has cluster siblings,
    // ask the agent to judge distinctiveness. Returns structured verdicts.
    // One agent call per target to stay within prompt budget.
    WorkflowStep::new("ihc_distinctiveness_review", StepKind::IhcDistinctivenessReview)
        .with_param(step_params::SKILL, "indexing-distinctiveness"),

    // Step 6 (deterministic): Merge all step outputs into a single campaign plan.
    // Writes indexing_campaign_plan.json.
    WorkflowStep::new("ihc_reduce_plan", StepKind::IhcReducePlan),
]
```

### 5.3 Post-Actions

After `indexing_health_campaign` completes:
1. Read `indexing_campaign_plan.json`.
2. For each target URL, spawn the appropriate child task based on the plan's `recommended_action`:
   - `"fix_content"` → `fix_content_article` (thin content, broken links, etc.)
   - `"add_links"` → `fix_indexing_internal_links` (zero incoming links)
   - `"merge"` → **Do NOT auto-spawn.** Record as a `merge_candidate` artifact. User must approve via `CannibalizationPicker`.
   - `"rewrite_title_h1"` → `fix_indexing` with enriched cluster context
   - `"no_action"` → Nothing (page is distinct, just waiting on Google)
3. Emit a summary artifact with all spawned task IDs and pending user actions.

---

## 6. Step Specifications

### 6.1 `IhcCheckPrerequisites`

**Deterministic.** No agent.

**Inputs:**
- `project_path`
- Hard-coded artifact paths and freshness thresholds (see §4)

**Logic:**
```rust
let checks = vec![
    check_artifact("gsc_collection.json", Duration::days(7)),
    check_artifact("link_scan.json", Duration::days(7)),
    check_artifact("cannibalization_strategy.json", Duration::days(30)),
    check_artifact("content_audit.json", Duration::days(14)),
];
```

**Output:**
```json
{
  "all_fresh": false,
  "checks": [
    {"artifact": "gsc_collection.json", "fresh": true, "age_hours": 12},
    {"artifact": "link_scan.json", "fresh": true, "age_hours": 48},
    {"artifact": "cannibalization_strategy.json", "fresh": false, "age_hours": 720, "action": "user_must_run_cannibalization_audit"},
    {"artifact": "content_audit.json", "fresh": false, "age_hours": 400, "action": "auto_enqueue_content_audit"}
  ]
}
```

**Behavior:**
- If `all_fresh` is true → step succeeds, workflow continues.
- If any `action` is `"auto_enqueue_*"` → spawn the task via `TaskSpawner`, set parent status to `pending_prerequisite`, and pause the runner. The executor will resume when the prerequisite completes (see §8).
- If any `action` is `"user_must_run_*"` → set parent status to `review` with `review_surface = PrerequisiteReview`, and include the artifact name and reason in the task output.

### 6.2 `IhcBuildTargetContext`

**Deterministic.** No agent.

**Inputs:**
- `gsc_recovery_drift.json` (from previous step)
- `cannibalization_clusters.json`
- `cannibalization_audit_context.json`
- `content_audit.json`

**Logic:**
1. Load all `not_indexed` URLs from drift report.
2. For each URL, find its article record in `cannibalization_audit_context.json`.
3. Find the cluster it belongs to (by matching `id` in `cannibalization_clusters.json`).
4. Collect sibling pages from the same cluster (excluding the target).
5. Load the target's `content_audit` record.
6. Build a per-target context object.

**Output per target:**
```json
{
  "target": {
    "url": "...",
    "slug": "...",
    "reason_code": "not_indexed_crawled",
    "title": "...",
    "h1": "...",
    "word_count": 3200,
    "incoming_links": 4,
    "content_audit_health": "good"
  },
  "cluster": {
    "cluster_id": "covered_calls",
    "theme": "covered calls",
    "sibling_count": 7,
    "siblings": [
      {"url": "...", "title": "...", "h1": "...", "word_count": 2800, "impressions": 1200},
      {"url": "...", "title": "...", "h1": "...", "word_count": 1900, "impressions": 800}
    ],
    "shared_headings": ["Strike Selection", "Risk Management"],
    "exact_keyword_dupe": false
  },
  "diagnosis": {
    "has_links": true,
    "is_long": true,
    "has_cluster_siblings": true,
    "suspected_root_cause": "cannibalization"
  }
}
```

**Why this matters:** The downstream agent step (§6.4) and any spawned `fix_indexing` tasks receive this structured context instead of hunting around the repo.

### 6.3 `ContentAuditRun` (reused)

Call the existing `exec_content_audit` if `content_audit.json` is stale. If fresh, load and return a compact summary. This is already implemented; we just invoke it from the new handler.

### 6.4 `IhcDistinctivenessReview`

**Agentic.** Uses a skill file.

**Deterministic prep:**
- Batch the targets into groups of ~5 to stay within prompt budget.
- For each batch, build a compact prompt containing only:
  - Target URL + title + H1 + first 200 words
  - Sibling list (URL + title + H1 only — no full body)
  - Shared headings detected in the cluster

**Agent prompt (via skill `indexing-distinctiveness`):**
```
You are an SEO content strategist. For each target article below, compare its title, H1, and opening focus against its cluster siblings.

Decide for each target:
1. DISTINCT — the article covers a unique angle and should remain standalone.
2. OVERLAP — the article overlaps significantly with siblings. Recommend either:
   a. MERGE into the highest-performing sibling (specify keep_url and redirect_url)
   b. REWRITE title/H1/intro to establish a clearer unique angle
   c. NOINDEX is NOT an option — if it can't be made distinct, recommend MERGE

Return a JSON array of verdicts:
[
  {
    "target_url": "...",
    "verdict": "DISTINCT|OVERLAP",
    "confidence": "high|medium|low",
    "recommendation": "MERGE|REWRITE|NO_ACTION",
    "keep_url": "...",
    "redirect_url": "...",
    "reason": "...",
    "suggested_title": "...",
    "suggested_h1": "..."
  }
]
```

**Output:** Structured `Vec<DistinctivenessVerdict>` extracted via Rig `extract_structured`.

**Why agentic:** Deterministic word overlap fails on semantic nuance. An agent can recognize that "Selling Covered Calls on Dividend Stocks" is genuinely different from "Selling Covered Calls for Income" even though they share 60% of words. Conversely, it can flag that "Bear Put Spread Strategy" and "Put Credit Spread Guide" might be overlapping in practice despite different keywords.

### 6.5 `IhcReducePlan`

**Deterministic.** No agent.

**Inputs:**
- All `IhcBuildTargetContext` outputs
- All `IhcDistinctivenessReview` verdicts
- `content_audit` records

**Logic:**
For each target URL, decide the recommended action using this priority order:

```
IF content_audit health == "poor" → "fix_content"
ELSE IF incoming_links == 0 → "add_links"
ELSE IF distinctiveness_verdict == "OVERLAP" AND confidence == "high" → "merge" (user approval required)
ELSE IF distinctiveness_verdict == "OVERLAP" AND confidence in ("medium", "low") → "rewrite_title_h1"
ELSE IF reason_code == "not_indexed_crawled" AND is_long AND has_links → "no_action" (wait / request indexing)
ELSE → "fix_indexing" (generic fallback)
```

**Output:** `indexing_campaign_plan.json`
```json
{
  "generated_at": "...",
  "targets": [
    {
      "url": "...",
      "reason_code": "not_indexed_crawled",
      "recommended_action": "rewrite_title_h1",
      "context_artifact_key": "ihc_target_context_001",
      "distinctiveness_verdict": {...},
      "content_audit_summary": {...}
    }
  ],
  "summary": {
    "total_targets": 12,
    "fix_content": 2,
    "add_links": 3,
    "merge": 1,
    "rewrite_title_h1": 4,
    "no_action": 2
  }
}
```

---

## 7. User Interaction Model

### 7.1 Happy Path (all prerequisites fresh)

1. User enqueues `indexing_health_campaign`.
2. Workflow runs all 6 steps automatically.
3. Post-actions spawn child tasks for `fix_content`, `add_links`, and `rewrite_title_h1`.
4. Merge candidates are surfaced in the task's review artifact. User opens the task detail, sees the merge recommendation, and can approve it to spawn a `consolidate_cluster` task.

### 7.2 Missing Cannibalization Audit (manual prerequisite)

1. User enqueues `indexing_health_campaign`.
2. Step 1 detects `cannibalization_strategy.json` is 45 days old.
3. Parent task status → `review`, review_surface → `PrerequisiteReview`.
4. UI shows: *"Cannibalization audit data is stale (45 days). Run `cannibalization_audit` first to get accurate cluster context for indexing fixes."*
5. User runs `cannibalization_audit`, approves any merge recommendations.
6. User returns to `indexing_health_campaign` and clicks **Resume**.
7. Step 1 now passes; workflow continues.

### 7.3 Missing Content Audit (auto prerequisite)

1. Step 1 detects `content_audit.json` is 20 days old.
2. Post-action spawns a `content_audit` child task automatically.
3. Parent task status → `pending_prerequisite`.
4. When `content_audit` completes, the queue runner resumes the parent.

---

## 8. Resuming After Prerequisites

The executor and queue runner need a small enhancement:

- When a task is set to `pending_prerequisite`, the runner records the dependency in a new `task_prerequisites` table (or reuses `depends_on` semantics).
- When any task completes, check if it was a prerequisite for a `pending_prerequisite` parent.
- If all prerequisites are now satisfied, move the parent to `queued`.

**Alternative (simpler, no schema change):**
- Use `depends_on` plus `not_before`.
- The parent task stays in `todo` with `depends_on = [prereq_task_id]`.
- The existing queue runner already respects `depends_on` — it won't start the parent until the prerequisite task is `done`.
- The parent task just needs to be created with `depends_on` populated by the `IhcCheckPrerequisites` post-action.

**Recommended:** Use the simpler approach. `IhcCheckPrerequisites` post-action spawns prerequisite tasks and adds their IDs to the parent's `depends_on`. The parent task stays `todo`; the queue runner will naturally wait.

---

## 9. Reuse Matrix

| Existing Component | How It's Reused |
|--------------------|-----------------|
| `exec_gsc_drift` | Called directly in `ihc_drift_analysis` step |
| `exec_content_audit` | Called directly in `ihc_content_audit` step |
| `cannibalization_audit` artifacts | Read as input; not re-run |
| `TaskSpawner::spawn` | Post-actions spawn child `fix_content_article`, `fix_indexing_internal_links`, `fix_indexing` tasks |
| `consolidate_cluster` handler | Spawned only after user approves a merge recommendation |
| `gsc/indexing.rs` `classify_record` | Unchanged; still classifies `not_indexed_crawled` |
| `engine/exec/indexing_fix.rs` | Enhanced to accept `IhcBuildTargetContext` artifact as input |

---

## 10. New / Changed Files

| File | Change |
|------|--------|
| `config/task_definitions.rs` | Add `indexing_health_campaign` definition |
| `engine/workflows/handlers.rs` | Add `IndexingHealthCampaignHandler` |
| `engine/workflows/step_kind.rs` | Add `IhcCheckPrerequisites`, `IhcBuildTargetContext`, `IhcDistinctivenessReview`, `IhcReducePlan` |
| `engine/step_registry.rs` | Register new step kinds |
| `engine/executor.rs` | Match new step kinds (thin wrappers) |
| `engine/exec/indexing_health_campaign.rs` | **New file.** Contains `exec_ihc_check_prerequisites`, `exec_ihc_build_target_context`, `exec_ihc_distinctiveness_review`, `exec_ihc_reduce_plan` |
| `engine/exec/indexing_fix.rs` | Enhance `IndexingFixContext` to accept optional cluster context artifact |
| `engine/post_actions.rs` | Add post-action for `indexing_health_campaign` that reads plan and spawns children |
| `.github/skills/indexing-distinctiveness/SKILL.md` | **New skill.** Agent prompt for distinctiveness review |
| `models/indexing_health.rs` | **New file.** `DistinctivenessVerdict`, `IndexingCampaignPlan`, `IndexingTargetPlan` structs with `#[ts(export)]` |
| `lib/tauri.ts` | Add `indexingHealthCampaign` wrapper |
| `lib/types.ts` | Export new TypeScript types |

---

## 11. Open Questions

1. **Prompt budget for `IhcDistinctivenessReview`:** If a cluster has 20 siblings and 5 targets, do we batch by target (5 prompts) or by cluster (1 prompt with all targets + siblings)? Suggest starting with one prompt per target to keep context focused, and cap siblings at 8.
2. **Merge confidence threshold:** Should "high confidence merge" ever auto-spawn `consolidate_cluster`, or always require user approval? **Recommendation:** Always require approval. Merging is destructive.
3. **Frequency:** How often should `indexing_health_campaign` run? **Recommendation:** Monthly, or after any batch content publish.
4. **Integration with scheduler:** Should the scheduler auto-enqueue `indexing_health_campaign`? **Recommendation:** Yes, monthly via `scheduler.rs`.

---

## 12. Success Criteria

- [ ] `indexing_health_campaign` detects that `selling-covered-calls-dividend-stocks` has cluster siblings and routes it to `rewrite_title_h1` instead of generic `fix_indexing`.
- [ ] `indexing_health_campaign` pauses with a clear message when `cannibalization_strategy.json` is stale.
- [ ] `fix_indexing` tasks spawned from the campaign include cluster sibling context in their agent prompt.
- [ ] No new `fix_indexing` task is spawned for a `not_indexed_crawled` URL that already has `>=1` incoming links unless the distinctiveness review says it's unique.
- [ ] `cannibalization_audit` no longer filters out zero-GSC articles from clustering.
