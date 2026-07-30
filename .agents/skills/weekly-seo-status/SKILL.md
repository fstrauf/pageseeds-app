---
name: weekly-seo-status
description: >-
  Cross-project overview of when the last weekly SEO pass ran for each
  PageSeeds project (report age, due/fresh/no-report, last task activity).
  Use when the user wants last weekly SEO run dates, which projects are due,
  SEO run status, or /weekly-seo-status. Read-only operator — never edit
  product source.
when-to-use: >-
  Triggers on "/weekly-seo-status", "weekly SEO status", "last weekly SEO",
  "when was the last SEO run", "which projects need weekly SEO",
  "SEO run overview", "projects due for weekly SEO".
argument-hint: "[optional: due-only | all]"
user-invocable: true
metadata:
  short-description: "Last weekly SEO run per project (due/fresh overview)"
---

# Weekly SEO Status — Cross-Project Recency Board

> Companion to [weekly-seo](../weekly-seo/SKILL.md). This skill **only reports**
> last-run status. It does **not** execute a weekly pass.

## Invocation

```
/weekly-seo-status
/weekly-seo-status due-only
/user:weekly-seo-status
```

| Arg | Meaning |
|-----|---------|
| *(none)* / `all` | Full table of every project |
| `due-only` | Only projects that are **due**, **no report**, or **path missing** |

## Role

| Layer | Role |
|-------|------|
| **This skill** | Registry projects + report files + task recency → status table |
| **weekly-seo** | Run the pass for **one** project when the user asks |
| **Product source** | **Out of scope** — never patch product crates mid-status |

---

## Canonical identity (do not freestyle)

Every project has one operator key. **Always key rows and invoke by `id`.**

| Field | Source | Role |
|-------|--------|------|
| **`id`** | `pageseeds-cli list-projects` / `projects.id` | **Canonical** — table key, `/weekly-seo <id>` |
| **`name`** | same | Human label only (e.g. "Days to Expiry") |
| **`path`** | same | Content root for reports — never invent from folder/domain |

Do **not** match on repo folder name (`call-analyzer`), domain (`daystoexpiry.com`),
or display name alone. Those diverge from `id` (e.g. `days_to_expiry` →
`call-analyzer`).

Example registry rows (illustrative):

| id | name | path |
|----|------|------|
| `days_to_expiry` | Days to Expiry | `…/call-analyzer` |
| `coffee` | Brewedlate | `…/nz-coffee-hub` |
| `expense` | Expense Sorted | `…/tx/txApp` |

---

## Source of truth (two signals)

### A. Weekly report (primary for fresh / due)

```text
{project.path}/.github/automation/weekly_seo_{YYYYMMDD_HHMMSS}.md
```

Written by the weekly-seo skill at end of a pass. **Primary** classifier for
`fresh` / `due`. Absence does **not** mean “no SEO work ever” — agents often
skip the file while still creating tasks.

### B. Last task activity (secondary, always show)

```text
MAX(created_at) FROM tasks WHERE project_id = <id>
```

Operator SQLite (same DB as the CLI):

```text
~/Library/Application Support/com.pageseeds.app/pageseeds.db
```

Use this so “no report” projects with recent desk work (e.g. Days to Expiry)
are not misread as idle. Task activity **does not** flip status to `fresh`
for skip-policy alignment — only the report does.

### Projects list

Prefer (in order):

1. `pageseeds-cli list-projects` (JSON — same registry as product)
2. Fallback: sqlite `SELECT id, name, path FROM projects ORDER BY name`

**Do not invent** project lists, report dates, or task timestamps.

---

## Hard rails

| # | Rule |
|---|------|
| 1 | **Read-only.** No task creates, no executes, no MDX edits, no report writes. |
| 2 | **No product crate edits** under `pageseeds-app` for this skill run. Skill doc updates are separate product work. |
| 3 | Prefer **installed** tools (`pageseeds-cli`, `sqlite3`, filesystem). Do not `cargo run` the product. |
| 4 | **Status from report only:** `fresh` if last report age **&lt; 5 days**; `due` if report age **≥ 5 days**. |
| 5 | Path exists but no `weekly_seo_*.md` → status **`no report`** (not “never ran SEO”). |
| 6 | Missing disk path → **`path missing`**. |
| 7 | Always show **last task activity** when the DB has rows for that `id`. |
| 8 | Flag `pageseeds` (marketing) and `*_live` managed clones in notes; still **include** them in the full table. |
| 9 | Suggest next runs as **`/weekly-seo <id>`** only (canonical id). |

---

## Procedure

### 1. Load projects

```bash
pageseeds-cli list-projects
# → count + projects[].id / name / path
```

If CLI missing/fails:

```bash
DB="$HOME/Library/Application Support/com.pageseeds.app/pageseeds.db"
sqlite3 -separator '|' "$DB" "SELECT id, name, path FROM projects ORDER BY name"
```

Abort clearly if neither works / DB missing / zero projects.

### 2. Resolve last report per project

For each registry row (`id`, `name`, `path`):

1. If `path` does not exist → status **`path missing`**, report **—**, **Runs (7d)** = **—**.
2. Else list all reports (newest first):

```bash
ls -1t "$path/.github/automation"/weekly_seo_*.md 2>/dev/null
```

3. **Newest report** = first line of that list (or none).
4. Parse timestamp from filename: `weekly_seo_YYYYMMDD_HHMMSS.md`  
   - Example: `weekly_seo_20260723_183104.md` → `2026-07-23 18:31`  
   - Use wall clock from the **filename** (do not re-stat for “run time”).
5. **Days ago** = calendar days from today to that date (same day = `0`).
6. **Runs (7d):** count every `weekly_seo_*.md` whose filename timestamp falls
   in the trailing **7 calendar days** (today inclusive; age 0–6 days, or
   equivalently timestamp ≥ today−6d start-of-day). Path missing / no reports
   → **—** / `0`.
7. **Cadence collapse:** when **Runs (7d) ≥ 2** — flag on the row (see Status
   / Cadence below). Retrospective by construction (counts files already on disk).
8. Classify **status** (report only; 5-day threshold unchanged):

| Status | Condition |
|--------|-----------|
| `fresh` | Report exists and age **&lt; 5** days |
| `due` | Report exists and age **≥ 5** days |
| `no report` | Path exists; automation dir missing **or** no `weekly_seo_*.md` |
| `path missing` | `projects.path` not on disk |

When cadence collapse applies, surface it on the row without replacing
`fresh`/`due` — e.g. Status `fresh · cadence collapse` or a **Cadence** column
value `collapse` (prefer one consistent style per board).

### 3. Last task activity (required on default board)

One query for all projects (cheap):

```bash
DB="$HOME/Library/Application Support/com.pageseeds.app/pageseeds.db"
sqlite3 -separator '|' "$DB" \
  "SELECT project_id, MAX(created_at), COUNT(*)
   FROM tasks GROUP BY project_id"
```

Per project:

| Column | Value |
|--------|--------|
| **Last activity** | `MAX(created_at)` formatted local/ISO short (`YYYY-MM-DD HH:MM` or date-only), or **—** if no tasks |
| **Activity age** | Calendar days from that timestamp to today, or **—** |

Optional note when useful (not a new status):

- `no report` + activity age **&lt; 5d** → “recent desk work; report missing”
- `no report` + activity age **≥ 5d** or no tasks → still `no report`

Do **not** invent activity. If sqlite fails, omit the column once with a note.

### 4. Optional open-work count (only if asked or highlighting dues)

```bash
pageseeds-cli list-tasks -i <id> -p <path>
```

Count open fix-like work (`todo` / `queued` / `in_progress` for types such as
`fix_content_article`, `fix_ctr_article`, `content_review`, indexing fixes).
Not required for the default overview.

### 5. Preferred implementation (one script)

Run a **single read-only** bash/python loop so ids/paths/dates stay consistent.
Sketch (adapt; keep local):

```python
# 1) projects from list-projects JSON or sqlite
# 2) for each: report glob → newest + Runs(7d) count → status fresh|due|no report|path missing
#    + cadence collapse if Runs(7d) >= 2
# 3) join task MAX(created_at) by project_id
# 4) sort + print markdown table
```

Do not hand-edit invent rows.

### 6. Present results

**Sort:** `path missing` → `no report` (oldest activity first; no-activity last) →
`due` (oldest report first) → `fresh` (oldest report first).

```markdown
# Weekly SEO status — {YYYY-MM-DD}

| ID | Name | Last report | Report days | Runs (7d) | Status | Last activity | Activity days | Report file |
|----|------|-------------|-------------|-----------|--------|---------------|---------------|-------------|
| `days_to_expiry` | Days to Expiry | — | — | 0 | no report | 2026-07-27 18:29 | 1 | — |
| `coffee` | Brewedlate | 2026-07-23 18:31 | 5 | 1 | due | … | … | `weekly_seo_….md` |
| `learnedlate` | Learned Late | 2026-07-25 07:04 | 3 | 2 | fresh · cadence collapse | … | … | `weekly_seo_….md` |

**Due (report ≥5d):** N  
**No report:** N  
**Fresh (report &lt;5d):** N  
**Path missing:** N  
**Cadence collapse (Runs 7d ≥ 2):** N
```

**Report file** column: basename only (or short relative path). Full path only
if the user needs to open it.

Then **action board**:

```markdown
## Suggested next runs

Prefer projects with `no report` or `due`. Order: path missing (fix path first)
is not runnable; then `no report` with oldest/stale activity; then `due` oldest
report first.

1. `/weekly-seo days_to_expiry` — Days to Expiry (no report; last activity: 2026-07-27)
2. `/weekly-seo expense` — Expense Sorted (no report; last activity: …)
3. `/weekly-seo coffee` — Brewedlate (last report: 2026-07-23)

## Skip for now (fresh)

- `/weekly-seo learnedlate` — Learned Late — report 3d ago

## Notes

- `pageseeds` = product marketing site (optional)
- `*_live` = managed clone (usually skip)
- path missing: list id + registered path so the user can fix the registry
```

If `due-only`: drop `fresh` rows from the table; keep summary counts for all.

### 7. Final user message rules

- No JSON dumps of `list-projects` / `list-tasks`.
- No inventing reports or activity timestamps.
- Never treat folder name / domain as `id`.
- Offer to start **one** weekly pass (`/weekly-seo <id>`) only if the user wants —
  do **not** auto-start all dues.
- If zero projects: say the registry is empty / wrong DB path.
- When `no report` but recent activity: say so once — “desk work without a
  weekly report file; run still recommended so a report is written.”

---

## Alignment with weekly-seo skip policy

| Signal | weekly-seo | this status board |
|--------|------------|-------------------|
| Last **mode-executing report** &lt; 5 days | **Hard refusal** of mode execution (Phase 0); measure-only exempt; override = “run anyway” / “force weekly” | `fresh` (status still report-age only) |
| Last **report** ≥ 5 days | Due for mode work | `due` |
| No report file | Treat as not-fresh (run unless forced) | `no report` |
| Task activity only | Not a skip reason by itself | **Last activity** column only |
| ≥ 5 open fix-like tasks | May skip (load signal; unchanged) | Optional enrichment only |
| **Runs (7d) ≥ 2** | weekly-seo hard-refuses *next* mode run if newest mode report &lt;5d; does not count 7d itself | **`cadence collapse`** flag + summary count (retrospective) |

Same **5-day** threshold on **reports** for `fresh`/`due`. weekly-seo promotes
spacing to a **hard Phase 0 refusal** for mode-executing runs (measure-only
exemption + explicit override). This board stays read-only: **Runs (7d)** and
**cadence collapse** surface how often reports landed in the last week. Task
activity is context so the board does not lie when agents forgot the report
file.

---

## Common failure modes (already fixed in this skill)

| Symptom | Cause | Correct behavior |
|---------|--------|------------------|
| “I ran SEO for Days to Expiry but status says never” | No `weekly_seo_*.md`; tasks exist | Status **`no report`** + show last activity |
| Matching on `call-analyzer` / domain | Folder ≠ registry id | Always use **`days_to_expiry`** from list-projects |
| Path missing for China Tea / Supplylah | Stale `projects.path` | Status **`path missing`**; do not invent a new path |
| Reports not in git | Many repos ignore `.github/automation/*` | Status uses **on-disk** files only; git is irrelevant |

---

## Guardrails (summary)

- Read-only board across all projects.  
- **Canonical key = `project.id`.**  
- Report files drive `fresh` / `due` / `no report`.  
- **Runs (7d)** from filename timestamps; **cadence collapse** when ≥ 2.  
- Task `MAX(created_at)` always shown when available.  
- 5-day report threshold matches weekly-seo Phase 0 spacing.  
- Never edit product crates or customer content during a status run.  
- Do not auto-run weekly passes — only report and suggest `/weekly-seo <id>`.
