# SEO Audit Frontend Spec — Disaggregated Dashboard

## Principle

The SEO Audit is **not a new task type**. It is a **frontend composition** that reads artifacts from 4 existing tasks and renders them in a unified dashboard.

The user does not run "SEO Audit." The user runs the same tasks they already run — `content_review`, `ctr_audit`, `cannibalization_audit`, `indexing_health_campaign` — and the dashboard shows the results together.

---

## Navigation

### Sidebar Item: "Health"

```typescript
{ id: 'health', label: 'Health', icon: <HeartPulse size={16} /> }
```

**Placement:** Between "Cannibalization" and "Scheduler".

---

## What Tasks the User Runs

| Data Source | Task Type | Run Policy | How User Triggers It |
|-------------|-----------|------------|---------------------|
| Content health + temporal URLs + page bloat | `content_review` | `UserEnqueue` | Click "Run Full Audit" in Health dashboard |
| CTR + template issues | `ctr_audit` | `AutoEnqueue` | Runs on schedule. Dashboard shows latest data. |
| Cannibalization clusters | `cannibalization_audit` | `AutoEnqueue` | Runs on schedule. Dashboard shows latest data. |
| Indexing + sitemap + redirects | `indexing_health_campaign` | `UserEnqueue` | Click "Run Full Audit" in Health dashboard |

### "Run Full Audit" Button

Creates 2 tasks:
- `content_review` (includes content_audit with new checks)
- `indexing_health_campaign`

`ctr_audit` and `cannibalization_audit` are auto-enqueued — no manual trigger needed.

---

## Dashboard Architecture

The dashboard is a **read-only composition layer**. It reads artifacts from disk.

Data sources:
- `getContentAuditReport(projectId)` → reads `content_audit.json`
- `getCtrHealthSummary(projectId)` → reads CTR health from DB
- `getCannibalizationStrategy(projectId)` → reads strategy from DB
- `getIndexingHealthSummary(projectId)` → reads GSC status from SQLite

---

## Dashboard Layout

### Top Bar
- Title "Health Audit" with `HeartPulse` icon
- "Last updated" timestamp
- "Run Full Audit" button

### Section 1: Priority Issues (Top 5)

Card grid showing highest-impact issues across all data sources. Each card shows:
- Severity badge (Critical / Warning / Info)
- One-line description
- Fix type badge (Auto-fixable / Developer-actionable / Review required)
- Action button navigating to relevant view

### Section 2: Content Health

- Score badge (0–100)
- Stat boxes: temporal URLs, bloat issues, literal vars, dup titles, exact dupes
- Health summary breakdown (good / needs work / poor)
- "View details" link to Articles view

### Section 3: CTR & Template Health

- Healthy / Unhealthy / Title issues / Meta issues counts
- List of top unhealthy articles with issue badges
- "View CTR panel" link

### Section 4: Cannibalization Clusters

- Merge / Hub / Territory counts
- Preview of top merge clusters
- "Review clusters" link to Cannibalization view

### Section 5: Indexing Health

- Indexed / Not indexed / Total URLs counts
- Issue reason breakdown (e.g., `not_indexed_crawled`, `indexing_error`)
- "View GSC" link

---

## Component Structure

```
src/components/health/
└── HealthDashboard.tsx    # All panels inlined in one file (~640 lines)
```

The dashboard uses inline sub-components rather than separate files:
- `EmptyState` — shown when no data exists
- `PriorityIssueCard` — individual priority issue card
- `ContentHealthSection` — content audit summary
- `CtrHealthSection` — CTR health summary
- `CannibalizationSection` — cluster preview
- `IndexingSection` — indexing summary
- `StatBadge` — reusable stat box

---

## Commands

### Backend (`src-tauri/src/commands/health.rs`)

| Command | Purpose |
|---------|---------|
| `run_health_audit` | Spawns `content_review` + `indexing_health_campaign` tasks |
| `get_content_audit_report` | Reads `content_audit.json` from automation dir |
| `get_indexing_health_summary` | Queries `gsc_url_indexing_status` SQLite table |

### Frontend (`src/lib/tauri.ts`)

| Wrapper | Command |
|---------|---------|
| `runHealthAudit` | `run_health_audit` |
| `getContentAuditReport` | `get_content_audit_report` |
| `getIndexingHealthSummary` | `get_indexing_health_summary` |

---

## Files Modified

| File | Change |
|------|--------|
| `src/components/layout/Sidebar.tsx` | Add Health nav item |
| `src/components/health/HealthDashboard.tsx` | New — main dashboard (all panels inlined) |
| `src/App.tsx` | Add `'health'` to `VALID_VIEWS`, render `HealthDashboard` |
| `src/lib/types.ts` | Add `'health'` to `View` union |
| `src/lib/tauri.ts` | Add 3 new wrappers |
| `src-tauri/src/commands/health.rs` | New — 3 commands |
| `src-tauri/src/commands/mod.rs` | Add `pub mod health;` |
| `src-tauri/src/lib.rs` | Register commands in handler list |

---

## Data Flow

```
User clicks "Run Full Audit"
        ↓
Frontend calls runHealthAudit() → creates content_review + indexing_health_campaign
        ↓
Tasks run, write artifacts:
  - content_audit.json (with new checks)
  - ctr_audit_context.json
  - cannibalization_strategy.json
  - indexing output
  - seo_feature_spec.md (from post_actions)
        ↓
Dashboard reads all artifacts → renders composite view
```

---

## Summary

**What the user does:**
1. Clicks "Health" in sidebar
2. Clicks "Run Full Audit"
3. Waits for tasks to complete
4. Views unified dashboard

**What the user does NOT do:**
- Does NOT run a new "seo_audit" task (it doesn't exist)
- Does NOT manually trigger `ctr_audit` or `cannibalization_audit`
- Does NOT navigate to 4 different pages

**Zero new task types. Zero new handlers.**
