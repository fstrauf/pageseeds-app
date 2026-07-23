---
name: weekly-seo-status
description: >-
  Cross-project overview of when the last weekly SEO pass ran for each
  PageSeeds project (report age, due/fresh, never-run). Use when the user
  wants last weekly SEO run dates, which projects are due, SEO run status,
  or /weekly-seo-status. Read-only operator — never edit product source.
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
| `due-only` | Only projects that are **due**, **never**, or **path missing** |

## Role

| Layer | Role |
|-------|------|
| **This skill** | Read projects + latest `weekly_seo_*.md` → status table |
| **weekly-seo** | Run the pass for **one** project when the user asks |
| **Product source** | **Out of scope** — never patch `pageseeds-app` |

---

## Source of truth

Weekly runs leave a report file (written by the weekly-seo skill):

```text
{project.path}/.github/automation/weekly_seo_{YYYYMMDD_HHMMSS}.md
```

Projects come from the same SQLite DB as the desktop app / CLI:

```text
~/Library/Application Support/com.pageseeds.app/pageseeds.db
```

**Do not invent** dates or project lists. Everything must come from SQLite +
filesystem (or `pageseeds-cli list-tasks` if you optionally enrich).

---

## Hard rails

| # | Rule |
|---|------|
| 1 | **Read-only.** No task creates, no executes, no MDX edits, no report writes. |
| 2 | **No product source edits** under `pageseeds-app`. |
| 3 | Prefer the **installed** tools (`sqlite3`, filesystem). Do not `cargo run` the product. |
| 4 | Align recency with weekly-seo: **fresh** if last report age **&lt; 5 days**; else **due**. |
| 5 | If path missing or no reports → say so explicitly (`path missing` / `never`). |
| 6 | Skip non-customer noise only when listing for action: you may flag `pageseeds` (product marketing site) and `*_live` managed clones, but still **include them** in the full table with a note. |

---

## Procedure

### 1. Load projects

```bash
DB="$HOME/Library/Application Support/com.pageseeds.app/pageseeds.db"
sqlite3 -separator '|' "$DB" "SELECT id, name, path FROM projects ORDER BY name"
```

State row count. Abort with a clear error if the DB is missing.

### 2. Resolve last report per project

For each `path`:

1. If `path` does not exist → status **`path missing`**, last run **—**.
2. Else list newest report:

```bash
ls -1t "$path/.github/automation"/weekly_seo_*.md 2>/dev/null | head -1
```

3. Parse timestamp from filename: `weekly_seo_YYYYMMDD_HHMMSS.md`
   - Example: `weekly_seo_20260723_183104.md` → `2026-07-23 18:31:04` (local wall clock from the name; do not re-stat for the “run time” column unless the filename is unreadable).
4. Compute **days ago** from today (calendar days is fine; use whole days, e.g. same day = `0`).
5. Classify:

| Status | Condition |
|--------|-----------|
| `fresh` | Report exists and age **&lt; 5** days |
| `due` | Report exists and age **≥ 5** days |
| `never` | Path exists, automation dir missing **or** no `weekly_seo_*.md` |
| `path missing` | `projects.path` not on disk |

Optional one-liner for a single project (illustrative — adapt for your shell):

```bash
# newest report basename for PATH
ls -1t "$PATH/.github/automation"/weekly_seo_*.md 2>/dev/null | head -1
```

You may script the loop in bash/python for accuracy; keep it local and read-only.

### 3. Optional enrichment (cheap, only if useful)

If the user asks about backlog pressure, or when highlighting **due** projects:

```bash
pageseeds-cli list-tasks -i <id> -p <path>
```

Count open fix-like work (`todo` / `queued` / `in_progress` for types such as
`fix_content_article`, `fix_ctr_article`, `content_review`, indexing fixes).
Do **not** require this for the default overview — report age alone is enough.
If CLI is missing, omit the open-work column and note it once.

### 4. Present results

**Primary output — markdown table** (sort: `path missing` → `never` → `due` (oldest first) → `fresh` (oldest first within group is fine)):

```markdown
# Weekly SEO status — {YYYY-MM-DD}

| Project | ID | Last run | Days ago | Status | Report |
|---------|----|----------|----------|--------|--------|
| … | … | 2026-07-23 18:31 | 1 | fresh | `…/weekly_seo_….md` |
| … | … | — | — | never | — |

**Due now (≥5d or never):** N  
**Fresh (&lt;5d):** N  
**Path missing:** N
```

Then a short **action board**:

```markdown
## Suggested next runs

1. `/weekly-seo <id-or-name>` — {project} (last: {date or never})
2. …

## Skip for now (fresh)

- {project} — {days}d ago
```

If `due-only` was requested, drop the fresh rows from the table but keep counts.

### 5. Final user message rules

- No JSON dumps of `list-tasks`.
- No inventing reports that are not on disk.
- Offer to start **one** weekly pass (`/weekly-seo …`) only if the user wants — do **not** auto-start all dues.
- If zero projects: say the DB has no rows / wrong DB path.

---

## Alignment with weekly-seo skip policy

The weekly-seo skill may **skip a run** when last weekly is **&lt; 5 days** *or*
there are **≥ 5** open fix-like tasks (unless the user forces). This status skill
uses the **same 5-day** threshold for `fresh` vs `due`. Open-task pressure is
optional context only; do not re-implement the full skip logic unless asked.

---

## Guardrails (summary)

- Read-only board across all projects.  
- SQLite + `weekly_seo_*.md` filenames are the recency SoT.  
- 5-day threshold matches weekly-seo.  
- Never edit product source or customer content.  
- Do not auto-run weekly passes — only report and suggest.
