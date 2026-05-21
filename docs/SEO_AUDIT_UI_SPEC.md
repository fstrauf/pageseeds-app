# SEO Audit — UI/UX Specification

How the unified audit, feature specs, and tracking layer surface in the PageSeeds frontend.

---

## User Journey

```
[1] Land on Overview
    └─ See health summary at a glance
    └─ See if audit is fresh or stale
    └─ One-click "Run Full Audit" if needed

[2] Click "Run Full Audit"
    └─ Queue starts → tasks appear in Recent Activity
    └─ Spinner / progress indicator

[3] Audit completes (30–90s)
    └─ Overview updates with new findings
    └─ Priority issues card shows top 5 cross-layer patterns
    └─ "Developer Feature Spec" card appears if code fixes found

[4] User inspects findings
    └─ Click priority issue → opens TaskDetail with full context
    └─ Click "View Spec" → opens markdown viewer with P0/P1/P2
    └─ Click "Fix Content" → enqueues fix tasks

[5] Fix tasks run
    └─ Progress tracked in Recent Activity
    └─ Individual articles show status
    └─ Cooldown prevents re-fixing recently touched articles

[6] User checks status later
    └─ Overview shows "Last audit: 2 days ago"
    └─ Stale indicator if > 14 days
    └─ Diff view shows new vs resolved vs worsened
```

---

## UI Surfaces

### 1. Overview — The Single Source of Truth

Overview is the primary landing page. It must answer three questions immediately:

1. **Is my site healthy?** → Health score + status
2. **Is my audit fresh?** → Timestamp + stale indicator
3. **What should I do?** → Priority actions

#### Layout

```
┌─────────────────────────────────────────────────────────────┐
│  Overview — coffee project                        [stale ⚠] │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌────────────────────┐  ┌────────────────────────────────┐ │
│  │  Health Score      │  │  Priority Issues (top 5)       │ │
│  │                    │  │  ───────────────────────────── │ │
│  │  ████████░░ 78     │  │  🔴 Template rewriting 72      │ │
│  │  Good              │  │     titles + duplicate brand   │ │
│  │                    │  │     → P0 Code Fix              │ │
│  │  Last audit:       │  │                                │ │
│  │  2 days ago        │  │  🟡 6 articles share identical │ │
│  │                    │  │     body content               │ │
│  │  [Run Full Audit]  │  │     → P1 Content Fix           │ │
│  │                    │  │                                │ │
│  │  [Quick Actions]   │  │  🟡 8 temporal URLs decaying   │ │
│  └────────────────────┘  │     → P2 Structural            │ │
│                          │                                │ │
│  ┌────────────────────┐  │  🟡 14 not-indexed pages       │ │
│  │  Audit Freshness   │  │     lacking internal links     │ │
│  │                    │  │     → P1 Content Fix           │ │
│  │  Content: 2d ✅    │  │                                │ │
│  │  Rendered: 5d ✅   │  │  🟡 GSC flat 90 days +         │ │
│  │  Architecture: 7d ✅│  │     cannibalization cluster    │ │
│  │  Spec: 2d ✅       │  │     → P1 Content Fix           │ │
│  └────────────────────┘  └────────────────────────────────┘ │
│                                                             │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Developer Feature Spec                                 │ │
│  │  ───────────────────────────────────────────────────── │ │
│  │  Generated 2 days ago from content_review audit         │ │
│  │                                                        │ │
│  │  🔴 3 P0 code fixes  │  🟡 5 P1 content fixes          │ │
│  │  🟡 2 P2 structural  │                                  │ │
│  │                                                        │ │
│  │  [View Full Spec]  [Open in Target Repo]               │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌────────────────────┐  ┌────────────────────────────────┐ │
│  │  Recent Activity   │  │  Run Workflow                  │ │
│  │  (clickable rows)  │  │  (quick actions)               │ │
│  └────────────────────┘  └────────────────────────────────┘ │
│                                                             │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Full Health Dashboard                                  │ │
│  │  (layer breakdowns, charts, detail)                    │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

#### Components

**Health Score Card**
- Aggregate score from all 3 layers (content + rendered + architecture)
- Color: green ≥ 80, yellow ≥ 60, red < 60
- Shows last audit timestamp
- Stale indicator if > 14 days

**Priority Issues Card**
- Reads `seo_audit_report.json` → `findings[]`
- Shows top 5 by severity
- Each issue shows:
  - Severity badge (🔴 P0 code / 🟡 P1 content / 🟢 P2 structural)
  - One-line description
  - Fix type label ("Code Fix", "Content Fix", "Structural")
  - Click → opens TaskDetail with full context

**Developer Feature Spec Card**
- Only visible if `seo_feature_spec.md` exists
- Shows generation date + source audit
- Count badges: P0 / P1 / P2
- "View Full Spec" → opens markdown modal
- "Open in Target Repo" → opens file in default editor (if local)

**Audit Freshness Card**
- Shows age of each layer's data
- Green ✅ if < 7 days
- Yellow ⚠️ if 7–14 days
- Red 🔴 if > 14 days
- Click any row → runs that specific audit

---

### 2. TaskDetail — Deep Inspection

When the user clicks a priority issue or a task in Recent Activity, the TaskDetail panel opens.

#### For `seo_audit` tasks

```
┌────────────────────────────────────────┐
│  SEO Audit                             │
│  Status: Done ✅                       │
│  Duration: 47s                         │
├────────────────────────────────────────┤
│                                        │
│  Findings (5)                          │
│  ─────────────────────────────────────│
│  🔴 P0 — Template rewriting titles     │
│     72 pages affected. Source titles   │
│     are overwritten by layout.tsx.     │
│     [View evidence]                    │
│                                        │
│  🟡 P1 — Duplicate body content        │
│     6 articles share identical content │
│     [View articles] [Merge]            │
│                                        │
│  🟡 P1 — Temporal URLs                 │
│     8 articles with dated slugs        │
│     [View list] [Rewrite]              │
│                                        │
├────────────────────────────────────────┤
│  Artifacts                             │
│  ─────────────────────────────────────│
│  seo_audit_report.json    → View      │
│  content_audit.json       → View      │
│  rendered_serp_audit.json → View      │
│  site_architecture.json   → View      │
│  seo_feature_spec.md      → View      │
│                                        │
├────────────────────────────────────────┤
│  [Mark as Done]                        │
└────────────────────────────────────────┘
```

#### For `generate_feature_spec` tasks

```
┌────────────────────────────────────────┐
│  Feature Spec from content_review      │
│  Status: Review 🟡                     │
├────────────────────────────────────────┤
│                                        │
│  Markdown Preview                      │
│  ─────────────────────────────────────│
│  # SEO Feature Specification           │
│  Generated: 2026-05-21 14:32 UTC       │
│                                        │
│  ## P0 — Code Changes Required         │
│  - **Problem**: Template rewriting...  │
│  - **Fix**: Edit app/layout.tsx...     │
│                                        │
│  ## P1 — Content Fixes                 │
│  ...                                   │
│                                        │
│  [Scrollable markdown area]            │
│                                        │
├────────────────────────────────────────┤
│  [Mark as Done]                        │
└────────────────────────────────────────┘
```

**Key interaction:** The markdown preview renders with syntax highlighting for code blocks. P0 sections are visually distinct (red border). The user can scroll through the full spec without leaving the app.

---

### 3. HealthDashboard — Layer Breakdown

The HealthDashboard (now embedded in Overview below the fold) shows the detailed layer breakdowns.

```
┌─────────────────────────────────────────────────────────────┐
│  Content Health          │  Rendered SERP Health           │
│  ──────────────────────  │  ─────────────────────────────  │
│  Total: 150 articles     │  Crawled: 150 pages             │
│  Good: 89 (59%)          │  Title match: 45 (30%)          │
│  Needs work: 42 (28%)    │  Title mismatch: 72 (48%)       │
│  Poor: 19 (13%)          │  404 errors: 6 (4%)             │
│                          │  Missing schema: 27 (18%)       │
│  [View details]          │  [View details]                 │
├─────────────────────────────────────────────────────────────┤
│  Architecture            │  Cannibalization                │
│  ──────────────────────  │  ─────────────────────────────  │
│  Indexed: 134            │  Clusters: 12                   │
│  Not indexed: 16         │  At-risk articles: 34           │
│  Orphans: 8              │  Merge candidates: 4            │
│  Plateaus: 23            │  [Review clusters]              │
│  [View details]          │                                 │
├─────────────────────────────────────────────────────────────┤
│  Developer Feature Spec                                     │
│  ─────────────────────────────────────────────────────────  │
│  Last generated: 2 days ago                                 │
│  P0 code fixes: 3    P1 content fixes: 5    P2 structural: 2│
│  [View Full Spec]  [Regenerate]                             │
└─────────────────────────────────────────────────────────────┘
```

---

### 4. Articles View — Per-Article Context

When browsing articles, each row should show audit status at a glance.

```
Article                        │ Status      │ Health │ Last Audit │ Actions
───────────────────────────────┼─────────────┼────────┼────────────┼─────────
Wheel Strategy Guide           │ Published   │ 72 🟡  │ 2d ago     │ [Fix]
Best Coffee Beans 2026         │ Published   │ 45 🔴  │ 2d ago     │ [Fix]
Theta Decay Explained          │ Published   │ 88 🟢  │ 2d ago     │ —
Covered Call Screener          │ In Review   │ —      │ —          │ [View]
```

**Hover tooltip on status:** Shows which checks failed and whether the article is in cooldown.

**Cooldown indicator:** If an article was fixed < 14 days ago, the [Fix] button is disabled with tooltip: "Fixed 3 days ago — cooldown expires in 11 days."

---

### 5. Queue / Runner Status

A persistent indicator showing queue state.

```
┌────────────────────────┐
│  ⚡ Queue: Running     │
│  3 tasks queued        │
│  1 task in progress    │
│  [Pause] [View]        │
└────────────────────────┘
```

When the user clicks "Run Full Audit":
- Button becomes disabled with spinner
- Queue indicator appears
- Tasks appear in Recent Activity with progress

---

## Data Flow: Backend → Frontend

```
Backend
  seo_audit task completes
    → writes seo_audit_report.json
    → writes seo_feature_spec.md
    → emits tauri event: "audit:completed"

Frontend
  Overview subscribes to "audit:completed"
    → re-fetches getProjectOverview()
    → re-fetches getFeatureSpec()
    → updates Health Score, Priority Issues, Spec Card

  TaskBoard subscribes to queue events
    → updates task statuses
    → shows/hides progress indicators
```

---

## Key Interactions

### Interaction: Run Full Audit

| Step | User Action | System Response |
|------|-------------|-----------------|
| 1 | Click "Run Full Audit" | Button disables, spinner shows |
| 2 | — | `runHealthAudit()` creates `seo_audit` task |
| 3 | — | Task enqueues, queue runner starts |
| 4 | — | Steps execute: content → rendered → architecture → synthesis |
| 5 | — | Post-actions spawn fix tasks + generate_feature_spec |
| 6 | — | Event emitted: "audit:completed" |
| 7 | Overview refreshes | New data appears |
| 8 | User sees findings | Priority issues + spec card visible |

### Interaction: View Feature Spec

| Step | User Action | System Response |
|------|-------------|-----------------|
| 1 | Click "View Full Spec" | Markdown modal opens |
| 2 | — | `getFeatureSpec(projectId)` fetches markdown |
| 3 | Modal renders | P0/P1/P2 sections collapsible |
| 4 | User scrolls | Full spec readable |
| 5 | User clicks P0 issue | Expands to show evidence + fix instructions |
| 6 | User closes modal | Returns to Overview |

### Interaction: Fix Content

| Step | User Action | System Response |
|------|-------------|-----------------|
| 1 | Click "Fix" on priority issue | Confirmation: "This will enqueue fix tasks for 5 articles" |
| 2 | Confirm | Tasks created with `Cooldown { days: 14 }` |
| 3 | — | Queue runs fix tasks sequentially |
| 4 | — | Each fix updates `last_edited_at` |
| 5 | All done | Articles table refreshes, status updated |

---

## States & Edge Cases

### State: No audit run yet

- Health Score shows "—" (no data)
- Priority Issues card shows "Run Full Audit to see findings"
- Feature Spec card hidden
- Run Full Audit button prominent

### State: Audit running

- Health Score shows previous data (if any) with "Updating..." overlay
- Priority Issues card shows previous findings (if any) dimmed
- Recent Activity shows running tasks
- Queue indicator visible

### State: Audit complete, no issues

- Health Score green, 100 or close
- Priority Issues card shows "No issues found — great job!"
- Feature Spec card hidden (or shows "No action required")

### State: Audit complete, only P0 code fixes

- Health Score red or yellow
- Priority Issues shows P0 items
- Feature Spec card prominent with P0 count
- Message: "Developer action required — 3 code fixes identified"

### State: Article in cooldown

- Articles table shows "Fixed 5d ago" tooltip
- Fix button disabled
- Status shows "Cooldown (9d left)"

### State: Stale audit (> 14 days)

- Health Score card shows "Stale ⚠️" badge
- Priority Issues dimmed with "Data may be outdated"
- Prompt to re-run audit

---

## What's Missing (UI Gaps)

| Gap | Impact | Priority |
|-----|--------|----------|
| No markdown viewer component | Can't render feature spec | P1 |
| No `getFeatureSpec` command | Frontend can't read spec file | P1 |
| No "audit:completed" event | Frontend doesn't auto-refresh | P1 |
| No cooldown indicator in Articles table | User can't see why Fix is disabled | P2 |
| No diff view (new vs resolved) | Can't track progress over time | P2 |
| No queue persistent indicator | User forgets queue is running | P2 |
| No per-article audit history | Can't see what was fixed when | P3 |

---

## Implementation Order

**Phase 1 (UI Unblocking):**
1. Backend command `get_feature_spec`
2. Tauri event `audit:completed` (or reuse existing queue events)
3. Markdown viewer component (can be simple `<pre>` initially)

**Phase 2 (Core Experience):**
4. Overview Priority Issues card
5. Overview Feature Spec card
6. HealthDashboard layer breakdown refresh

**Phase 3 (Polish):**
7. TaskDetail artifact preview
8. Articles table cooldown indicator
9. Queue persistent indicator

**Phase 4 (Advanced):**
10. Diff view (new/resolved/worsened)
11. Per-article audit history
12. Stale data warnings
