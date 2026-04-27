# GSC Collection Workflow — App vs CLI Gap Analysis

Comparison of what PageSeeds CLI does vs what PageSeeds App has implemented, and what needs to be built to close the gap.

---

## One-Line Summary

The CLI's `collect_gsc` task uses the **URL Inspection API** to classify every page as indexed/not-indexed, then automatically creates specific fix tasks. The app has none of that — when `collect_gsc` runs, it calls the Python CLI to sync analytics data, marks the task done, and creates nothing. The core value of the workflow (actionable fix tasks) is absent.

---

## What the CLI Does (The Reference)

The CLI `collect_gsc` workflow produces two outcomes:

1. **URL-level diagnosis**: Calls GSC URL Inspection API for every URL in the sitemap (up to 200), classifies each one (`robots_blocked`, `fetch_error`, `canonical_mismatch`, `not_indexed_crawled`, etc.), and writes `gsc_collection.json` with a prioritised action queue.

2. **Automatic task spawning**: Reads that action queue and creates up to 20 specific fix tasks — `fix_indexing`, `fix_technical`, `fix_content`, `fix_gsc_access` — each with the affected URL, reason code, and suggested action in its notes.

The `gsc-sync-articles` step (writing analytics metrics into `articles.json`) is a **separate concern** only used by the `content_review` workflow, not by `collect_gsc`.

---

## What the App Has Built

### ✅ Solid foundations

| Capability | Where it lives | Status |
|---|---|---|
| Service account JWT auth | `gsc/auth.rs` + `commands/gsc.rs:gsc_authenticate` | Working |
| OAuth2 browser flow | `gsc/auth.rs` + `commands/gsc.rs:gsc_oauth_start` | Working |
| Search Analytics API | `gsc/analytics.rs` | Working |
| URL Inspection API (batch) | `gsc/indexing.rs:inspect_batch` | Working |
| Coverage 404 CSV parsing | `gsc/coverage.rs` + `commands/gsc.rs:gsc_parse_coverage_csv` | Working |
| Redirect CSV parsing | `gsc/redirects.rs` + `commands/gsc.rs:gsc_parse_redirect_csv` | Working |
| Indexing report generation | `gsc/reports.rs:generate_and_save_indexing_report` | Working |
| GSC analytics → articles.json | `executor.rs:exec_gsc_sync_articles` | Working (native Rust) |
| 6-tab GSC frontend | `components/gsc/` | Working |
| 10 typed IPC commands | `commands/gsc.rs` | Working |

### ✅ URL Inspection — partially built

`gsc/indexing.rs` has `inspect_batch()` which calls the URL Inspection API correctly with rate-limiting. `commands/gsc.rs:gsc_inspect_urls` exposes it as an IPC command. `GscIndexing.tsx` lets the user paste URLs manually and see results.

The capability exists. It's just not wired into the task system.

---

## The Gaps

### Gap 1 — `collect_gsc` task calls Python CLI instead of native code

**Current behaviour** (`engine/workflows/handlers.rs:37-40`):
```rust
"collect_gsc" => vec![
    WorkflowStep::new("collect_gsc_inspect", StepKind::CollectGscInspect),
],
```

This calls the Python CLI subprocess. If `pageseeds` is not installed, the task silently fails. Beyond availability, it's the wrong CLI command — `gsc-sync-articles` syncs analytics data, it doesn't run URL Inspection.

**What it should do**: Use native Rust. The `inspect_batch` function and `exec_gsc_sync_articles` are both already implemented.

---

### Gap 2 — Wrong operation: analytics sync when URL Inspection is needed

The `collect_gsc` task currently syncs analytics (clicks, impressions, CTR) into `articles.json`. That's what `gsc-sync-articles` does.

But the CLI's `collect_gsc` purpose is different: it runs the **URL Inspection API** to find pages that aren't indexed and diagnose why. Analytics sync is a support step for `content_review`, not the collection phase.

The app has conflated these two distinct operations:

| Operation | Purpose | CLI task | App |
|---|---|---|---|
| URL Inspection | Find and classify non-indexed pages | `collect_gsc` | ❌ not in task system |
| Analytics sync | Populate `gsc` block in articles.json | `content_review` (step 1) | ✅ `exec_gsc_sync_articles` |

---

### Gap 3 — No task spawning after collection

This is the biggest missing piece. After `collect_gsc` completes, the CLI creates up to 20 specific fix tasks. The app creates nothing.

The executor has this pattern for content review:
```rust
// After a successful content review, create a single content_review_apply task.
if all_ok && matches!(task.task_type.as_str(), "content_review" | "content_audit") {
    create_content_review_apply_task(conn, &task, &project_path);
}
```

There is no equivalent block for `collect_gsc`. The user runs the task, it marks done, and nothing actionable appears in the task list.

**What the CLI does instead:**

```
reason_code       → task_type
not_indexed       → fix_indexing
crawl_anomaly     → fix_technical
redirect_error    → fix_technical
canonical_mismatch→ fix_technical
robots_blocked    → fix_technical
soft_404          → fix_content
duplicate_content → fix_content
api_error (all)   → fix_gsc_access
no issues at all  → investigate_gsc
```

Each task gets the affected URL, the reason code, and the action description in its notes. This is the value of the collection phase.

---

### Gap 4 — No sitemap URL loading

The CLI's `gsc-indexing-report` fetches the live sitemap XML and constructs the list of URLs to inspect. The app has no equivalent. `GscIndexing.tsx` requires the user to manually paste URLs.

The app needs a way to fetch the sitemap and populate the URL list automatically — either from `manifest.json`'s `sitemap` field or derived from the project's `url` field.

---

### Gap 5 — `classify_record` and priority scoring not ported to Rust

`gsc/indexing.rs` has `inspect_batch()` which returns `Vec<InspectionRecord>`. But the classification logic — `classify_record()` and `priority_for_record()` from the Python CLI — has not been ported. The app can call the inspection API but cannot categorise the results into actionable buckets.

The CLI's classification logic determines:
- **what type of fix task** to create per URL
- **how urgent it is** (priority 10–999, lower = more urgent)
- **what action description** to show the user

Without this, raw `InspectionRecord` data cannot drive task creation.

---

### Gap 6 — `investigate_gsc` task also calls Python CLI

**Current behaviour** (`handlers.rs:58-61`):
```rust
"investigate_gsc" => vec![
    WorkflowStep::new("investigate_gsc_summarise", StepKind::GscSummarise),
    WorkflowStep::new("investigate_gsc_agent", StepKind::GscInvestigateAgentic),
],
```

In the CLI, `investigate_gsc` is an LLM-based fallback that reads `gsc_collection.json` and writes structured issue recommendations. In the app it calls the Python `content-audit` command, which does audit checks (same as `exec_content_audit`) — a completely different operation. This mapping is wrong.

The `investigate_gsc` task should probably be turned into an agentic step that reads the `gsc_collection.json` artifact from the collection phase and generates a structured investigation report.

---

### Gap 7 — `exec_gsc_sync_articles` ignores the in-memory token

`exec_gsc_sync_articles` in `executor.rs` does its own auth from scratch:
```rust
let token = match rt.block_on(crate::gsc::auth::get_service_account_token(&sa_path)) { ... };
```

It never reads from `GscState`. If the user authenticated via the GSC tab 30 seconds ago, a new JWT is still minted for every content review run. This means auth errors in task execution are disconnected from auth status in the UI.

---

### Gap 8 — No domain validation guard

The CLI validates that the GSC site returned actually matches the project's manifest URL before writing any data. If there's a mismatch, it deletes the output and fails the task with an explicit error.

The app has no equivalent check. A misconfigured `gsc_site` in `manifest.json` could silently write wrong data into `articles.json`.

---

### Gap 9 — Token not persisted across app restarts

`GscState` is initialized as `Mutex<Option<None>>` in `lib.rs`. Every app launch requires re-authentication. There is no keychain or disk cache. While not a workflow blocker, it means users must authenticate on every launch, and automated task runs in the background would fail if no token is held.

---

### Gap 10 — Fix task type enum incomplete

`config/mod.rs` defines `TASK_TYPES` but it's missing the types the collection phase would create:

```rust
pub const TASK_TYPES: &[&str] = &[
    // ...
    "fix_404s",           // ← only this is listed
    "fix_redirects",      // ← only this is listed
    // MISSING: "fix_indexing", "fix_technical", "fix_content", "fix_gsc_access"
];
```

Any UI rendering, filtering, or routing that checks `TASK_TYPES` would not recognise these task types if they were created.

---

## Summary Table

| CLI capability | App has it? | Notes |
|---|---|---|
| URL Inspection API call | ✅ built | In `gsc/indexing.rs:inspect_batch` and `GscIndexing.tsx`, but not in task system |
| Sitemap URL loading | ❌ missing | No automatic URL discovery; GscIndexing requires manual paste |
| `classify_record` — reason codes | ❌ not ported | Rust equivalent of Python's `classify_record()` not written |
| `priority_for_record` — urgency scoring | ❌ not ported | Priority logic not in Rust |
| Task spawning from collection results | ❌ missing | `create_tasks_from_collection()` equivalent doesn't exist |
| `fix_indexing` / `fix_technical` / `fix_content` task types | ❌ incomplete | Not in `TASK_TYPES`; no creation logic |
| Domain validation of collection output | ❌ missing | No guard against wrong-site data |
| `collect_gsc` → native Rust (no Python CLI) | ❌ wrong | Still calls Python `gsc-sync-articles` subprocess |
| `investigate_gsc` — correct operation | ❌ wrong | Calls Python `content-audit` (wrong command entirely) |
| Token shared between UI and task execution | ❌ disconnected | `exec_gsc_sync_articles` mints its own token |
| Token persistence across restarts | ❌ missing | In-memory only |
| Analytics sync → articles.json | ✅ built | `exec_gsc_sync_articles` works correctly (used by content_review) |
| GSC auth (service account + OAuth) | ✅ built | Both paths work |
| 6-tab GSC frontend | ✅ built | Functional for manual use |

---

## Recommended Build Order

Based on the gaps above, the logical order to close them is:

1. **Port `classify_record` + `priority_for_record` to Rust** (`gsc/indexing.rs`) — prerequisite for everything else.

2. **Add sitemap URL loader** (`gsc/` module) — fetches sitemap XML, extracts `<loc>` entries. Used by both the new collect task and the GscIndexing tab.

3. **Replace `collect_gsc` handler** with a native step kind (e.g. `collect_gsc_inspect`) that:
   - Reads sitemap URL from manifest
   - Calls `inspect_batch` on live URLs (up to 200)
   - Runs `classify_record` + `priority_for_record` on each result
   - Writes `gsc_collection.json` to the automation artifacts dir
   - No subprocess required

4. **Add `create_tasks_from_collection`** in `executor.rs` post-step hook — mirrors the CLI logic: reads `gsc_collection.json`, maps reason codes to task types, creates fix tasks in SQLite.

5. **Add missing task types** to `config/mod.rs:TASK_TYPES` — `fix_indexing`, `fix_technical`, `fix_content`, `fix_gsc_access`.

6. **Fix `investigate_gsc` handler** — should be an agentic step that reads `gsc_collection.json` (not call the Python content-audit command).

7. **Wire `GscState` token into `exec_gsc_sync_articles`** — reuse the in-memory token if valid, only mint new one if absent/expired.

8. **Add domain validation** — check `meta.site_url` in `gsc_collection.json` matches project manifest `url` before accepting the output.
