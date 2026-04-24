# SEO Improvement Workflow Implementation Todo

**Branch:** `seo-improvement-workflows`  
**Goal:** Implement `ctr_audit` and `cannibalization_audit` workflows with proper deterministic/agentic separation.

---

## Design Principles (from user feedback)

1. **Title analysis = agentic.** Cannot hard-code rules about title quality, brand patterns, duplication. Needs intelligence.
2. **Snippet analysis = agentic.** Cannot hard-code FAQ/snippet rules per site. Needs intelligence.
3. **Ranking/scoring = deterministic.** Math on GSC data is pure computation.
4. **Data collection = deterministic.** Gathering titles, meta descs, first paragraphs, GSC metrics into structured context.
5. **No hard-coded site-specific logic.** Must work for any website dynamically.
6. **Follow existing pattern.** Workflows run checks, then auto-spawn fix tasks (like `content_review` → `fix_content_article`).

---

## Phase 1: ctr_audit — Fix Clicks (CTR)

### Rust Backend

- [ ] **Add new step kinds to `engine/workflows/step_kind.rs`**
  - [ ] `CtrBuildContext` = `ctr_build_context`
  - [ ] `CtrAnalyze` = `ctr_analyze` (agentic)

- [ ] **Add `CtrAuditHandler` to `engine/workflows/handlers.rs`**
  - [ ] Supports task type `"ctr_audit"`
  - [ ] Plan: 4 steps
    1. `ctr_gsc_sync` (GscSyncArticles, optional)
    2. `ctr_build_context` (CtrBuildContext) — deterministic data collection
    3. `ctr_analyze` (CtrAnalyze, agentic) — skill: `"ctr-optimization"`
    4. `ctr_normalize` (Normalizer) — artifact: `ctr_recommendations`

- [ ] **Implement `engine/exec/ctr_audit.rs`**
  - [ ] `exec_ctr_build_context(task, project_path)` — deterministic
    - Reads articles.json
    - For each article: collects title, meta_description, first_paragraph, h1, target_keyword, url_slug, GSC metrics
    - Computes deterministic CTR metrics: clicks_lost = impressions * max(0, target_ctr - actual_ctr)
    - Ranks articles by clicks_lost descending
    - Outputs structured JSON context for agent
    - NO hard-coded quality judgments — just raw data + math
  - [ ] `exec_ctr_analyze(task, project_path, agent_provider)` — agentic wrapper
    - Builds prompt from context + skill
    - Calls agent
    - Returns raw output for normalizer

- [ ] **Wire into `engine/executor.rs`**
  - [ ] Match arm for `StepKind::CtrBuildContext`
  - [ ] Match arm for `StepKind::CtrAnalyze`
  - [ ] Pass `latest_raw_output` from build_context to analyze step

- [ ] **Add auto-spawn logic in spawner or executor**
  - [ ] After `ctr_audit` completes successfully, read `ctr_recommendations` artifact
  - [ ] Spawn `fix_title_meta` task (idempotency: `ctr_fix:title_meta:{project_id}`)
  - [ ] Spawn `fix_faq_schema` task (idempotency: `ctr_fix:faq:{project_id}`)
  - [ ] Spawn `fix_snippet_bait` task (idempotency: `ctr_fix:snippet:{project_id}`)
  - [ ] Each fix task gets relevant subset of recommendations as artifact

- [ ] **Add to `config/mod.rs`**
  - [ ] `"ctr_audit"` to TASK_TYPES
  - [ ] default_execution_mode: `automatic`
  - [ ] default_phase: `"investigation"`

- [ ] **Register commands in `lib.rs`**

### SKILL.md

- [ ] **Create `.github/automation/skills/ctr-optimization.md`**
  - Prompt: analyze titles, meta descriptions, FAQ presence, snippet readiness
  - Input contract: structured JSON with per-article data + CTR metrics
  - Output contract: recommendations array with fixes per article
  - Rules: prioritize by clicks_lost, specific rewrite suggestions, no generic advice

### Frontend

- [ ] **Add types to `src/lib/types.ts`**
  - [ ] `CtrRecommendation`, `CtrFix`, etc.

- [ ] **Add invoke wrappers to `src/lib/tauri.ts`**
  - [ ] Any new commands needed

- [ ] **Add `ctr_audit` to task creation UI**
  - [ ] `src/components/tasks/TaskCreate.tsx`

---

## Phase 2: cannibalization_audit — Fix Impressions

### Rust Backend

- [ ] **Add new step kinds to `engine/workflows/step_kind.rs`**
  - [ ] `CanBuildContext` = `can_build_context`
  - [ ] `CanAnalyze` = `can_analyze` (agentic)

- [ ] **Add `CannibalizationAuditHandler` to `engine/workflows/handlers.rs`**
  - [ ] Supports task type `"cannibalization_audit"`
  - [ ] Plan: 5 steps
    1. `can_gsc_sync` (GscSyncArticles, optional)
    2. `can_coverage_load` (CoverageLoadArticles)
    3. `can_build_context` (CanBuildContext) — deterministic: TF-IDF similarity + data formatting
    4. `can_analyze` (CanAnalyze, agentic) — skill: `"cannibalization-strategy"`
    5. `can_normalize` (Normalizer) — artifact: `cannibalization_strategy`

- [ ] **Implement `engine/exec/cannibalization_audit.rs`**
  - [ ] `exec_can_build_context(task, project_path)` — deterministic
    - Loads articles.json
    - Computes TF-IDF similarity matrix on [title, h1, target_keyword, first_200_words]
    - Finds similarity pairs > 0.7 threshold
    - Groups articles by shared target_keyword
    - Collects GSC metrics per article
    - Formats everything into structured JSON context
    - NO judgment about what to merge — just data + math
  - [ ] `exec_can_analyze(task, project_path, agent_provider)` — agentic wrapper
    - Builds prompt from context + skill
    - Calls agent
    - Returns raw output for normalizer

- [ ] **Wire into `engine/executor.rs`**
  - [ ] Match arm for `StepKind::CanBuildContext`
  - [ ] Match arm for `StepKind::CanAnalyze`
  - [ ] Pass `latest_raw_output` from build_context to analyze step

- [ ] **Add auto-spawn logic**
  - [ ] After `cannibalization_audit` completes, read `cannibalization_strategy` artifact
  - [ ] Spawn `fix_content_merge` task (idempotency: `can_fix:merge:{project_id}`)
  - [ ] Spawn `fix_hub_page` task (idempotency: `can_fix:hub:{project_id}`)
  - [ ] Spawn `research_territory` task (idempotency: `can_fix:territory:{project_id}`)

- [ ] **Add to `config/mod.rs`**
  - [ ] `"cannibalization_audit"` to TASK_TYPES
  - [ ] default_execution_mode: `automatic`
  - [ ] default_phase: `"investigation"`

- [ ] **Register commands in `lib.rs`**

### SKILL.md

- [ ] **Create `.github/automation/skills/cannibalization-strategy.md`**
  - Prompt: analyze similarity clusters, recommend merges, hub pages, new territories
  - Input contract: structured JSON with similarity pairs, article metadata, GSC metrics
  - Output contract: merge_recommendations, hub_recommendations, territory_recommendations
  - Rules: use impressions as authority proxy, be specific about keepers/redirects

### Frontend

- [ ] **Add types to `src/lib/types.ts`**
  - [ ] `CannibalizationStrategy`, `MergeRecommendation`, `HubRecommendation`, etc.

- [ ] **Add invoke wrappers to `src/lib/tauri.ts`**

- [ ] **Add `cannibalization_audit` to task creation UI**
  - [ ] `src/components/tasks/TaskCreate.tsx`

---

## Phase 3: Testing & Verification

- [ ] **Test `ctr_audit` end-to-end**
  - [ ] Create task for daystoexpiry project
  - [ ] Run workflow
  - [ ] Verify `ctr_recommendations` artifact produced
  - [ ] Verify follow-up tasks auto-spawned
  - [ ] Verify fix tasks have correct artifact subsets

- [ ] **Test `cannibalization_audit` end-to-end**
  - [ ] Create task for daystoexpiry project
  - [ ] Run workflow
  - [ ] Verify `cannibalization_strategy` artifact produced
  - [ ] Verify follow-up tasks auto-spawned

- [ ] **Run lint + typecheck + build**
  - [ ] `pnpm run lint`
  - [ ] `pnpm exec tsc -b`
  - [ ] `pnpm run build`
  - [ ] `cargo check`

---

## Notes

### Deterministic vs Agentic Boundaries

| What | Mode | Why |
|---|---|---|
| Collect titles/meta/GSC metrics | Deterministic | Data gathering, no judgment |
| Compute CTR scores, clicks lost | Deterministic | Math formula |
| Compute TF-IDF similarity | Deterministic | Pure math |
| Assess title quality | **Agentic** | Requires understanding brand, intent, competition |
| Assess meta description quality | **Agentic** | Requires understanding benefit/CTA/context |
| Detect FAQ/snippet issues | **Agentic** | Requires understanding content structure per site |
| Decide which article to keep in merge | **Agentic** | Requires weighing authority, quality, brand alignment |
| Recommend hub page topics | **Agentic** | Requires understanding taxonomy and search demand |

### Pattern to Follow

```
Deterministic step: build structured context from raw data
    |
Agentic step: analyze context, produce recommendations
    |
Normalizer: enforce output contract
    |
Auto-spawn: create fix tasks from recommendations artifact
```

This mirrors the existing `content_review` pattern:
- `content_review_gsc_sync` → `content_review_audit` → `content_review_sync` → `content_review_recommend` (agentic)
- Then auto-spawns `fix_content_article` tasks
