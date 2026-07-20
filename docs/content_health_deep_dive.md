# Content Health Deep Dive — Current Workflow vs. Proposed Loop

> This doc maps how PageSeeds currently surfaces content-audit findings, what we had to do manually for the Days to Expiry project, and how to close the gap so the next audit is actionable inside the app.

---

## 1. What currently exists

### 1.1 Backend audit pipeline

```
content_review / content_audit task
  ├─ GscSyncArticles          (optional deterministic step)
  ├─ ContentAudit             (deterministic 21-check audit → DB + JSON)
  ├─ ContentSync              (optional deterministic sync)
  └─ ContentReviewRecommend   (agentic → recommendations.json)
```

- Source: `src-tauri/src/engine/workflows/handlers.rs` (`ContentReviewHandler`)
- Audit logic: `src-tauri/src/engine/exec/content_audit.rs`
- DB persistence: `src-tauri/src/db/content_audit.rs`
- Recommendation logic: `src-tauri/src/engine/exec/content/review.rs`

After the parent task succeeds, `src-tauri/src/engine/post_actions.rs` calls `create_fix_content_article_tasks` (`src-tauri/src/engine/exec/content/task_spawner.rs`), which creates one `fix_content_article` task per recommended article.

### 1.2 Per-article fix pipeline

```
fix_content_article task
  ├─ fix_content_article_context     (deterministic: load recs + file)
  ├─ fix_content_article_generate    (agentic, skill = "content-fix-apply")
  ├─ fix_content_article_apply       (deterministic patch apply)
  └─ fix_content_article_verify      (deterministic re-audit checks)
```

- Source: `src-tauri/src/engine/workflows/handlers.rs` (`ImplementationHandler`)
- Steps: `src-tauri/src/engine/exec/content/fix_*.rs`
- Skill: `src-tauri/skills/content-fix-apply/SKILL.md` (embedded app default)

### 1.3 Frontend Health Dashboard

`src/components/health/HealthDashboard.tsx` already consumes `get_content_audit_report` and surfaces:

- Content health summary (`good / needs_improvement / poor`)
- A content score
- A few priority issue cards:
  - Title token duplication
  - Literal template variables
  - Temporal URLs
  - Page bloat
  - Exact duplicate content
- CTR, cannibalization, and indexing sections

It has a **Run Full Audit** button that calls `run_health_audit`, which spawns `content_review` + `indexing_health_campaign` tasks.

### 1.4 Existing commands

| Command | File | Purpose |
|---|---|---|
| `run_health_audit` | `src-tauri/src/commands/health.rs` | Spawns content_review + indexing_health_campaign |
| `get_content_audit_report` | `src-tauri/src/commands/health.rs` | Returns latest audit JSON from DB or legacy file |
| `get_indexing_health_summary` | `src-tauri/src/commands/health.rs` | Indexing stats from `gsc_url_indexing_status` |

---

## 2. What we still had to do manually

For Days to Expiry, the audit produced a 26 KB JSON report with 162 articles. The dashboard showed the high-level buckets, but to turn it into work we had to run ad-hoc scripts to find:

1. **Missing external links** — 95 of 97 poor/needs articles had 0 external links. Not surfaced as a priority issue.
2. **Keyword / H1 mismatch** — ~40 articles had target keywords that never appeared in the H1 or first 100 words. Not surfaced.
3. **Meta title/description length** — dozens of titles/descriptions were too short or too long. Not surfaced.
4. **Thin content** — 28 articles under 2,000 words. Not surfaced as a group.
5. **Duplicate target keywords** — 6 keyword phrases were assigned to multiple articles (cannibalization). Not surfaced.
6. **Temporal URL evergreening** — the dashboard flags temporal URLs, but does not suggest merging month-specific pages into a hub.
7. **Trend / diff** — no way to see which articles moved between runs without remembering previous numbers.
8. **Batch actions** — the only action on most issue cards is "View details"; there is no "Fix all 95 missing-external-links articles" button.

In short: the app does a great job **running** the audit and **fixing one article at a time**, but it does not yet **expose the patterns** or let the user **enqueue pattern-level fixes**.

---

## 3. Proposed enhancement: pattern-driven Content Health

The goal is to keep the existing pipeline almost intact and add a pattern-analysis layer on top of the audit result.

### 3.1 New data flow

```
content_review / content_audit task runs
  ↓
Audit result stored in DB (already happens)
  ↓
NEW: Pattern analyzer reads the latest run
  ↓
HealthDashboard displays patterns + affected articles
  ↓
User clicks "Fix pattern" → app creates task(s)
  ↓
Queue runs tasks → post_actions updates state
  ↓
User clicks "Re-audit" → dashboard refreshes with deltas
```

### 3.2 Patterns to surface

| Pattern | Detection | Severity | Fix mode |
|---|---|---|---|
| Missing external links | `quality_warnings` contains "Too few external links (0)" | High | Deterministic batch |
| Target keyword not in H1 | `quality_critical` / checks | High | Agentic per article |
| Target keyword not in first 100 words | `quality_critical` / checks | High | Agentic per article |
| Meta title too short | `quality_warnings` regex | Medium | Deterministic batch |
| Meta title too long | `quality_warnings` regex | Medium | Deterministic batch |
| Meta description too short | `quality_warnings` regex | Medium | Deterministic batch |
| Meta description too long | `quality_warnings` regex | Medium | Deterministic batch |
| Thin content (< 2000 words) | `word_count` + `quality_critical` | Medium | Agentic per article |
| Duplicate target keywords | GROUP BY `target_keyword` HAVING count > 1 | High | Review / retarget |
| Temporal URLs | `temporal_url == true` or slug regex | Medium | Review / merge |
| Exact duplicate body | `md5_body_hash` groups | Critical | Review / merge |
| Title token duplication | existing check | Critical | Agentic / manual |
| Literal template variables | existing check | Critical | Deterministic batch |

### 3.3 Pattern priority score

Each pattern instance should be sortable by impact:

```text
priority = (100 - health_score) * 10
         + (health == 'poor' ? 500 : 0)
         + log10(gsc_impressions + 1) * 50
         + pattern_weight
```

This lets the user attack the highest-ROI articles first rather than the alphabetical list.

---

## 4. Concrete implementation plan

### Phase A — Backend pattern analyzer (no UI yet)

1. **Add a pattern-analysis module**
   - New file: `src-tauri/src/engine/content_health/patterns.rs`
   - Struct `ContentPattern { name, severity, fix_mode, articles: Vec<PatternArticle>, priority_score }`
   - Function `analyze_patterns(conn, project_id, run_id) -> Vec<ContentPattern>`
   - Reads the latest `content_audit_runs` + `article_content_audits` rows.

2. **Add a command**
   - File: `src-tauri/src/commands/health.rs`
   - `get_content_health_patterns(project_id) -> Vec<ContentPattern>`
   - Wrapper that calls the analyzer and returns JSON.

3. **Add deterministic fix helpers**
   - `src-tauri/src/engine/content_health/fix_external_links.rs`
     - Input: article file path
     - Output: append 2–3 curated external links to the end of the article body
     - Use a hardcoded domain list + topic matching (CBOE, OCC, FINRA, IRS Pub 550, etc.)
   - `src-tauri/src/engine/content_health/fix_meta_length.rs`
     - Input: article file path
     - Output: rewrite title/description to hit length targets
     - This can be rule-based for simple cases, agentic for hard ones.

4. **Wire new task types (optional new types, or reuse `fix_content`)**
   - Option 1 (minimal): reuse `fix_content_article` with a new skill per pattern.
   - Option 2 (better): add deterministic task types that skip the LLM:
     - `fix_external_links`
     - `fix_meta_length`
   - For this proposal, **Option 1 is recommended** because it reuses the existing 4-step pipeline and verification.

### Phase B — Extend HealthDashboard with patterns

1. **Update `src/lib/tauri.ts`**
   - Add `getContentHealthPatterns(projectId)` wrapper.

2. **Update `src/lib/types.ts`**
   - Add `ContentPattern`, `PatternArticle` interfaces.

3. **Update `src/components/health/HealthDashboard.tsx`**
   - Fetch patterns in addition to the raw audit.
   - Add a **Patterns** section above or beside Priority Issues.
   - Each pattern card shows:
     - Pattern name
     - Affected article count
     - Average health score
     - Severity badge
     - **Fix pattern** button
   - Clicking a pattern opens a drill-down table with:
     - Article ID, title, health score, priority
     - Checkboxes to include/exclude
     - **Enqueue selected** button

4. **Add batch enqueue command**
   - `src-tauri/src/commands/health.rs`: `enqueue_content_pattern_fixes(project_id, pattern_name, article_ids)`
   - Creates one `fix_content_article` task per article with the appropriate skill param.
   - Skills to add:
     - `.github/skills/add-external-links/SKILL.md`
     - `.github/skills/rewrite-meta/SKILL.md`
     - `.github/skills/align-keyword-and-h1/SKILL.md`
     - `.github/skills/expand-content/SKILL.md`
     - `.github/skills/evergreen-temporal-pages/SKILL.md`

### Phase C — Trend / diff view

1. **Add backend helper**
   - `src-tauri/src/db/content_audit.rs`: `get_audit_run_history(project_id, limit) -> Vec<AuditRunSummary>`
   - Already have `content_audit_runs` table; just query it.

2. **Add frontend trend chart**
   - Reuse existing chart component or add a simple bar/line chart.
   - Show good / needs_improvement / poor counts over the last 5–10 runs.

3. **Add moved-articles list**
   - Compare current run to previous run per article.
   - Show "Moved to good", "Moved to poor", "New issues".

### Phase D — Re-audit close-the-loop button

1. **Frontend**
   - Add a **Re-run Content Audit** button in HealthDashboard.
   - It calls `run_health_audit` or a slimmer `content_audit`-only variant.
   - Polls queue status and refreshes patterns + audit when done.

2. **Backend**
   - Existing queue system already handles this; just enqueue `content_review`.

---

## 5. Recommended quick wins (start here)

The fastest path to value is to enhance the existing HealthDashboard with the missing patterns and use the existing `fix_content_article` pipeline.

### 5.1 Backend quick win

Add a single new command `get_content_health_patterns` that returns the 8–10 patterns above. No new task types, no new exec modules.

### 5.2 Frontend quick win

Add a **Patterns** section to `HealthDashboard` that:
- Lists patterns sorted by priority
- Shows count and avg health
- Has a **Fix all** button that creates `fix_content_article` tasks using the existing skill mechanism

### 5.3 Skill quick win

Create one new skill `.github/skills/add-external-links/SKILL.md`. This alone unlocks fixing the 95 Days to Expiry articles with missing external links.

---

## 6. Files to touch

| File | Change |
|---|---|
| `src-tauri/src/engine/content_health/patterns.rs` | NEW — pattern analyzer |
| `src-tauri/src/commands/health.rs` | ADD `get_content_health_patterns`, `enqueue_content_pattern_fixes` |
| `src/lib/tauri.ts` | ADD wrappers |
| `src/lib/types.ts` | ADD `ContentPattern`, `PatternArticle` |
| `src/components/health/HealthDashboard.tsx` | ADD patterns section + drill-down + enqueue buttons |
| `.github/skills/add-external-links/SKILL.md` | NEW skill |
| `.github/skills/rewrite-meta/SKILL.md` | NEW skill |
| `.github/skills/align-keyword-and-h1/SKILL.md` | NEW skill |
| `src-tauri/src/engine/workflows/handlers.rs` | Possibly map pattern skill param to `fix_content_article` |
| `src-tauri/src/engine/exec/content/fix_generate.rs` | Read skill from task params if overridden by pattern |

---

## 7. Acceptance criteria

- [ ] `get_content_health_patterns` returns at least the top 8 patterns for any project with a recent audit.
- [ ] HealthDashboard shows patterns with counts, severity, and avg health score.
- [ ] User can click **Fix pattern** and enqueue `fix_content_article` tasks for all affected articles.
- [ ] User can exclude individual articles from the batch.
- [ ] Re-audit button refreshes the dashboard and shows deltas.
- [ ] Trend chart shows last 5 runs' good/needs/poor counts.
- [ ] Deterministic-only patterns (external links, meta length) can optionally skip the LLM step.

---

## 8. Why this is the right scope

- **Reuses existing pipeline:** The `content_review` → `fix_content_article` flow already works. We are not rebuilding it.
- **Fits the architecture:** Pattern analysis is deterministic; fix tasks are agentic; the queue orchestrates everything.
- **Works for every project:** Once built, BrewedLate, Days to Expiry, and any future project get the same Content Health view.
- **Matches user mental model:** Users think "fix all the missing external links" not "open 95 articles one by one."
