# Feature Specification: Content Review Tracking & Deduplication

Generated: 2026-05-21
Status: Draft — Ready for Implementation

---

## Executive Summary

PageSeeds content review currently has a deduplication gap: articles can be re-fixed indefinitely because the system tracks "when was the last fix task completed" but not "has the file actually changed since then?" or "should we wait before re-fixing?". This leads to churn, wasted agent calls, and potential regression of previous fixes.

This spec outlines the tracking infrastructure needed to give PageSeeds (and the user) confidence that when content review says "done", it actually means done.

---

## P0 — Tracking Infrastructure (Backend)

### 1. Add `Cooldown` Policy to `fix_content_article` Tasks

**Problem:** `SkipIfActive` only blocks duplicate fix tasks while the previous one is running. Once it finishes (`done`), a new `content_review` run can immediately spawn the exact same fix again.

**Fix:** Change the deduplication policy from `SkipIfActive` to `Cooldown { days: 14 }`.

**File:** `src-tauri/src/engine/exec/content/task_spawner.rs` (in `create_apply_task`)

```rust
// BEFORE
dedup_policy: Some(DeduplicationPolicy::SkipIfActive)

// AFTER
dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 14 })
```

**Behavior:** If an article was fixed in the last 14 days, do not spawn a new fix task for it — regardless of health scores, regression signals, or anything else. After 14 days, it becomes eligible again.

**Rationale:** 14 days is long enough for Google to re-crawl and for GSC data to reflect changes, but short enough that genuinely new issues aren't ignored forever.

---

### 2. Add `content_hash` to Content Audit

**Problem:** Content review re-audits files even when they haven't changed since the last audit. The CTR audit subsystem already hashes file contents; content review does not.

**Fix:** Compute and store a content hash during `exec_content_audit`, then skip unchanged files on subsequent runs.

**Files:**
- `src-tauri/src/db/mod.rs` — add `content_hash TEXT` column to `articles` table (migration V17)
- `src-tauri/src/engine/exec/content_audit.rs` — compute MD5/SHA256 of body before auditing, store in output
- `src-tauri/src/engine/exec/content/review.rs` — in `select_priority_articles`, skip articles where `content_hash` matches previous audit and `last_reviewed_at` < 30 days

**Behavior:**
1. First audit: hash computed, stored, article audited
2. Second audit (file unchanged): hash matches, article skipped unless > 30 days old
3. Second audit (file changed): hash mismatch, article re-audited

---

### 3. Add `last_edited_at` to Articles Table

**Problem:** `last_reviewed_at` tracks when the fix *task finished*, not when the *file was actually modified*. These are different: a fix task might finish without making changes (nothing to fix), or it might make substantial changes.

**Fix:** Add `last_edited_at` to the `articles` table and update it whenever `fix_content_article_apply` actually modifies the MDX file.

**Files:**
- `src-tauri/src/db/mod.rs` — add `last_edited_at TEXT` column (migration V17)
- `src-tauri/src/engine/exec/content/fix_apply.rs` — after successful write, update `last_edited_at`
- `src-tauri/src/engine/exec/content/review.rs` — use `last_edited_at` (not just `last_reviewed_at`) in revisit logic

**Behavior:**
- `last_reviewed_at` = when fix task finished (existing)
- `last_edited_at` = when file was actually modified (new)
- Revisit logic checks: if `last_edited_at` is recent AND file is healthy, skip

---

### 4. Persistent Recommendations History

**Problem:** `recommendations.json` is overwritten on every `content_review` run. There is no audit trail of what was fixed when, making it impossible to answer "what did we actually change last time?"

**Fix:** Append recommendations to `recommendations_history.jsonl` instead of overwriting.

**File:** `src-tauri/src/engine/exec/content/review.rs`

**Behavior:**
```jsonl
{"run_id": "task-abc", "timestamp": "2026-05-01T10:00:00Z", "article_id": 149, "suggestions": [...]}
{"run_id": "task-abc", "timestamp": "2026-05-01T10:00:00Z", "article_id": 7, "suggestions": [...]}
{"run_id": "task-def", "timestamp": "2026-05-15T14:00:00Z", "article_id": 149, "suggestions": [...]}
```

This gives us a full history per article, searchable by run ID or date.

---

## P0 — Architectural Clarification (Scope)

### 5. Feature Spec Goes to Target Repo, Not PageSeeds Repo

**Rule:** The `generate_feature_spec` task writes `.github/automation/seo_feature_spec.md` in the **target repo** (the user's site repo). It is a document for developers to act on. PageSeeds does not attempt to modify framework files (`layout.tsx`, `_app.js`, etc.) in the target repo.

**Already implemented correctly:** ✅ The `generate_feature_spec` exec function writes to `ProjectPaths::from_path(project_path).automation_dir`, which resolves to the target repo.

**No action needed.** This is documented here for clarity.

---

## P1 — UI Integration (Frontend)

### 6. Backend Command: `get_feature_spec`

**Problem:** No way for the frontend to read the generated spec from the target repo.

**Fix:** Add a thin Tauri command.

**File:** `src-tauri/src/commands/health.rs`

```rust
#[tauri::command]
pub fn get_feature_spec(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<String, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&conn, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    let spec_path = repo_root.join(".github").join("automation").join("seo_feature_spec.md");
    
    if !spec_path.exists() {
        return Err("No feature spec found. Run an audit to generate one.".to_string());
    }
    
    std::fs::read_to_string(&spec_path)
        .map_err(|e| format!("Failed to read feature spec: {}", e))
}
```

**Register:** Add to `commands/mod.rs` and `lib.rs` handler list.

**Tauri.ts wrapper:**
```typescript
export const getFeatureSpec = (projectId: string): Promise<string> =>
  invoke('get_feature_spec', { projectId })
```

---

### 7. HealthDashboard Spec Card

**Problem:** `HealthDashboard` loads 4 data sources but not the feature spec.

**Fix:** Add a "Developer Feature Spec" card to `HealthDashboard.tsx`.

**Behavior:**
- On mount: call `getFeatureSpec(project.id)`
- If spec exists: show card with:
  - Generation date (parsed from markdown)
  - P0 count, P1 count, P2 count (parsed from headings)
  - "View Full Spec" button → opens markdown modal
- If no spec: show "Run Full Audit to generate a feature spec"

**File:** `src/components/health/HealthDashboard.tsx`

---

### 8. TaskDetail Artifact Preview

**Problem:** `TaskDetail` shows artifact metadata (key, path, source) but not the content.

**Fix:** For tasks with `review_surface == ArtifactReview`, read and display the artifact content.

**Behavior:**
- Detect `generate_feature_spec` tasks
- Call `getFeatureSpec(projectId)` to fetch the markdown
- Render in a scrollable `<div>` with simple styling
- Show P0/P1/P2 sections as collapsible

**File:** `src/components/tasks/TaskDetail.tsx`

---

### 9. Overview "Latest Spec" Callout

**Problem:** Overview has no indication that a feature spec was generated.

**Fix:** Add a small callout card in Overview when a recent spec exists.

**Behavior:**
- Call `getFeatureSpec` on mount
- If spec exists and is < 7 days old: show banner
  - "Feature spec generated on {date} — {n} code fixes identified"
  - Link to HealthDashboard or open modal
- If no spec or > 30 days old: show nothing (or "Run Full Audit" prompt)

**File:** `src/components/overview/Overview.tsx`

---

## P2 — Quality Improvements

### 10. Title Duplication Threshold Fix

**Problem:** `title_token_duplication` only fires when a token appears ≥ 3 times. "Days to Expiry | Days to Expiry" has each token appearing only 2× — not caught.

**Fix:** Lower threshold from ≥ 3 to ≥ 2.

**File:** `src-tauri/src/engine/exec/content_audit.rs`

```rust
// BEFORE
let title_token_duplication = max_token_count >= 3;

// AFTER
let title_token_duplication = max_token_count >= 2;
```

**Weight adjustment:** Reduce weight from 10 to 5 (2× duplication is less severe than 3×).

---

### 11. Rendered vs Frontmatter Title Diff Check

**Problem:** The CTR rendered audit detects missing dynamic titles, but there is no dedicated deterministic step that compares rendered `<title>` against frontmatter `title:` for every article.

**Fix:** After `ctr_rendered_serp_audit` runs, add a deterministic post-step that:
1. Reads `ctr_rendered_page_audits` table
2. For each article, compares `rendered_title` vs `frontmatter_title`
3. Flags cases where they are completely different (not just template-appended)
4. Writes a dedicated artifact: `title_mismatch_report.json`

**File:** New exec function in `src-tauri/src/engine/exec/ctr_audit/rendered.rs` or as a post-action

---

## P2 — Data Retention

### 12. Article Audit State for Content Review

**Problem:** The `article_audit_state` table exists but is only used by CTR audit. Content review should use it too.

**Fix:** Store content audit results in `article_audit_state` with:
- `content_hash`
- `health_score`
- `checks_failed`
- `quality_score`
- `audit_timestamp`

**File:** `src-tauri/src/engine/exec/content_audit.rs` — insert/update rows after audit

---

## Issue Matrix

| # | Issue | Priority | Type | Status |
|---|-------|----------|------|--------|
| 1 | Add `Cooldown` to fix tasks | P0 | Backend | Not started |
| 2 | Add `content_hash` to content audit | P0 | Backend | Not started |
| 3 | Add `last_edited_at` to articles | P0 | Backend | Not started |
| 4 | Persistent recommendations history | P0 | Backend | Not started |
| 5 | Feature spec scope clarification | P0 | Architecture | ✅ Correct |
| 6 | Backend command `get_feature_spec` | P1 | Backend | Not started |
| 7 | HealthDashboard spec card | P1 | Frontend | Not started |
| 8 | TaskDetail artifact preview | P1 | Frontend | Not started |
| 9 | Overview spec callout | P1 | Frontend | Not started |
| 10 | Title duplication threshold | P2 | Backend | Not started |
| 11 | Rendered vs frontmatter diff | P2 | Backend | Not started |
| 12 | Article audit state for content | P2 | Backend | Not started |

---

## Implementation Order

**Phase 1 (Immediate):**
1. Cooldown policy on fix tasks — one line, prevents churn immediately
2. Backend command `get_feature_spec` — unblocks all UI work

**Phase 2 (This Week):**
3. `content_hash` in content audit — prevents re-auditing unchanged files
4. `last_edited_at` tracking — distinguishes "reviewed" from "edited"
5. HealthDashboard spec card — makes spec visible

**Phase 3 (Next Week):**
6. Persistent recommendations history — audit trail
7. TaskDetail artifact preview — full spec viewing
8. Overview spec callout — user awareness

**Phase 4 (Backlog):**
9. Title duplication threshold
10. Rendered vs frontmatter diff
11. Article audit state for content review
