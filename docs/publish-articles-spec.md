# Publish Articles — Feature Spec

## Problem

The pageseeds-app has no way to transition articles from `ready_to_publish` / `draft` to `published`. The existing CLI implementation (`PublishingRunner`) is ~80% deterministic but hides that behind LLM agent calls for work Python already knows how to do. All the logic needed already exists as Rust functions in this codebase (`content/dates.rs`, `content/cleaner.rs`, `content/ops.rs`) — it just isn't wired into a publish workflow.

---

## Design Goals

1. **Deterministic by default.** All structural cleaning, date analysis, date redistribution, status transitions, and MDX frontmatter patching run in Rust without LLM calls.
2. **Human-gated.** The user selects which articles to publish from the UI. Nothing transitions automatically.
3. **Agent only for genuine ambiguity.** The only case that warrants an LLM call is a title/year mismatch where the right fix requires editorial judgment (update the title year vs backdate the publish date).
4. **Consistent with existing architecture.** New Rust commands are thin wrappers over module functions. Business logic stays in the `content/` module, not in `commands.rs`.

---

## Deterministic vs Agentic Split

| Step | Type | Reason |
|---|---|---|
| Structural scan (duplicate H1s, missing frontmatter) | Deterministic | Rule-based, already in `cleaner.rs` |
| Clean structural issues | Deterministic | Mechanical fix, already in `cleaner.rs` |
| Analyse article dates (future, duplicate, missing) | Deterministic | Arithmetic on dates, already in `dates.rs` |
| Redistribute recent dates (≤7 days) with 2-day spacing | Deterministic | Algorithm already in `dates.rs::calculate_fixes` |
| Detect internal link gaps | Deterministic | Already in `linking.rs` |
| Set `status = "published"`, assign date | Deterministic | SQL UPDATE + JSON write |
| Patch MDX frontmatter (`date:`, `status:`) | Deterministic | Regex already in `ops.rs::apply_date_fixes` |
| **Year mismatch resolution** (title says "2024", publish year is 2026) | **Agentic** | Requires editorial judgment — update title or backdate? |

---

## User Flow

```
Articles tab
  └── "Publish" button (visible when ≥1 ready_to_publish/draft article exists)
        └── PublishPanel (Sheet)
              1. Load ready_to_publish + draft articles
              2. [Run Pre-flight] button
                 → calls preflight_publish_articles()
                 → buckets results:
                   ✅ Ready
                   ⚠️  Date issues (auto-fixable)
                   🤔 Year mismatch (needs agent)
                   ❌ Blocked (file missing, structural error)
              3. For year-mismatch articles:
                 [Resolve with AI] → calls resolve_year_mismatch_agent()
                 → shows proposed resolution, user accepts/edits
              4. [Publish N articles] confirmation button
                 → calls apply_publish_articles(ids, date_fixes, year_resolutions)
                 → progress shown via task_step_progress events
              5. Done state: shows count published, any errors
```

---

## New Rust Code

### `content/publish.rs` (new file)

All business logic for the pre-flight and apply phases.

```rust
pub struct PublishPreflightResult {
    pub ready: Vec<Article>,
    pub needs_date_fix: Vec<ArticleWithIssue>,    // auto-fixable
    pub year_mismatches: Vec<YearMismatch>,        // needs agent
    pub blocked: Vec<ArticleWithIssue>,            // file missing, etc.
    pub structural_issues: CleaningResult,
}

pub struct ArticleWithIssue {
    pub article: Article,
    pub issue: String,
}

pub struct YearMismatch {
    pub article_id: i64,
    pub title: String,
    pub title_year: i32,
    pub publish_year: i32,
}

pub struct YearMismatchResolution {
    pub article_id: i64,
    pub action: YearMismatchAction,   // UpdateTitle | BackdatePublish
    pub new_value: String,            // new title string OR new date string
}

pub enum YearMismatchAction {
    UpdateTitle,
    BackdatePublish,
}

pub struct PublishResult {
    pub published: Vec<Article>,
    pub skipped: Vec<ArticleWithIssue>,
    pub errors: Vec<String>,
}
```

**Functions:**

```rust
/// Run all pre-flight checks. Never writes anything.
pub fn preflight(
    articles: &[Article],
    content_dir: &Path,
    automation_dir: &Path,
) -> Result<PublishPreflightResult>

/// Apply all fixes and transition statuses.
/// date_fixes: map of article_id → new date (from redistribute step)
/// resolutions: map of article_id → YearMismatchResolution (from agent step)
pub fn apply_publish(
    conn: &Connection,
    article_ids: &[i64],
    date_fixes: HashMap<i64, String>,
    resolutions: HashMap<i64, YearMismatchResolution>,
    content_dir: &Path,
    automation_dir: &Path,
) -> Result<PublishResult>
```

The `preflight` function:
1. Calls `cleaner::scan_and_clean(content_dir, dry_run=true)` — structural scan only
2. Calls `dates::analyse_dates(articles)` — buckets issues
3. Calls `dates::calculate_fixes(articles)` — generates auto-fixable date reassignments
4. Detects year mismatches: extract `20\d\d` from title, compare to `published_date.year()`; flag if `publish_year - title_year > 1`
5. Checks file existence on disk for each article's `file` field

The `apply_publish` function (in order):
1. `cleaner::scan_and_clean(content_dir, dry_run=false)` — fix structural issues
2. Apply `date_fixes` + `resolutions` to SQLite `articles` table
3. Apply `resolutions` that have `UpdateTitle` action to SQLite `title` field
4. For each article_id in `article_ids`: set `status = "published"`, ensure `published_date` is set
5. `ops::sync_and_validate(automation_dir, repo_root, apply_sync=true)` — patches MDX frontmatter from SQLite state
6. `export::write_articles_to_repo(conn, automation_dir)` — writes articles.json

### Year mismatch agent call

In `engine/agent.rs` or a thin wrapper, a scoped call:

**Prompt template** (assembled in `content/publish.rs`):
```
You are resolving a year mismatch for an SEO article.

Article title: "{title}"
Title mentions year: {title_year}
Intended publish date year: {publish_year}

The gap is {publish_year - title_year} year(s). Choose one action:
A) Update the title to use year {publish_year}
B) Backdate the publish date to {title_year}-{original_month}-{original_day}

Respond with ONLY valid JSON:
{"action": "update_title", "new_value": "updated title here"}
OR
{"action": "backdate", "new_value": "YYYY-MM-DD"}

Rules:
- Prefer update_title if the content is still current/timeless
- Prefer backdate if the article is genuinely about events in {title_year}
- The backdated date must not conflict with another published article
- Do not add explanation, only JSON
```

Response normalized in Rust with `serde_json` — no free-form parsing.

### New commands (`commands/content.rs` additions)

```rust
#[tauri::command]
pub fn preflight_publish_articles(
    project_id: String,
    article_ids: Vec<i64>,
    state: State<'_, AppState>,
) -> Result<PublishPreflightResult, String>

#[tauri::command]
pub fn apply_publish_articles(
    project_id: String,
    article_ids: Vec<i64>,
    date_fixes: HashMap<String, String>,          // article_id → new_date
    year_resolutions: Vec<YearMismatchResolution>,
    state: State<'_, AppState>,
) -> Result<PublishResult, String>

#[tauri::command]
pub fn resolve_year_mismatch_agent(
    project_id: String,
    article_id: i64,
    title: String,
    title_year: i32,
    publish_year: i32,
    state: State<'_, AppState>,
) -> Result<YearMismatchResolution, String>
```

All three registered in `lib.rs` `generate_handler!`.

---

## New TypeScript Code

### `src/lib/types.ts` additions

```typescript
export interface PublishPreflightResult {
  ready: Article[];
  needsDateFix: ArticleWithIssue[];
  yearMismatches: YearMismatch[];
  blocked: ArticleWithIssue[];
  structuralIssues: CleaningResult;
}

export interface ArticleWithIssue {
  article: Article;
  issue: string;
}

export interface YearMismatch {
  articleId: number;
  title: string;
  titleYear: number;
  publishYear: number;
}

export interface YearMismatchResolution {
  articleId: number;
  action: 'update_title' | 'backdate';
  newValue: string;
}

export interface PublishResult {
  published: Article[];
  skipped: ArticleWithIssue[];
  errors: string[];
}
```

### `src/lib/tauri.ts` additions

```typescript
export const preflightPublishArticles = (projectId: string, articleIds: number[]) =>
  invoke<PublishPreflightResult>('preflight_publish_articles', { projectId, articleIds });

export const applyPublishArticles = (
  projectId: string,
  articleIds: number[],
  dateFixes: Record<string, string>,
  yearResolutions: YearMismatchResolution[],
) => invoke<PublishResult>('apply_publish_articles', { projectId, articleIds, dateFixes, yearResolutions });

export const resolveYearMismatchAgent = (
  projectId: string,
  articleId: number,
  title: string,
  titleYear: number,
  publishYear: number,
) => invoke<YearMismatchResolution>('resolve_year_mismatch_agent', { projectId, articleId, title, titleYear, publishYear });
```

### `src/components/articles/PublishPanel.tsx` (new component)

A `Sheet` triggered from `ArticleTable`. State machine:

```
idle
  → preflight_running (calls preflightPublishArticles)
  → preflight_done (show bucketed results)
       → resolving_mismatch (per article, calls resolveYearMismatchAgent)
  → publishing (calls applyPublishArticles)
  → done (show summary)
  → error
```

Layout (using shadcn/ui only):
- `Sheet` / `SheetContent` / `SheetHeader` / `SheetTitle` / `SheetDescription`
- `ScrollArea` for the article list
- `Badge` for status (ready / date-fix / year-mismatch / blocked)
- `Button` for pre-flight, AI resolve, confirm publish
- `Separator` between buckets
- No raw `<div>` wrappers for the sheet shell

---

## Overview Screen Entry Point

Add a "Publish Ready Articles" quick-action card to the Overview view:
- Shows count of `ready_to_publish` + `draft` articles for the active project
- On click: navigates to Articles tab and opens `PublishPanel` directly

---

## Files Changed

| File | Change |
|---|---|
| `src-tauri/src/content/publish.rs` | New — preflight + apply logic |
| `src-tauri/src/content/mod.rs` | Add `pub mod publish;` |
| `src-tauri/src/commands/content.rs` | Add 3 new commands |
| `src-tauri/src/lib.rs` | Register 3 new commands |
| `src/lib/types.ts` | Add 5 new interfaces |
| `src/lib/tauri.ts` | Add 3 new invoke wrappers |
| `src/components/articles/PublishPanel.tsx` | New component |
| `src/components/articles/ArticleTable.tsx` | Add "Publish" button trigger |
| `src/App.tsx` or `src/components/overview/` | Add quick-action card |

---

## Out of Scope

- `pageseeds content validate` subprocess call — all equivalent logic is already in Rust
- Any git/deploy commands — user deploys via their own pipeline after publishing
- Batch-automated publishing without human confirmation
- Editing article content (title, body) — that's the write_article workflow

---

## Pre-Implementation Checklist

- [ ] `cargo check` passes before touching frontend
- [ ] `publish.rs` functions tested in isolation with fixture articles
- [ ] Year-mismatch detection handles: no year in title, multiple years in title, year at start vs end
- [ ] `apply_publish` is atomic — on any error after partial writes, report which articles succeeded vs failed
- [ ] No secrets or machine paths in source code
- [ ] Types mirror Rust structs exactly
