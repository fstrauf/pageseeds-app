# PageSeeds SEO Discovery Workflow — Technical Specification

**Status:** Draft  
**Author:** AI Agent working session  
**Date:** 2026-07-09  
**Goal:** Make PageSeeds automatically surface, rank, and act on the same kinds of SEO opportunities we currently discover manually for sites like brewedlate.com.

---

## 1. Executive Summary

PageSeeds already has strong *specialist* audits (CTR, indexing, cannibalization, Clarity UX, content quality), but they run in silos. A user who wants to answer "what should we do next to grow organic traffic?" has to run several tasks, open several review surfaces, and mentally merge the results. This spec proposes a single **unified SEO discovery loop** that reuses the existing steps and artifacts, adds one deterministic cross-signal ranker, and surfaces a ranked backlog of opportunities that the user can approve into fix tasks.

The concrete trigger for this work was the **brewedlate.com cold-brew CTR opportunity**: a page with impressions and a mid-SERP position but a CTR well below the position target. Existing `ctr_audit` can compute this, but it only spawns fixes when source-level health checks fail (title length, meta length, FAQ, etc.). A page with a technically valid title but a boring/competing snippet is invisible to the current follow-up logic. The same pattern appears for indexing, cannibalization, and UX signals: each audit finds its own slice, and no workflow fuses them into a single prioritised action list.

**High-level proposal**

1. Add a new `seo_health_scan` umbrella task that orchestrates the existing `content_audit`, `ctr_audit` context build, `indexing_health_campaign` prerequisites, `cannibalization_audit` context, and optional Clarity summary.
2. Add a deterministic `RankOpportunities` step that reads all of those artifacts and emits one `seo_opportunities.json` ranked by expected traffic impact and fix effort.
3. Surface the ranked list in a new `OpportunityReview` UI, letting the user select which opportunities become `fix_content_article`, `fix_ctr_article`, `fix_indexing_internal_links`, or `consolidate_cluster` children.
4. Tighten the existing CTR pipeline so it does not silently drop pure snippet/title-quality opportunities when source-level health checks pass.
5. Persist the opportunity backlog in SQLite so discovery is stateful across runs.

---

## 2. Current State

### 2.1 Task types that already discover issues

| Task type | What it discovers | Where the logic lives | Follow-up today |
|-----------|-------------------|----------------------|-----------------|
| `content_review` / `content_audit` | Top 20 priority articles from `content_audit` + GSC; agent writes recommendations | `src-tauri/src/engine/exec/content/review.rs` selects; agent generates `recommendations.json` | `create_fix_content_article_tasks` in `src-tauri/src/engine/exec/content/task_spawner.rs` spawns `fix_content_article` children |
| `ctr_audit` | Articles with title/meta/FAQ/snippet issues; computes `clicks_lost` per article | `src-tauri/src/engine/exec/ctr_audit/context.rs` builds context; `src-tauri/src/engine/exec/ctr_audit/task_spawner.rs` spawns fixes | `create_ctr_fix_tasks` spawns `fix_ctr_article` children only when `issues_detected` flags are true |
| `indexing_health_campaign` | Not-indexed URLs, cluster siblings, thin content, missing internal links | `src-tauri/src/engine/exec/indexing_health/build_context.rs` + `reduce.rs` | `spawn_campaign_children` in `src-tauri/src/engine/exec/indexing_health/spawn.rs` spawns `fix_content_article`, `fix_indexing_internal_links`, `fix_indexing` children |
| `cannibalization_audit` | TF-IDF similarity clusters, duplicate target keywords, hub gaps | `src-tauri/src/engine/exec/cannibalization/build_context.rs` + `analyze.rs` | `create_can_fix_tasks` intentionally returns `Vec::new()`; user must approve via `CannibalizationPicker` |
| `clarity_analytics` | UX anomalies: rage/dead/quickback clicks, scroll depth, engagement | `src-tauri/src/engine/exec/clarity/investigate.rs` | `ArtifactReview` only — no child tasks |
| `generate_feature_spec` | Developer-focused spec from audit findings | `src-tauri/src/engine/post_actions.rs` spawns it after `content_review`/`content_audit`/`ctr_audit`/`indexing_health_campaign` | None |

### 2.2 Where the data lives

All of these write artifacts to the project's `.github/automation/` dir and/or to the app SQLite database:

- `content_audit.json` / DB `content_audit_runs` + `article_content_audits`
- `ctr_audit_context.json` / DB `content_audit_artifacts` key `ctr_audit_context`
- `indexing_target_contexts.json`, `indexing_campaign_plan.json` / DB `content_audit_artifacts`
- `cannibalization_clusters.json`, `cannibalization_strategy.json` / DB `content_audit_artifacts`
- `clarity_summary.json`
- `recommendations.json`

### 2.3 Key code paths

- Task lifecycle metadata (run policy, review surface, follow-up policy): `src-tauri/src/config/task_definitions.rs`
- Step plans: `src-tauri/src/engine/workflows/handlers.rs`
- Follow-up spawning: `src-tauri/src/engine/post_actions.rs`
- Content-review priority scoring: `src-tauri/src/engine/exec/content/review.rs:124-152`
- CTR context + `clicks_lost`: `src-tauri/src/engine/exec/ctr_audit/context.rs:95-336`
- CTR fix spawning (and the current filter): `src-tauri/src/engine/exec/ctr_audit/task_spawner.rs:66-91`

### 2.4 What already works well

- Deterministic scoring is fast and cheap.
- `content_review` already cross-references content audit failures with GSC position/CTR/quality score.
- `indexing_health_campaign` already fuses indexing drift, cannibalization clusters, and content audit.
- `cannibalization_audit` already requires user approval before destructive merges.
- The fix pipelines (`fix_content_article`, `fix_ctr_article`, `fix_indexing_internal_links`) are reusable.

---

## 3. Gaps We Discovered

### 3.1 No unified priority list across all signals

A single page can simultaneously have:
- a content-audit health of "needs_improvement",
- a CTR well below its position target,
- zero or few internal links,
- a cannibalization cluster sibling, and
- a Clarity quickback-rate anomaly.

Today those findings live in five different artifacts and review surfaces. There is no deterministic step that reads all five and says "this page is your #1 opportunity because fixing it hits content + CTR + indexing + UX at once."

### 3.2 `ctr_audit` drops pure snippet-quality opportunities

In `src-tauri/src/engine/exec/ctr_audit/task_spawner.rs:66-91`, a `fix_ctr_article` task is only created when one of these source-level flags is true:

- `file_not_found`
- `title_too_long`
- `meta_too_short`
- `snippet_suboptimal`
- `missing_faq_schema` (and no frontmatter FAQ)

The brewedlate cold-brew case had a technically valid title/meta but a snippet that was not compelling versus competitors. If `check_article_health` returned all-OK, the article would be skipped as healthy (`skipped_healthy += 1`) even though `clicks_lost` was large. The deterministic `clicks_lost` score is computed but not used to decide whether to spawn a fix.

### 3.3 `content_review` excludes non-indexed articles

`src-tauri/src/engine/exec/content/review.rs:350-387` filters out any article whose slug appears in `gsc_collection.json` with a `not_indexed*` reason. That is correct for content fixes (Google cannot reward content it cannot see), but it means indexing problems are invisible to the umbrella review. A unified opportunity list should keep indexing issues in their own lane rather than dropping them silently.

### 3.4 `clarity_analytics` findings are not connected to GSC opportunity

`clarity_analytics` produces `clarity_summary.json` with z-scored UX anomalies. Today it is a standalone investigation. A page with both high `clicks_lost` and high quickback rate is a much stronger opportunity than either signal alone, but the two are never joined.

### 3.5 There is no persistent opportunity backlog

Each audit run is a snapshot. If a page is not in the top N of a given run, it is gone. There is no SQLite table that accumulates opportunities, tracks which were accepted/declined/done, and resurfaced stale ones.

### 3.6 `generate_feature_spec` is developer-focused, not editorial

The existing feature-spec task is useful for planning repo-level changes, but it does not produce the editorial backlog the user actually asked for: a ranked list of "fix this page next" actions.

---

## 4. Proposed Changes

### 4.1 New task type: `seo_health_scan`

Add a new task definition in `src-tauri/src/config/task_definitions.rs`:

```rust
TaskDefinition {
    task_type: "seo_health_scan",
    phase: "investigation",
    run_policy: TaskRunPolicy::UserEnqueue,
    review_surface: TaskReviewSurface::OpportunityReview, // new surface
    follow_up_policy: FollowUpPolicy::UserSelection,
    handler_family: HandlerFamily::Implementation, // or a new Discovery family
}
```

Add a handler in `src-tauri/src/engine/workflows/handlers.rs` (or extend `ContentReviewHandler`) with this step plan:

1. `gsc_sync` (optional) — refresh GSC page + query metrics.
2. `content_audit` — deterministic 21-check content quality.
3. `ctr_build_context` — deterministic `clicks_lost` + query intent.
4. `can_build_context` (optional) — cannibalization clusters + hub gaps.
5. `ihc_build_target_context` (optional) — not-indexed URLs + cluster siblings.
6. `clarity_summarise` (optional) — UX anomaly scores if Clarity is configured.
7. `rank_opportunities` — **new deterministic step** that fuses all of the above into `seo_opportunities.json`.
8. `opportunity_review_agent` (optional) — **new agentic step** that writes a one-paragraph explanation per top opportunity.

The optional steps depend on configured integrations (Clarity project ID, GSC service account). The core step is `rank_opportunities`, which must not require any API call.

### 4.2 New deterministic step: `RankOpportunities`

Implement in a new file `src-tauri/src/engine/exec/seo_discovery/rank.rs`.

**Inputs** (all already exist on disk/DB):

- `content_audit` DB snapshot or `content_audit.json`
- `ctr_audit_context.json` (DB key `ctr_audit_context`)
- `cannibalization_candidates.json` (DB key `cannibalization_candidates`) — primary cannibalization evidence
- `exact_keyword_duplicates.json` — fallback exact-keyword dupe evidence
- `cannibalization_clusters.json` — soft TF-IDF clusters; **hub_gap only** for evidence-matched articles, not authority for `cannibalized`
- `indexing_target_contexts.json`
- `clarity_summary.json`
- `articles.json`

**Output**: `seo_opportunities.json` in `.github/automation/`, plus a DB table `seo_opportunities` for persistence.

**Per-page signal extraction**

For every article in `articles.json`, compute:

| Signal | Source | Value |
|--------|--------|-------|
| `content_health` | content audit | `"good"`, `"needs_improvement"`, `"poor"` + `checks_failed` + `health_score` |
| `clicks_lost` | CTR context | `impressions * max(0, target_ctr - actual_ctr)` |
| `ctr_opportunity` | CTR context | boolean: `clicks_lost > 10` and `avg_position <= 20` |
| `indexing_status` | GSC collection / indexing contexts | `"indexed"`, `"not_indexed_crawled"`, `"not_indexed_other"`, `"unknown"` |
| `cannibalized` | cannibalization_candidates / exact_keyword_duplicates | boolean: fail-closed evidence only — article appears in the evidence shortlist (`cannibalization_candidates`) or an exact-keyword dupe group (`exact_keyword_duplicates`, non-empty shared target_keyword, ≥2 pages). Soft TF-IDF `cannibalization_clusters` membership is **not** authority and must not set this flag. |
| `hub_gap` | soft clusters (only if `cannibalized`) | boolean: matching soft cluster has no hub page — only evaluated after honest cannibalization evidence is present; soft cluster alone never implies cannibalized |
| `ux_anomaly` | clarity_summary | z-score for this URL, if present |
| `internal_links` | content audit | count |
| `word_count` | content audit | count |
| `target_keyword` | articles.json | string |

**Scoring function**

```rust
fn opportunity_score(s: &Signals) -> i64 {
    let mut score = 0i64;

    // CTR: 1 click lost ≈ 1 point, scaled by position urgency
    score += (s.clicks_lost * 10.0) as i64;
    if s.ctr_opportunity && s.avg_position <= 10.0 {
        score += 500;
    }

    // Content health
    score += match s.content_health.as_str() {
        "poor" => 800,
        "needs_improvement" => 400,
        _ => 0,
    };
    score += s.checks_failed * 25;
    score += (100 - s.health_score).max(0) * 3;

    // Indexing: not-indexed pages are high priority but not always fixable quickly
    score += match s.indexing_status.as_str() {
        "not_indexed_crawled" => 600,
        "not_indexed_other" => 300,
        _ => 0,
    };

    // Cannibalization: only if the cluster has no hub or is exact-duplicate keyword
    if s.cannibalized && s.hub_gap {
        score += 500;
    } else if s.cannibalized {
        score += 250;
    }

    // UX anomaly: weight only when there is also search traffic (avoid noise on tiny pages)
    if s.ux_anomaly_z_score > 2.0 && s.impressions > 50.0 {
        score += (s.ux_anomaly_z_score * 100.0) as i64;
    }

    // Quick-win boost: low word count + internal links == cheap to fix
    if s.word_count > 0 && s.word_count < 600 && s.internal_links < 3 {
        score += 200;
    }

    // Deduplicate already-fixed pages
    if s.review_status == "in_review" || s.recently_edited_days < 30 {
        score = 0;
    }

    score
}
```

The exact weights should be configurable per project in the long run, but the first version can hard-code them with comments explaining the rationale.

**Effort classification**

Each opportunity also gets an `effort` estimate:

- `low`: title/meta/intro tweak, add FAQ, add internal link
- `medium`: rewrite section, merge small cannibalized page, add Related Articles
- `high`: full rewrite, consolidate cluster, framework/template change

Effort is derived deterministically from the signal combination:

```rust
if s.indexing_status.starts_with("not_indexed") && s.internal_links == 0 {
    "low" // add internal links
} else if s.cannibalized && s.hub_gap {
    "high" // cluster consolidation
} else if s.content_health == "poor" || s.word_count < 600 {
    "medium"
} else if s.ctr_opportunity {
    "low"
} else {
    "medium"
}
```

**Recommended action**

Map the top signal to a concrete task type:

```rust
fn recommended_action(s: &Signals) -> &'static str {
    if s.indexing_status.starts_with("not_indexed") {
        "fix_indexing_internal_links"
    } else if s.cannibalized && s.hub_gap {
        "consolidate_cluster"
    } else if s.ctr_opportunity && s.content_health != "poor" {
        "fix_ctr_article"
    } else {
        "fix_content_article"
    }
}
```

### 4.3 Persist opportunities in SQLite

Add a new migration in `src-tauri/src/db/mod.rs` for a `seo_opportunities` table:

```sql
CREATE TABLE seo_opportunities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    article_id INTEGER NOT NULL,
    url_slug TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    opportunity_score INTEGER NOT NULL,
    effort TEXT NOT NULL,
    recommended_action TEXT NOT NULL,
    signals_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open', -- open, accepted, declined, done
    accepted_at TEXT,
    resulting_task_id TEXT,
    UNIQUE(project_id, article_id, generated_at)
);
CREATE INDEX idx_seo_opportunities_project_score ON seo_opportunities(project_id, status, opportunity_score DESC);
```

On each `seo_health_scan` run:
1. Insert new rows for articles not already present with `status = 'open'`.
2. Mark stale rows (>90 days, still `open`) as `declined` or re-score them.
3. Keep `accepted`/`done`/`declined` history.

### 4.4 New review surface: `OpportunityReview`

Add `OpportunityReview` to `TaskReviewSurface` in `src-tauri/src/models/task.rs` (and regenerate bindings). The frontend component (`src/components/review/OpportunityReview.tsx`) should:

1. Load `seo_opportunities.json` and/or query the DB table.
2. Show a ranked table: page, score, effort, primary signal, recommended action.
3. Let the user check/uncheck rows.
4. On "Create tasks", call a new command `create_tasks_from_opportunities` that spawns the appropriate child tasks via `TaskSpawner::spawn`.

The command lives in `src-tauri/src/commands/seo_discovery.rs` and is thin: validate input → call `engine::exec::seo_discovery::spawn_from_opportunities` → return task IDs.

### 4.5 Fix the CTR pipeline's pure-opportunity blind spot

In `src-tauri/src/engine/exec/ctr_audit/task_spawner.rs`, change the spawn logic so that `clicks_lost` itself can trigger a fix task even when source-level health checks pass.

Current code (simplified):

```rust
let has_issues = issues["file_not_found"].as_bool().unwrap_or(false)
    || issues["title_too_long"].as_bool().unwrap_or(false)
    || issues["meta_too_short"].as_bool().unwrap_or(false)
    || issues["snippet_suboptimal"].as_bool().unwrap_or(false)
    || missing_source_faq;

if !has_issues {
    skipped_healthy += 1;
    continue;
}
```

Proposed change:

```rust
let clicks_lost = article["clicks_lost"].as_f64().unwrap_or(0.0);
let significant_ctr_opportunity = clicks_lost >= 10.0;

let has_issues = /* same flags as before */;

if !has_issues && !significant_ctr_opportunity {
    skipped_healthy += 1;
    continue;
}
```

When `significant_ctr_opportunity` is true but source-level checks pass, the spawned `fix_ctr_article` task should get an artifact that tells the agent: "CTR is below target; current title/meta are technically valid, so focus on snippet competitiveness, intent match, and compelling copy." This can be done by adding a hint to the `ctr_context` artifact or by adding a new `prompt_hint` field.

**Implementation status (issue #104):** `prompt_hint` is now shipped on each admitted CTR context article record when `detection_reasons` is pure underperformance (`ctr_underperformance` without `format_violation`). Context also includes `current_year` and `head_query` (highest-impression query after GSC enrichment). The `ctr-optimization` skill (v3) uses recovery mode for pure underperformance (year refresh, head-term, intent) rather than micro-CTR length-only rewrites. Residual work in this section (spawn gate relaxation via `clicks_lost` threshold in `task_spawner.rs`) may still apply separately from the analyze/context framing shipped in #104.

### 4.6 Cross-reference Clarity UX anomalies with CTR opportunity

In `RankOpportunities`, when a URL has both `clicks_lost > 0` and `ux_anomaly_z_score > 2.0`, add a `snippet_mismatch_likely` flag to the opportunity. This tells the user (and the agent) that searchers are landing and immediately bouncing — a strong signal the title/meta promises something the page does not deliver.

### 4.7 Update `generate_feature_spec` prompt to consume `seo_opportunities.json`

The post-action in `src-tauri/src/engine/post_actions.rs` that spawns `generate_feature_spec` should pass the latest `seo_opportunities.json` (if present) into the agent context. The existing skill at `.github/skills/feature-spec-generation/SKILL.md` already expects "audit findings"; extend its input contract to include the unified opportunity list so it can separate P0 code changes from P1 content fixes more accurately.

---

## 5. Implementation Plan

### Phase 1 — Minimal viable unified discovery (no UI)

1. **Add `seo_opportunities` SQLite table** (`src-tauri/src/db/mod.rs` migration).
2. **Implement `RankOpportunities` step** in new file `src-tauri/src/engine/exec/seo_discovery/rank.rs`.
3. **Register `seo_health_scan` task type** in `src-tauri/src/config/task_definitions.rs`.
4. **Add handler + step plan** in `src-tauri/src/engine/workflows/handlers.rs`.
5. **Write `seo_opportunities.json`** to `.github/automation/` and persist to DB.
6. **Wire post-action**: after `seo_health_scan`, do not auto-spawn; instead mark task as `review` and store the opportunity artifact.
7. **Add tests** for `RankOpportunities` scoring using fixture JSON files.

### Phase 2 — Review surface and task creation

1. **Add `OpportunityReview` variant** to `TaskReviewSurface` and regenerate TS bindings (`./scripts/sync-bindings.sh`).
2. **Create frontend component** `src/components/review/OpportunityReview.tsx`.
3. **Add backend command** `create_tasks_from_opportunities` in `src-tauri/src/commands/seo_discovery.rs`.
4. **Implement `spawn_from_opportunities`** in `src-tauri/src/engine/exec/seo_discovery/spawn.rs`.
5. **Update `src/lib/tauri.ts`** with the new command wrapper.

### Phase 3 — CTR and Clarity integration

1. **Relax CTR fix spawning** as described in §4.5.
2. **Add `snippet_mismatch_likely`** flag in `RankOpportunities` when CTR + Clarity anomalies overlap.
3. **Update `feature-spec-generation` skill** to consume `seo_opportunities.json`.

### Phase 4 — Recurring scan (optional)

1. Add a project setting `seo_discovery_recurrence` (`weekly` | `monthly`).
2. Add a lightweight cron/scheduler in the backend that enqueues `seo_health_scan` based on the setting.

---

## 6. Files to Change

| File | Change |
|------|--------|
| `src-tauri/src/config/task_definitions.rs` | Add `seo_health_scan` definition |
| `src-tauri/src/engine/workflows/handlers.rs` | Add `SeoDiscoveryHandler` or extend existing handler |
| `src-tauri/src/engine/workflows/step_kinds.rs` (or wherever `StepKind` is defined) | Add `RankOpportunities`, `OpportunityReviewAgent` |
| `src-tauri/src/engine/exec/seo_discovery/rank.rs` | New: opportunity scoring |
| `src-tauri/src/engine/exec/seo_discovery/spawn.rs` | New: spawn child tasks from selected opportunities |
| `src-tauri/src/engine/post_actions.rs` | Mark `seo_health_scan` for review; pass opportunities to `generate_feature_spec` |
| `src-tauri/src/engine/exec/ctr_audit/task_spawner.rs` | Allow `clicks_lost` to trigger fix tasks |
| `src-tauri/src/db/mod.rs` | Add `seo_opportunities` table migration |
| `src-tauri/src/db/seo_discovery.rs` (new) | CRUD for opportunity table |
| `src-tauri/src/models/task.rs` | Add `OpportunityReview` to `TaskReviewSurface` |
| `src-tauri/src/commands/seo_discovery.rs` (new) | `create_tasks_from_opportunities` command |
| `src/lib/tauri.ts` | Wrap new command |
| `src/lib/types.ts` | Add opportunity type |
| `src/components/review/OpportunityReview.tsx` (new) | Review surface UI |
| `.github/skills/feature-spec-generation/SKILL.md` | Input contract update |

---

## 7. Acceptance Criteria

- [ ] Running `seo_health_scan` produces `seo_opportunities.json` with every article scored, ranked, and tagged with effort + recommended action.
- [ ] The top opportunity for a brewedlate-like cold-brew page is a `fix_ctr_article` task even when source-level health checks pass.
- [ ] Not-indexed pages appear in the opportunity list with `recommended_action = fix_indexing_internal_links` (or `fix_indexing`), not dropped.
- [ ] Cannibalized cluster pages appear with `recommended_action = consolidate_cluster` when no hub exists.
- [ ] Pages with both CTR opportunity and Clarity UX anomaly get a `snippet_mismatch_likely` flag.
- [ ] The `OpportunityReview` UI shows the ranked list and lets the user create tasks.
- [ ] Created tasks use `TaskSpawner::spawn` with proper idempotency keys and `DeduplicationPolicy::Cooldown { days: 30 }`.
- [ ] `cargo test` passes; `pnpm run check:ipc` passes; `pnpm exec tsc -b` passes.
- [ ] `generate_feature_spec` consumes `seo_opportunities.json` when available.

---

## 8. Out of Scope (for this spec)

- Automatic deployment / indexing request submission to Google.
- New LLM providers or changes to the Rig integration layer.
- Front-end redesign of the Overview screen (only the new review surface is in scope).
- Rewriting existing `content_review`, `ctr_audit`, `indexing_health_campaign`, or `cannibalization_audit` logic — we reuse them as data sources.

---

## 9. Notes for the Brewedlate Case

The cold-brew CTR opportunity maps directly to this spec:

- **Signal**: `avg_position` between 5–20, high impressions, actual CTR below `target_ctr_for_position`.
- **Current behaviour**: if title length, meta length, FAQ, and first paragraph pass the deterministic checks, no fix task is created.
- **New behaviour**: `clicks_lost >= 10.0` triggers a `fix_ctr_article` task with a prompt hint to optimize snippet competitiveness even though title/meta are technically valid.

The same logic applies to any page that ranks but does not earn clicks: the fix is not always a structural source problem; sometimes it is simply a weaker snippet than the competition.
