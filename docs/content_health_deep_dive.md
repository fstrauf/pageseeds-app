# Content Health Deep Dive — Current Workflow vs. Proposed Loop

> **Status:** Archival design notes (desktop UI removed #184). Implementation targets are **CLI + domain APIs** only — do not invent a `commands/` module under core or rebuild desktop UI paths. Prefer `task_definitions.rs`, `engine/queue.rs`, `TaskSpawner`, and thin `pageseeds-cli` surfaces.
>
> This doc maps how PageSeeds surfaces content-audit findings, what we had to do manually for the Days to Expiry project, and how to close the gap so the next audit is actionable for operators.

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

- Source: `crates/pageseeds-core/src/engine/workflows/handlers.rs` (`ContentReviewHandler`)
- Audit logic: `crates/pageseeds-core/src/engine/exec/content_audit.rs`
- DB persistence: `crates/pageseeds-core/src/db/content_audit.rs`
- Recommendation logic: `crates/pageseeds-core/src/engine/exec/content/review.rs`

After the parent task succeeds, `crates/pageseeds-core/src/engine/post_actions.rs` calls `create_fix_content_article_tasks` (`crates/pageseeds-core/src/engine/exec/content/task_spawner.rs`), which creates one `fix_content_article` task per recommended article.

### 1.2 Per-article fix pipeline

```
fix_content_article task
  ├─ fix_content_article_context     (deterministic: load recs + file)
  ├─ fix_content_article_generate    (agentic, skill = "content-fix-apply")
  ├─ fix_content_article_apply       (deterministic patch apply)
  └─ fix_content_article_verify      (deterministic re-audit checks)
```

- Source: `crates/pageseeds-core/src/engine/workflows/handlers.rs` (`ImplementationHandler`)
- Steps: `crates/pageseeds-core/src/engine/exec/content/fix_*.rs`
- Skill: `crates/pageseeds-core/skills/content-fix-apply/SKILL.md` (embedded app default)

### 1.3 Operator surfaces (CLI / domain)

Desktop Health Dashboard was removed in **#184**. Operators work through:

- Desk reads + hard actions (weekly-seo skill / CLI Path B)
- Task types: `content_review`, `content_audit`, `fix_content_article`, `indexing_health_campaign`
- Domain APIs for audit reports and indexing summaries (via DB / export paths in `pageseeds-core`)
- Enqueue via `engine/queue.rs` or documented CLI task tools

Historical desktop IPC names (`run_health_audit`, `get_content_audit_report`, `get_indexing_health_summary`) mapped to domain behavior that still lives under `engine/`, `db/`, and task types — not under a `commands/` module.

---

## 2. What we still had to do manually

For Days to Expiry, the audit produced a 26 KB JSON report with 162 articles. High-level buckets were available, but to turn it into work we had to run ad-hoc scripts to find:

1. **Missing external links** — 95 of 97 poor/needs articles had 0 external links. Not surfaced as a priority issue.
2. **Keyword / H1 mismatch** — ~40 articles had target keywords that never appeared in the H1 or first 100 words. Not surfaced.
3. **Meta title/description length** — dozens of titles/descriptions were too short or too long. Not surfaced.
4. **Thin content** — 28 articles under 2,000 words. Not surfaced as a group.
5. **Duplicate target keywords** — 6 keyword phrases were assigned to multiple articles (cannibalization). Not surfaced.
6. **Temporal URL evergreening** — temporal URLs may be flagged, but merge-into-hub suggestions are not automated.
7. **Trend / diff** — no way to see which articles moved between runs without remembering previous numbers.
8. **Batch actions** — no first-class "fix all N missing-external-links articles" operator command.

In short: the product does a great job **running** the audit and **fixing one article at a time**, but it does not yet **expose the patterns** or let the operator **enqueue pattern-level fixes** via CLI.

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
CLI / desk surfaces patterns + affected articles
  ↓
Operator selects pattern or article IDs → enqueue task(s)
  ↓
Queue runs tasks → post_actions updates state
  ↓
Operator re-audits → patterns + deltas refresh
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

This lets the operator attack the highest-ROI articles first rather than the alphabetical list.

---

## 4. Concrete implementation plan

### Phase A — Domain pattern analyzer

1. **Add a pattern-analysis module**
   - New file: `crates/pageseeds-core/src/engine/content_health/patterns.rs` (or under `content/` / `db/` if more natural)
   - Struct `ContentPattern { name, severity, fix_mode, articles: Vec<PatternArticle>, priority_score }`
   - Function `analyze_patterns(conn, project_id, run_id) -> Vec<ContentPattern>`
   - Reads the latest `content_audit_runs` + `article_content_audits` rows.

2. **Expose via domain API + thin CLI (if operator-facing)**
   - Domain: `get_content_health_patterns(project_id) -> Vec<ContentPattern>`
   - CLI: thin subcommand that prints JSON — parse args → call core → print
   - Do **not** add `crates/pageseeds-core/src/commands/health.rs` (no commands layer after #184)

3. **Add deterministic fix helpers**
   - `crates/pageseeds-core/src/engine/content_health/fix_external_links.rs`
     - Input: article file path
     - Output: append 2–3 curated external links to the end of the article body
     - Use a hardcoded domain list + topic matching (CBOE, OCC, FINRA, IRS Pub 550, etc.)
   - `crates/pageseeds-core/src/engine/content_health/fix_meta_length.rs`
     - Input: article file path
     - Output: rewrite title/description to hit length targets
     - This can be rule-based for simple cases, agentic for hard ones.

4. **Wire new task types (optional new types, or reuse `fix_content`)**
   - Option 1 (minimal): reuse `fix_content_article` with a new skill per pattern.
   - Option 2 (better): add deterministic task types that skip the LLM:
     - `fix_external_links`
     - `fix_meta_length`
   - For this proposal, **Option 1 is recommended** because it reuses the existing 4-step pipeline and verification.

### Phase B — Operator pattern enqueue

1. **CLI list patterns** — print ranked patterns + affected article IDs as JSON.
2. **CLI / selection path to enqueue fixes**
   - Domain: `enqueue_content_pattern_fixes(project_id, pattern_name, article_ids)`
   - Creates one `fix_content_article` task per article with the appropriate skill param via `TaskSpawner`.
   - Enqueue through `engine/queue.rs`.
3. **Skills to add**
   - `.github/skills/add-external-links/SKILL.md`
   - `.github/skills/rewrite-meta/SKILL.md`
   - `.github/skills/align-keyword-and-h1/SKILL.md`
   - `.github/skills/expand-content/SKILL.md`
   - `.github/skills/evergreen-temporal-pages/SKILL.md`

### Phase C — Trend / diff

1. **Add backend helper**
   - `crates/pageseeds-core/src/db/content_audit.rs`: `get_audit_run_history(project_id, limit) -> Vec<AuditRunSummary>`
   - Already have `content_audit_runs` table; just query it.

2. **CLI trend summary**
   - Print good / needs_improvement / poor counts over the last 5–10 runs as JSON.

3. **Moved-articles list**
   - Compare current run to previous run per article.
   - Show "Moved to good", "Moved to poor", "New issues".

### Phase D — Re-audit close-the-loop

1. **Operator**
   - Enqueue `content_review` (or a slimmer `content_audit`-only variant) via CLI/queue.
2. **Backend**
   - Existing queue system already handles this; re-run pattern analysis after completion.

---

## 5. Recommended quick wins (start here)

The fastest path to value is domain pattern analysis + CLI enqueue of the existing `fix_content_article` pipeline.

### 5.1 Domain quick win

Add `get_content_health_patterns` in core that returns the 8–10 patterns above. No new task types, no new exec modules.

### 5.2 CLI quick win

Print patterns sorted by priority (count + avg health) and a **fix pattern** path that creates `fix_content_article` tasks using the existing skill mechanism.

### 5.3 Skill quick win

Create one new skill `.github/skills/add-external-links/SKILL.md`. This alone unlocks fixing the 95 Days to Expiry articles with missing external links.

---

## 6. Files to touch

| File | Change |
|---|---|
| `crates/pageseeds-core/src/engine/content_health/patterns.rs` | NEW — pattern analyzer |
| Domain API + thin CLI subcommand (not `commands/`) | ADD list patterns + enqueue pattern fixes |
| `.github/skills/add-external-links/SKILL.md` | NEW skill |
| `.github/skills/rewrite-meta/SKILL.md` | NEW skill |
| `.github/skills/align-keyword-and-h1/SKILL.md` | NEW skill |
| `crates/pageseeds-core/src/engine/workflows/handlers.rs` | Possibly map pattern skill param to `fix_content_article` |
| `crates/pageseeds-core/src/engine/exec/content/fix_generate.rs` | Read skill from task params if overridden by pattern |

---

## 7. Acceptance criteria

- [ ] `get_content_health_patterns` returns at least the top 8 patterns for any project with a recent audit.
- [ ] CLI (or desk-readable JSON) shows patterns with counts, severity, and avg health score.
- [ ] Operator can enqueue `fix_content_article` tasks for all affected articles of a pattern.
- [ ] Operator can exclude individual articles from the batch.
- [ ] Re-audit refreshes patterns and shows deltas.
- [ ] Trend summary shows last 5 runs' good/needs/poor counts.
- [ ] Deterministic-only patterns (external links, meta length) can optionally skip the LLM step.

---

## 8. Why this is the right scope

- **Reuses existing pipeline:** The `content_review` → `fix_content_article` flow already works. We are not rebuilding it.
- **Fits the architecture:** Pattern analysis is deterministic; fix tasks are agentic; the queue orchestrates everything.
- **Works for every project:** Once built, BrewedLate, Days to Expiry, and any future project get the same Content Health patterns.
- **Matches operator mental model:** Operators think "fix all the missing external links" not "open 95 articles one by one."
