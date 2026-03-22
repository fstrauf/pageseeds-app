# Reddit Feature Parity Plan: pageseeds-app → dashboard_ptk

## Status Legend
- [ ] Not started
- [~] In progress
- [x] Done

---

## Current State Summary

The app has solid backend plumbing (agent runner, skills system, deterministic search executor, DB schema) but the UI only exposes manual/ad-hoc workflows. The Python CLI does all the intelligent work (relevance scoring, reply drafting with all config context) via an AI agent. The app's agent infrastructure exists but is not wired for Reddit at all.

### What the CLI does that the app doesn't

| Capability | dashboard_ptk | pageseeds-app |
|---|---|---|
| Auto search from config (trigger topics × seed subreddits) | ✅ | ✅ backend only, no UI trigger |
| Validate all config files exist before running | ✅ | ❌ |
| Optional user context before search | ✅ | ❌ |
| AI-assessed relevance score (0–10) | ✅ | ❌ hardcoded 5.0 |
| `why_relevant`, `key_pain_points`, `website_fit` populated | ✅ | ❌ always null |
| AI reply drafting with config context | ✅ | ❌ 100% manual |
| Mention stance enforcement (REQUIRED/RECOMMENDED/OPTIONAL/OMIT) | ✅ | ❌ |
| Product name check + vague phrase rejection | ✅ | ❌ |
| Critique pass on every reply | ✅ | ❌ |
| Brandvoice applied to replies | ✅ | ❌ |
| `_reply_guardrails.md` applied | ✅ | ❌ hardcoded rules only |
| History file dedup (`_posted_history.json`) | ✅ | ❌ DB-only dedup |
| Markdown artifact saved per search | ✅ | ❌ |

---

## Phase 1 — Automated Search Trigger in UI

**Goal:** Allow the user to run the full automated `reddit_opportunity_search` from the UI, not just manual ad-hoc search.

### Backend

- [x] **`src-tauri/src/reddit/config.rs`** (new file) — `RedditProjectConfig` struct + parser
  - Parse `product_name`, `mention_stance`, `trigger_topics`, `seed_subreddits`, `excluded_subreddits` from `reddit_config.md`
  - Used by Phase 1, 2, 3, 4
- [x] **`src-tauri/src/commands.rs`** — add `run_reddit_opportunity_search(project_id, user_context?)` command
  - Validate all 4 config files exist: `project_summary.md`, `reddit_config.md`, `brandvoice.md`, `reddit/_reply_guardrails.md`
  - Return clear error listing missing files
  - Create `reddit_opportunity_search` task via task store
  - Call `execute_task()` → `exec_reddit_search()`
  - Return count of opportunities found

### Frontend

- [x] **`src/components/reddit/Reddit.tsx`** — add "Run Search" button in Opportunities tab header
  - Pre-search dialog: "Add optional context for this search (Enter to skip)"
  - Pass user context to `run_reddit_opportunity_search`
  - Loading state: "Searching Reddit..."
  - On complete: toast "Found N opportunities"
  - On error: list missing config files with link to project settings

---

## Phase 2 — AI-Powered Relevance Scoring & Enrichment

**Goal:** Replace hardcoded `relevance_score = 5.0` with AI-assessed scores. Fill in `why_relevant`, `key_pain_points`, `website_fit`.

### Backend

- [x] **`src-tauri/src/engine/executor.rs`** — add enrichment step after `exec_reddit_search()`
  - Read un-enriched posts (`why_relevant IS NULL`)
  - Read `project_summary.md` + `reddit_config.md`
  - Build agentic prompt with post titles/content and project context
  - Instruct agent to return structured JSON per post:
    ```json
    [{"post_id":"...","relevance_score":8,"why_relevant":"...","key_pain_points":["..."],"website_fit":"..."}]
    ```
  - Parse response, update `relevance_score`, recalculate `final_score` and `severity` per post
- [x] **`src-tauri/src/commands.rs`** — add `enrich_reddit_opportunities(project_id)` command
  - Manual re-enrichment trigger without re-running the search

### Frontend

- [x] **`src/components/reddit/OpportunityDetail.tsx`** — no structural changes needed; `why_relevant`, `key_pain_points`, `website_fit` will now have data

---

## Phase 3 — AI Reply Drafting

**Goal:** Add "Draft with AI" per opportunity. Agent reads all config context and writes a well-formed reply following the CLI's full prompt logic.

### Backend

- [x] **`src-tauri/src/commands.rs`** — add `draft_reddit_reply(project_id, post_id)` command
  - Load opportunity from DB
  - Read all 4 config files
  - Extract `product_name` and `mention_stance` from `reddit_config.md` (via Phase 1 config parser)
  - Use `reddit-reply-drafting` SKILL.md as base prompt (`skills.rs` already loads skills)
  - Inject: post title, subreddit, selftext, `why_relevant`, `key_pain_points`, `website_fit`, brandvoice, guardrails
  - Enforce: `Acknowledge → Educate → Mention → Engage` formula
  - Enforce: exact product name when stance=REQUIRED
  - Run critique pass instruction in prompt
  - Parse agent output, update `reply_text` in DB, return text to frontend

### Frontend

- [x] **`src/components/reddit/OpportunityDetail.tsx`**
  - Add "Draft with AI" button above reply textarea
  - Loading spinner while drafting
  - On complete: populate textarea (user can edit before posting)
  - On error: toast with error message

---

## Phase 4 — Mention Stance Enforcement

**Goal:** Read `## Mention Stance` from `reddit_config.md`, show it in the UI, enforce it in reply validation.

### Backend

- [x] **DB migration** — add `mention_stance TEXT` column to `reddit_opportunities`:
  ```sql
  ALTER TABLE reddit_opportunities ADD COLUMN mention_stance TEXT;
  ```
- [x] **`src-tauri/src/engine/executor.rs`** — populate `mention_stance` on each saved opportunity during search
- [x] **`src-tauri/src/commands.rs`** — update `validate_reddit_reply(project_id, post_id, text)`:
  - Load `product_name` and `mention_stance` for the project
  - If stance=`REQUIRED`: verify exact product name present (case-insensitive)
  - If missing: return `valid: false, error: "Reply must mention {product_name} by name (stance: REQUIRED)"`
  - Warn (not block) on vague phrases: `"a dedicated tool"`, `"the app"`, `"a platform"`, `"a tracker"`, `"my tool"`

### Frontend

- [x] **`src/lib/types.ts`** — add `mention_stance?: 'REQUIRED' | 'RECOMMENDED' | 'OPTIONAL' | 'OMIT'` to `RedditOpportunity`
- [x] **`src/components/reddit/OpportunityDetail.tsx`**
  - Show stance badge near the reply textarea (REQUIRED = red, RECOMMENDED = orange, OPTIONAL = grey, OMIT = grey)
  - Real-time validation hint as user types (not just on button click)

---

## Phase 5 — History File Deduplication

**Goal:** Keep `_posted_history.json` in sync with the DB so the CLI and app share dedup state.

### Backend

- [x] **`src-tauri/src/reddit/history.rs`** (new file) — `RedditHistoryManager`
  - `is_handled(post_id) -> bool`
  - `mark_posted(post_id) -> Result<()>`
  - `mark_skipped(post_id) -> Result<()>`
  - `get_all_handled_ids() -> HashSet<String>`
  - File location: `{repo}/.github/automation/reddit/_posted_history.json`
- [x] **`src-tauri/src/engine/executor.rs`** — in `exec_reddit_search()`:
  - Load history before upserting posts
  - Skip post_ids already in history
- [x] **`src-tauri/src/commands.rs`** — in `mark_reddit_posted()` and `mark_reddit_skipped()`:
  - After updating DB, write to `_posted_history.json` via `RedditHistoryManager`

---

## Phase 6 — User Context Before Search (add-on to Phase 1)

Covered by Phase 1 — the pre-search dialog passes `user_context` through to `exec_reddit_search()` which injects it into the agentic enrichment prompt.

- [x] **`src-tauri/src/engine/executor.rs`** — accept and inject `user_context: Option<String>` parameter in `exec_reddit_search()`

---

## Phase 7 — Markdown Artifact (Optional)

**Goal:** Save `artifacts/reddit/search_{project}_{timestamp}.md` for auditability and CLI/app interop.

- [ ] **`src-tauri/src/engine/executor.rs`** — after persisting opportunities, write markdown artifact
  - Format matches CLI output structure
  - Path: `{repo}/.github/automation/artifacts/reddit/search_{project}_{timestamp}.md`
  - Store path in task's `output_artifact` field

---

## Implementation Order

```
Phase 1 (Search Trigger) ←── Phase 6 (User Context: simple add-on)
    └── Phase 2 (AI Relevance Scoring)
            └── Phase 4 (Mention Stance DB + validation)

Phase 3 (AI Reply Drafting) ←── start in parallel with Phase 1
    └── Phase 4 (Mention Stance UI)

Phase 5 (History File)    ← independent, any time
Phase 7 (Artifact)        ← independent, lowest priority
```

**Suggested sequence:**
1. **Phase 1 + 6** — automated search from UI (most visible win, exposes existing backend)
2. **Phase 3** — AI reply drafting (core differentiator, biggest gap)
3. **Phase 4** — mention stance (makes Phase 3 output correct and validated)
4. **Phase 2** — enriched relevance scoring (improves search quality)
5. **Phase 5** — history file sync (CLI/app interop correctness)
6. **Phase 7** — markdown artifact (optional, auditability)

---

## Files Changed Summary

| File | Change |
|---|---|
| `src-tauri/src/reddit/config.rs` | **NEW** — `RedditProjectConfig` parser, `MentionStance` enum |
| `src-tauri/src/reddit/history.rs` | **NEW** — `RedditHistoryManager` for `_posted_history.json` |
| `src-tauri/src/commands.rs` | Add `run_reddit_opportunity_search`, `draft_reddit_reply`, `enrich_reddit_opportunities`; update `validate_reddit_reply` |
| `src-tauri/src/engine/executor.rs` | Add enrichment step; inject user_context; history dedup; optional artifact write |
| `src-tauri/src/db/` | Migration: add `mention_stance` column |
| `src/lib/types.ts` | Add `mention_stance` to `RedditOpportunity` |
| `src/components/reddit/Reddit.tsx` | "Run Search" button + pre-search context dialog |
| `src/components/reddit/OpportunityDetail.tsx` | "Draft with AI" button, stance badge, real-time stance hint |
