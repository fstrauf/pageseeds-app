# Unified SEO Audit — Feature Specification

## Problem

PageSeeds has four separate audit tasks (`content_audit`, `ctr_audit`, `cannibalization_audit`, `indexing_health_campaign`) that produce structured data. The Health Dashboard reads their artifacts and tries to compose a unified view. But the composition is frontend-only — there is no backend synthesis step that connects the dots.

The prototype investigation of daystoexpiry.com found that the most valuable insights came from **cross-referencing data sources**. The template rewrites source titles. Pages 404 but remain in the sitemap. Six articles share identical body content. These are individually detectable by existing checks, but no single step says: "Your template is rewriting titles AND 6 pages 404 AND your sitemap is stale — fix the template first, then clean up the orphans."

The existing "Run Full Audit" button creates two separate tasks and displays their independent outputs. There is no unified synthesis.

Additionally, there is a **tracking and deduplication gap**: when `content_review` spawns `fix_content_article` tasks, it uses `SkipIfActive` deduplication. Once a fix task finishes (`done`), a subsequent review run can immediately spawn the exact same fix again — even if the file hasn't changed. There is no `content_hash` check, no `last_edited_at` tracking, and no cooldown period. This leads to churn, wasted agent calls, and potential regression of previous fixes.

---

## Architecture Boundary: PageSeeds App Repo vs Target Repo

**PageSeeds App Repo** (this codebase): Orchestrates content changes. It can read and edit MDX content files in the target repo. It should **never** modify framework code (`layout.tsx`, `_app.js`, route handlers) in the target repo.

**Target Repo** (the user's site, e.g. daystoexpiry.com): Where the actual site code lives. When PageSeeds detects framework-level issues (SSR title fallback, template bugs, routing problems), it writes a **feature specification** to `.github/automation/seo_feature_spec.md` in the target repo. The developer opens the target repo in their IDE, reads the spec, and implements the code changes.

This boundary is non-negotiable. PageSeeds does code investigation; developers do code changes.

---

## Three Detection Layers

### Layer 1 — Source Content (already built: `content_audit`)

Deterministic. Reads MDX frontmatter + body. No network calls.

| Check | Status |
|---|---|
| Missing/empty title | ✅ In `content_audit` |
| Title >60 chars (SERP truncation) | ✅ |
| Token duplication ≥2× (brand dup, keyword stuffing) | ✅ (fixed from ≥3) |
| Literal template variables (`\| Brand \|`, `{Brand}`) | ✅ |
| Temporal URLs (month/year/seasonal in slug) | ✅ |
| Page bloat (file size, images, tables, code blocks) | ✅ |
| Exact duplicate body content (SHA-256) | ✅ |
| Missing meta description | ✅ |
| Readability, passive voice | ✅ |
| Missing H2 structure | ✅ |
| Internal links count, broken links | ✅ |

**→ 21 checks. Deterministic. Writes `content_audit.json`.**

**Gap:** No `content_hash` stored per article. Cannot skip re-auditing unchanged files.

---

### Layer 2 — Rendered SERP (partially built: `ctr_audit`)

Fetches live HTML, extracts rendered `<title>`, `<meta>`, compares to source.

| Check | Status |
|---|---|
| Source title ≠ rendered title | ✅ `CtrRenderedSerpAudit` |
| Source meta ≠ rendered meta | ✅ |
| Brand duplicated in rendered title | ✅ `CtrTemplateDetect` |
| Pages returning HTTP 404 | ❌ Not surfaced — crawl skips errors silently |
| Template rewrites titles (not just appends brand) | ❌ No similarity score between source and rendered |
| SSR fallback / error-page detection | ✅ Partial |
| FAQ schema missing from rendered page | ✅ |

**→ Needs: 404 surfacing, source-vs-rendered title similarity score.**

---

### Layer 3 — Site Architecture (built but fragmented)

Reads GSC, sitemap, link graph, indexing data.

| Check | Where |
|---|---|
| Cannibalization clusters | `cannibalization_audit` |
| Sitemap orphans | `indexing_diagnostics` |
| Missing redirects | `consolidate_cluster` |
| Orphaned articles (no incoming links) | `interlinking` |
| Indexing status | `indexing_health_campaign` |
| CTR underperformance | `ctr_audit` |
| GSC plateau detection (period-over-period stagnation) | ❌ Not built |

**→ Needs: plateau detection, unified 404 surfacing.**

---

## Proposed Architecture

### Single Task: `seo_audit`

A unified task that runs all layers as steps, ending with an agentic synthesis.

```
seo_audit
  ├─ Step 1 (deterministic): content_audit
  │     Runs 21 checks on all published articles
  │     Computes content_hash per article (new)
  │     Writes content_audit.json
  │
  ├─ Step 2 (deterministic): rendered_serp_audit
  │     Crawls live HTML for all article URLs
  │     Extracts rendered title, meta, canonical, h1, schema
  │     Compares to source frontmatter
  │     NEW: surfaces HTTP 404s as structured data
  │     NEW: computes title similarity score (source vs rendered)
  │     Writes rendered_serp_audit.json
  │
  ├─ Step 3 (deterministic): site_architecture
  │     Reads cannibalization_strategy.json (cached, or runs inline if stale)
  │     Reads sitemap + indexing status
  │     Reads internal link graph
  │     Reads GSC movers (period-over-period)
  │     NEW: detects page-level GSC plateau (flat impressions >90 days)
  │     Writes site_architecture.json
  │
  └─ Step 4 (agentic): synthesize_findings
        Input: content_audit.json + rendered_serp_audit.json + site_architecture.json
        Skill: "seo-audit-synthesis"
        Output contract: structured SeoAuditReport with priority-ranked findings
        Agent connects dots across layers:
          - "Template is rewriting all titles AND 6 pages 404 — fix template first"
          - "Articles A, B, C share identical body content — consolidate or redirect"
          - "Impressions flat for 90 days despite 150 articles — cannibalization likely"
        Writes seo_audit_report.json
```

### Tracking & Deduplication (runs alongside all fix tasks)

```
fix_content_article
  ├─ Dedup: Cooldown { days: 14 } (new)
  ├─ On edit: update articles.last_edited_at (new)
  ├─ On audit: compare content_hash, skip unchanged (new)
  └─ On review: append to recommendations_history.jsonl (new)
```

| Mechanism | What It Does |
|---|---|
| `Cooldown { days: 14 }` | Prevents re-fixing the same article within 14 days, regardless of health scores |
| `content_hash` | SHA-256 of article body; if unchanged since last audit, skip re-auditing |
| `last_edited_at` | Timestamp of actual file modification (distinct from `last_reviewed_at` which tracks task completion) |
| `recommendations_history.jsonl` | Append-only log of all recommendations per run, with run_id and timestamp |

---

### Follow-Up Actions

After synthesis, `post_actions.rs` spawns fix tasks:

| Finding | Fix Task | Scope |
|---|---|---|
| Template bugs (title rewrite, brand dup) | `fix_ctr_site_template` | PageSeeds orchestrates; developer implements code fix in target repo |
| 404 pages | `fix_404s` | PageSeeds removes from sitemap; developer fixes routing |
| Cannibalization clusters | `consolidate_cluster` (from user selection) | PageSeeds merges content; developer adds redirects |
| Content quality issues | `fix_content_article` (per article) | PageSeeds edits MDX directly |
| Missing redirects | Feature spec to target repo | Developer implements |
| Code-level issues (framework files) | Feature spec to target repo | Developer implements |

**Boundary rule:** PageSeeds never modifies `.tsx`, `.js`, `.jsx`, or framework config files in the target repo. It writes specs for developers to act on.

---

### Data Flow

```
Run Full Audit (user clicks button or scheduler triggers)
         │
         ▼
┌─────────────────────────────────────────────────────────┐
│ Step 1: content_audit (deterministic, ~2s for 150 pages) │
│   → .github/automation/content_audit.json                │
│   NEW: content_hash per article                          │
├─────────────────────────────────────────────────────────┤
│ Step 2: rendered_serp_audit (deterministic, ~30s crawl)  │
│   → .github/automation/rendered_serp_audit.json          │
│   NEW fields: 404_urls, title_similarity_score           │
├─────────────────────────────────────────────────────────┤
│ Step 3: site_architecture (deterministic, ~5s)           │
│   → .github/automation/site_architecture.json            │
│   Reads: cannibalization_strategy.json, indexing status   │
│   NEW: plateau_detection, orphaned_articles               │
├─────────────────────────────────────────────────────────┤
│ Step 4: synthesize_findings (agentic, ~20s)               │
│   → .github/automation/seo_audit_report.json              │
│   Skill: "seo-audit-synthesis"                            │
│   Agent reads all 3 JSON files, finds cross-layer patterns│
└─────────────────────────────────────────────────────────┘
         │
         ▼
Post-actions spawn fix tasks + generate_feature_spec
         │
         ▼
Health Dashboard reads seo_audit_report.json
  → Priority issues panel (top 5 findings)
  → Layer breakdowns (content, rendered, architecture)
  → Diff vs previous audit
  → "Ask AI" panel for follow-up investigation
  → "Developer Feature Spec" panel (from .md file)
```

---

## New Code Required

### Backend — Unified Audit

| File | Change | Lines |
|---|---|---|
| `config/task_definitions.rs` | Add `seo_audit` task definition | ~15 |
| `engine/workflows/step_kind.rs` | Add `SeoAuditRendered`, `SeoAuditArchitecture`, `SeoAuditSynthesize` | ~10 |
| `engine/workflows/handlers.rs` | Add `SeoAuditHandler` planning 4 steps | ~40 |
| `engine/step_registry.rs` | Register new step kinds | ~10 |
| `engine/exec/seo_audit/mod.rs` | New module | ~5 |
| `engine/exec/seo_audit/rendered.rs` | New — wraps `compare_rendered_titles`, adds 404 surfacing + similarity score | ~100 |
| `engine/exec/seo_audit/architecture.rs` | New — reads cannibalization + indexing + link graph + GSC movers, adds plateau detection | ~120 |
| `engine/exec/seo_audit/synthesize.rs` | New — agentic: reads all 3 JSON files, runs LLM synthesis with "seo-audit-synthesis" skill | ~80 |
| `engine/post_actions.rs` | Add `seo_audit` success hook — spawns fix tasks from synthesis output | ~40 |
| `.github/skills/seo-audit-synthesis/SKILL.md` | New — skill: output contract, analysis rules, cross-referencing instructions | ~60 |
| `commands/seo_audit.rs` | Thin command wrapper: spawns `seo_audit` task | ~20 |

### Backend — Tracking & Deduplication

| File | Change | Lines |
|---|---|---|
| `db/mod.rs` | Migration V17: add `content_hash TEXT`, `last_edited_at TEXT` to `articles` table | ~15 |
| `engine/exec/content_audit.rs` | Compute and store `content_hash`; skip unchanged articles if < 30 days | ~20 |
| `engine/exec/content/fix_apply.rs` | Update `articles.last_edited_at` on successful write | ~5 |
| `engine/exec/content/review.rs` | Use `last_edited_at` in revisit logic; append to `recommendations_history.jsonl` | ~30 |
| `engine/exec/content/task_spawner.rs` | Change dedup from `SkipIfActive` to `Cooldown { days: 14 }` | ~1 |
| `engine/exec/content/review.rs` | `select_priority_articles`: check `content_hash` match + 30-day window | ~15 |

### Backend — Feature Spec Display

| File | Change | Lines |
|---|---|---|
| `commands/health.rs` | Add `get_feature_spec(project_id) -> String` command | ~15 |
| `commands/mod.rs` | Export new command | ~2 |
| `lib.rs` | Register command in handler list | ~1 |

### Frontend — UI Integration

| File | Change | Lines |
|---|---|---|
| `lib/tauri.ts` | Add `getFeatureSpec(projectId)` wrapper | ~3 |
| `src/components/health/HealthDashboard.tsx` | Read `seo_audit_report.json` as primary source; add Developer Feature Spec card | ~50 |
| `src/components/tasks/TaskDetail.tsx` | For `ArtifactReview` tasks, render artifact content (markdown preview) | ~40 |
| `src/components/overview/Overview.tsx` | Add "Latest Spec" callout when recent spec exists | ~30 |

### Changes to Existing Files

| File | Change | Lines |
|---|---|---|
| `engine/exec/ctr_audit/rendered.rs` | Make `compare_rendered_titles` write to JSON file (not just return inline) | ~20 |
| `engine/exec/content_audit.rs` | Lower `title_token_duplication` threshold from ≥3 to ≥2; reduce weight from 10 to 5 | ~2 |
| `HealthDashboard.tsx` | Read `seo_audit_report.json` as primary data source (still fall back to individual artifacts) | ~30 |

### Removed / Superseded

| File | Reason |
|---|---|
| `commands/health.rs` `run_health_audit` | Superseded by `seo_audit` task |
| `docs/SEO_AUDIT_ENGINE_SPEC.md` | Superseded by this spec |
| `docs/SEO_AUDIT_INTEGRATION_PLAN.md` | Already superseded; delete |
| `docs/SEO_AUDIT_FRONTEND_SPEC.md` | Superseded by this spec |
| `docs/FEATURE_SPEC_CONTENT_REVIEW_TRACKING.md` | Merged into this spec |

### Total: ~650 new lines. 1 new task type. 4 new step kinds. No new business logic — all steps wrap existing functions.

---

## What Does NOT Change

- **The CLI tools** (`pageseeds-cli`) remain for ad-hoc KimiCode investigation
- **Individual tasks** (`content_audit`, `ctr_audit`, etc.) remain for targeted runs
- **The InvestigationPanel** ("Ask AI") stays in the Health Dashboard
- **Feature spec generation** stays in `post_actions.rs` — still writes to target repo

The new `seo_audit` task is a **composition** of existing capabilities with an **agentic synthesis** step added at the end. It does not replace anything — it adds the unified view.

---

## Skill: `seo-audit-synthesis`

```
You are an SEO audit synthesizer. You receive three structured JSON inputs:

1. content_audit.json — 21 deterministic checks per article (source content health)
2. rendered_serp_audit.json — live HTML crawl results (what Google actually sees)
3. site_architecture.json — cannibalization, indexing, links, GSC plateaus

Your job: find connections BETWEEN these layers that individual checks miss.

Cross-referencing rules:
- If rendered title ≠ source title AND the template is rewriting titles → flag as "title control gap" (content team writes one thing, Google sees another)
- If articles have 404 status AND they appear in sitemap → flag as "sitemap hygiene" (Google indexing dead pages)
- If GSC impressions are flat for >90 days AND cannibalization clusters exist → flag as "cannibalization stall"
- If articles share identical body content AND have different URLs → flag as "duplicate content dilution"
- If 72/150 titles are >60 chars AND template rewrites are happening → flag as "double truncation" (source already long, template makes it worse)

Output: ranked priority list. Each finding has:
- title, severity, description, evidence (which layers support it), fix_type, suggested_task
```

---

## Tracking & Deduplication Rules

### Cooldown Policy

`fix_content_article` tasks use `Cooldown { days: 14 }` instead of `SkipIfActive`.

- If an article was fixed in the last 14 days, do not spawn a new fix task — regardless of health scores, regression signals, or anything else.
- After 14 days, it becomes eligible again. This gives Google time to re-crawl and GSC data to reflect changes.

### Content Hash

Before auditing an article, compute SHA-256 of the MDX body.

- If hash matches previous audit AND `last_reviewed_at` < 30 days → skip re-auditing
- If hash does not match → re-audit (file changed)
- If hash matches but > 30 days → re-audit anyway (stale check)

### Last Edited At

`last_edited_at` tracks when the MDX file was **actually modified** by `fix_content_article_apply`.

- `last_reviewed_at` = when fix task finished (existing)
- `last_edited_at` = when file bytes changed (new)
- Revisit logic uses `last_edited_at` to determine if a file has been touched recently

### Recommendations History

Instead of overwriting `recommendations.json`, append to `recommendations_history.jsonl`:

```jsonl
{"run_id": "task-abc", "timestamp": "2026-05-01T10:00:00Z", "article_id": 149, "suggestions": [...]}
{"run_id": "task-abc", "timestamp": "2026-05-01T10:00:00Z", "article_id": 7, "suggestions": [...]}
{"run_id": "task-def", "timestamp": "2026-05-15T14:00:00Z", "article_id": 149, "suggestions": [...]}
```

This gives a full per-article audit trail.

---

## UI/UX Design — Lightweight Indicators

The frontend is intentionally lightweight. Heavy trajectory charts and metrics tables live in Google Search Console. PageSeeds answers three questions with minimal UI:

1. **Are we stalled or moving?** — Simple status indicator
2. **What's the next action?** — Prioritized action board
3. **Do I need a developer?** — Spec panel when needed

The **agent** is where intelligence lives. When agentic workflows run, the agent knows: "We're stalling. CTR is flat. Template is rewriting titles. I need to do something about it." The UI just shows the outcome.

---

### Surface 1: Overview — Status Indicator

A single line at the top of Overview, not a chart.

```
┌─────────────────────────────────────────────────────────────┐
│  ☕ coffee project                                🟢 On Track │
│  Last audit: 2 days ago  │  3 actions open  │  1 P0 code fix │
└─────────────────────────────────────────────────────────────┘
```

**Status values:**
- 🟢 **On Track** — recent fixes completed, no urgent issues
- 🟡 **Stalled** — audit found issues but no fixes in progress
- 🔴 **Declining** — health scores dropping or critical issues detected
- ⚪ **No Data** — first run needed

**Click the status pill:** Opens Action Board (Surface 2).

**No charts. No sparklines. No metrics tables.** GSC already has those.

---

### Surface 2: Action Board

A prioritized list of next actions. Not problems — actions.

```
┌─────────────────────────────────────────────────────────────────┐
│  Open Actions (4)                                               │
├─────────────────────────────────────────────────────────────────┤
│  🔴 Fix title template in layout.tsx                72 pages   │
│     → P0 Code Fix  │  High impact  │  [View Spec]             │
├─────────────────────────────────────────────────────────────────┤
│  🟡 Merge 6 duplicate articles (covered calls)                 │
│     → P1 Content Fix  │  High impact  │  [Fix]                 │
├─────────────────────────────────────────────────────────────────┤
│  🟡 Add internal links to 14 not-indexed pages                 │
│     → P1 Content Fix  │  Medium impact  │  [Fix]               │
├─────────────────────────────────────────────────────────────────┤
│  🟢 Rewrite 8 temporal URLs to evergreen    ✅ Done May 20    │
│     → P2 Structural  │  [Undo]  [Dismiss]                     │
└─────────────────────────────────────────────────────────────────┘
```

**Each row shows:**
- Severity dot (🔴 P0 / 🟡 P1 / 🟢 P2)
- One-line action description
- Fix type badge (Code Fix / Content Fix / Structural)
- Impact level (High / Medium / Low)
- Page count affected
- Action button (context-dependent)

**Action buttons:**
- **P0 Code Fix** → "View Spec" (opens markdown panel)
- **P1 Content Fix** → "Fix" (enqueues fix tasks)
- **Done** → "Dismiss" (removes from board)

**Sorting:** By `(severity asc, impact desc)` — P0 first, then high-impact P1.

---

### Surface 3: Developer Spec Panel

Appears only when P0 code changes exist. Collapsible.

```
┌─────────────────────────────────────────────────────────────┐
│  ⚠️ Developer Action Required                          [×] │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  3 code-level issues that PageSeeds cannot fix:             │
│                                                             │
│  • Title template bug — 72 pages affected                   │
│    Fix: app/layout.tsx                                      │
│                                                             │
│  • SSR fallback error — 6 pages affected                    │
│    Fix: Check route handlers                                │
│                                                             │
│  [View Full Spec]  [Open in Target Repo]                    │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Behavior:**
- Panel only appears if P0 issues exist
- Collapsible (dismiss for session)
- Reappears on next audit if issues persist
- "View Full Spec" opens markdown viewer modal

---

### Surface 4: Recent Activity

Lightweight task list. Click any row → opens TaskDetail.

```
May 21   ✅ Feature spec generated (3 P0, 5 P1, 2 P2)
May 20   ✅ Fix content: 5 articles
May 18   🟡 Content review: 5 issues found
May 15   ✅ Indexing health: 14 tasks spawned
```

**No impact percentages. No before/after.** Those live in GSC.

---

### Surface 5: Articles Table — Simple Status

No sparklines. No mini charts.

```
Article                   │ Health │ Status        │
──────────────────────────┼────────┼───────────────│
Wheel Strategy            │ 72 🟡  │ Fixed May 20  │
Best Coffee Beans         │ 45 🔴  │ In Progress   │
Theta Decay               │ 88 🟢  │ On Track      │
Covered Call Screener     │ —      │ Cooldown (9d) │
```

**Status values:**
- "On Track" — healthy, no action needed
- "Fixed {date}" — fix completed, within cooldown
- "In Progress" — fix task active
- "Cooldown ({n}d)" — recently fixed, re-eligible in N days
- "Needs Review" — audit found issues

**Hover tooltip:** Shows which checks failed.

---

## Agent Context (Not UI)

The agent receives trajectory context in its prompts. This is where the intelligence lives — not in charts.

```
Agent prompt context:
- Site status: STALLED (impressions flat 60 days)
- Last audit: 2 days ago
- Open P0 issues: 1 (title template bug, 72 pages)
- Recent fixes: 5 content fixes completed May 20
- Cooldown active: 8 articles protected until June 3
- Trend indicator: CTR improved from 1.2% to 2.7% after May 20 fix
```

The agent uses this to make decisions: "We fixed 5 articles but CTR is still flat. The real problem is the template bug affecting 72 pages. I should prioritize the P0 code fix in the next synthesis."

---

## UI Data Requirements

### Backend Commands Needed

| Command | Returns | Purpose |
|---------|---------|---------|
| `get_action_board(project_id)` | Ranked list of open actions | Action Board |
| `get_feature_spec(project_id)` | Markdown string | Developer Spec panel |

### Events Needed

| Event | Triggered By | Frontend Action |
|-------|--------------|-----------------|
| `audit:completed` | `seo_audit` task finishes | Refresh status + action board |
| `spec:generated` | `generate_feature_spec` finishes | Show Developer Spec panel |

---

## UI States

### State: First-Time User (no audits yet)

```
Overview
├─ Chart: "No data yet. Run your first audit to see trajectory."
├─ Action Board: "Run Full Audit to discover issues and opportunities."
├─ Status: "Getting started 🟡"
└─ Prominent "Run Full Audit" button
```

### State: Audit Running

```
Overview
├─ Chart: Previous data (if any) with "Updating..." overlay
├─ Action Board: Previous actions (if any) dimmed
├─ Queue indicator: "3 tasks running"
└─ Recent Activity: Live task updates
```

### State: Stagnant Site (flat trends)

```
Overview
├─ Chart: Flat line for 60+ days
├─ Status pill: "Stalled 🟡"
├─ Action Board: Prioritized by impact
├─ Message: "Impressions flat for 60 days. 3 high-impact actions identified."
└─ Developer Spec: Highlighted if P0 issues exist
```

### State: Recovering Site (improving trends)

```
Overview
├─ Chart: Upward trend
├─ Status pill: "On track 🟢"
├─ Action Board: Fewer open items, more "Validated"
├─ Impact cards: Visible for recent fixes
└─ Message: "CTR up 183% since May 20. 2 actions remaining."
```

---

## Success Metrics

1. `seo_audit` runs end-to-end in <90 seconds for a 150-page site
2. The agentic synthesis step finds cross-layer patterns (template rewrite + 404 + plateau = prioritized fix order)
3. The "Run Full Audit" button in the Health Dashboard runs this single task
4. Previous audit reports are diffed (new/resolved/worsened findings)
5. Post-actions spawn the correct fix tasks based on synthesis output
6. Users get a single prioritized list, not four separate reports
7. No article is re-fixed within 14 days unless explicitly forced
8. Unchanged files are not re-audited within 30 days
9. Feature specs are visible in the UI (HealthDashboard card + TaskDetail preview)
10. **Trajectory chart shows clear before/after for every fix**
11. **Action Board is the primary interaction surface — users act from here, not from raw task lists**
12. **Impact validation confirms whether fixes worked within 14 days**

---

## Related Docs

- `AGENTIC_INVESTIGATION_SPEC.md` — the CLI tools and investigation panel (ad-hoc exploration)
- `content_audit.rs:1` — existing 21-check deterministic audit
- `ctr_audit/rendered.rs:1` — existing rendered SERP audit
- `cannibalization_audit.rs:1` — existing cannibalization pipeline
- `indexing_health_campaign.rs:1` — existing indexing health checks

- `AGENTIC_INVESTIGATION_SPEC.md` — the CLI tools and investigation panel (ad-hoc exploration)
- `content_audit.rs:1` — existing 21-check deterministic audit
- `ctr_audit/rendered.rs:1` — existing rendered SERP audit
- `cannibalization_audit.rs:1` — existing cannibalization pipeline
- `indexing_health_campaign.rs:1` — existing indexing health checks
