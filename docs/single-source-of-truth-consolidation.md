# Single Source of Truth Consolidation

Date: 2026-05-21

Continuation of `reuse-consolidation-audit.md` (2026-04-30). That audit documented architectural fragmentation (queue, task creation, article persistence). This spec documents **operational fragmentation** — the case where a single primitive exists but isn't used, because 10+ other modules rewrote their own copy.

---

## Root Cause

When an agent adds a feature in `engine/exec/new_feature.rs`, it cannot see that `content/ops.rs` already has `count_words()` or that `content/slug.rs` already has `normalize_url_slug()`. The agent writes its own inline version. Later, someone changes the canonical version — the copy silently diverges. The broken behavior shows up weeks later in a different workflow.

**The only operation that avoided this: `articles.json` export (`db/export.rs`)** — because it's the *only* file that writes that file, and everything imports from it.

---

## The 10 Duplicated Operations

### 1. Word count — 1 canonical + 26 ad-hoc call sites (SEVERE)

| Canonical | Ad-hoc pattern | Files affected | Impact |
|---|---|---|---|
| `content/ops.rs:74` — `count_words()` strips markdown syntax | `text.split_whitespace().count()` — raw split, no markdown stripping | 14 files (cannibalization_audit, ctr_audit, content_audit, quality_rater, fix_generate, fix_verify, consolidate_cluster, feature_spec, index_fix, reddit/validation, etc.) | Same article gets different word counts depending on which workflow asks |

**Fix:** Replace all 26 ad-hoc calls with `content::ops::count_words()`. Mechanical — no logic change needed.

### 2. Slug generation — 8 distinct functions (SEVERE)

| Function | File | Behavior |
|---|---|---|
| `normalize_url_slug()` | `content/slug.rs:89` | **CANONICAL** — strips numeric prefix, lowercase, `_`→`-` |
| `slug_from_filename()` | `content/ops.rs:28` | Preserves underscores (different result) |
| `derive_url_slug()` | `content/article_index.rs:352` | Delegates partially to canonical |
| `slugify()` | `cannibalization_audit.rs:669` | Lowercase → split → join `_` |
| `slugify()` | `research/landing_page.rs:206` | Different algorithm (char-by-char) |
| `slugify_url()` | `indexing_health_campaign.rs:927` | URL-specific variant |
| `slug_from_live_site_path()` | `coverage.rs:132` | Path-specific variant |
| `normalize_slug_underscored()` | `handlers.rs:1459` | Different normalization |

**Fix:** Audit each non-canonical caller. Some generate a different result intentionally (underscores vs. dashes, prefix handling). Standardize on `content::slug` or add named variants there. Delete duplicates.

### 3. Frontmatter parsing — 7 different approaches (SEVERE)

| Function | File | Approach |
|---|---|---|
| `split_mdx()` + `parse()` | `content/frontmatter.rs:36` | **CANONICAL** — serde_yaml semantic parse |
| `parse_frontmatter()` | `content/cleaner.rs:28` | Text-split only, returns `(&str, &str)` |
| `parse_frontmatter()` | `engine/exec/utils.rs:21` | Line-by-line `key:value` → HashMap |
| `parse_frontmatter()` | `social/content/extractor.rs:64` | Different `---` delimiter detection |
| `extract_frontmatter_string()` | `engine/exec/indexing_fix.rs:326` | `\n---\n` closing delimiter |
| `extract_frontmatter_string()` | `engine/exec/gsc/drift.rs:510` | No closing delimiter check |
| `replace_frontmatter_field()` | `content/cleaner.rs:150` | Line-based with alias handling |

**Fix:** Route everything through `content::frontmatter`. Remove `cleaner::parse_frontmatter` and `utils::parse_frontmatter`. Add `frontmatter::extract_frontmatter_string()` as a thin canonical helper.

### 4. Agent invocation — 14 call paths, 13 bypass the central handler (SEVERE)

| Central path | Bypasses |
|---|---|
| `handlers.rs::exec_agentic()` — loads skill, builds prompt, calls agent, normalizes output | 13 exec modules call `agent::run_agent()` directly: feature_spec, indexing_link, coverage, indexing_fix, social/templates, social/generate, research/mod, reddit/config, reddit/enrich, gsc/investigate, ctr_audit/analyze, content/cluster_link, research/mod (tool agent) |

Each bypass builds its own prompt, handles errors differently, parses output differently. When the agent provider changes, all 14 sites need updating.

**Fix:** Route bypasses through `exec_agentic()` or a shared typed helper. Add a `RigAgenticContext` struct that assembles prompt + skill + output contract generically.

### 5. Prompt construction — 14 distinct builders (SEVERE)

| Central builder | Domain-specific builders |
|---|---|
| `engine/prompts.rs::build_prompt()` — assembles SKILL.md + project context + task artifacts | 8 exec modules build their own: content/review, research/prompts, coverage, cannibalization_audit, indexing_health_campaign, ctr_audit/generate, content/fix_generate, research/mod |

**Fix:** Add typed prompt section builders to `engine::prompts` (e.g., `build_skill_section`, `build_article_context_section`, `build_artifact_section`). Domain modules compose from these rather than building raw strings.

### 6. DB connections opened in exec/ modules — 23 violations (HIGH)

Per the AGENTS.md rule: exec modules should receive a connection, not open one. 23 exec/ files create their own `rusqlite::Connection`. Some even open the DB multiple times in the same module (e.g., `indexing_link.rs` at lines 86 and 740, `gsc/drift.rs` at lines 172, 254, 297).

**Fix:** Pass connections from the caller (executor or step registry). This also enables future connection pooling.

### 7. Raw SQL in exec/ modules — 6 queries (HIGH)

| File | SQL |
|---|---|
| `cannibalization_audit.rs:752` | `SELECT ... FROM articles` |
| `content/task_spawner.rs:63,88` | `UPDATE articles SET review_status` |
| `territory_research.rs:346` | `SELECT ... FROM articles` |
| `gsc/sync.rs:388` | `UPDATE articles SET target_keyword` |
| `content/hub_page.rs:48` | `SELECT ... FROM articles` |

**Fix:** Route through `content::article_index` or `content::ops`. The hub_page one is legacy code to be deleted (see Phase 3 below).

### 8. Date computation — 3 copies of same backward-cursor algorithm (MODERATE)

`content/date_policy.rs:133` (`suggest_next_safe_date`), `handlers.rs:1626` (`compute_next_publish_date`), and `content/publish.rs:463` (`assign_free_date`) all walk backward from yesterday, skipping occupied dates. `compute_next_publish_date` has a comment: "implements the same logic as suggest_next_safe_date but reads from SQLite."

**Fix:** Make `suggest_next_safe_date` accept an `Iterator<Item = NaiveDate>` for occupied dates. Both SQLite and in-memory callers feed it their date list. One algorithm, parameterized by data source.

### 9. Unregistered task types — 9 tasks missing from definitions (MODERATE)

| Missing from `task_definitions.rs` | Where handled |
|---|---|
| `cluster_and_link` | `handlers.rs:234` |
| `publish_content` | `handlers.rs:238` |
| `content_strategy` | `handlers.rs:240` |
| `technical_fix` | `handlers.rs:241` |
| `landing_page_spec` | `handlers.rs:243` |
| `territory_research` | `handlers.rs:690` (4-step pipeline) |
| `social_generate_from_article` | `handlers.rs:618` |
| `social_regenerate_post` | `handlers.rs:624` |
| `social_create_template` | `handlers.rs:629` |

Silent fallback to `phase: implementation, run_policy: UserEnqueue, review_surface: None, follow_up_policy: None`.

**Fix:** Add all 9 to `task_definitions.rs` with correct policies. `cluster_and_link` should be `AutoEnqueue` / `BackendAuto`.

### 10. Bare agentic steps — 7 with no skill parameter (MODERATE)

In `handlers.rs`, 7 places create `StepKind::Agentic` with no `with_param("skill", ...)`:
- `collect_posthog` fallback
- `investigate_posthog` fallback
- `research_keywords` / `research_landing_pages` (seed extraction/validation)
- `write_article` / `optimize_article` / `create_content` / `optimize_content` (content write stage)
- `fix_*` catch-all (fix_404s, fix_redirects, etc.)
- `reddit_*` fallback

Some are intercepted (research gets routed to Rig extractor). Some are truly bare (`fix_404s`, `fix_redirects`) and will produce garbage.

**Fix:** Every bare `StepKind::Agentic` must have a skill parameter or a documented reason why not. `fix_404s` and `fix_redirects` need proper handler plans or removal from the task creation UI.

---

## Additional Findings

### Dead social module (HIGH)
8 files in `social/` masked with `#![allow(dead_code)]`. Either never fully integrated or abandoned.

### Legacy hub page still wired (MEDIUM, documented April 30)
`engine/exec/content/hub_page.rs` (140 lines) still exists despite being the canonical anti-pattern in AGENTS.md. `create_hub_page` and `refresh_hub_page` still in task_definitions.

### ImplementationHandler grab-bag (MEDIUM)
`handlers.rs` ImplementationHandler handles 22 task types. Match arms use `starts_with("fix_")` as catch-all — order matters, bare agentic is the default. Adding a new fix type requires knowing the fallthrough behavior.

### `engine_clean.rs` — dead demo file (LOW)
125-line demonstration file not compiled into the app but present in the commands directory.

---

## Implementation Plan

### Phase 1: Mechanical consolidation (low risk, high impact — start here)

**1a. Unify word count** — Replace 26 ad-hoc `split_whitespace().count()` with `content::ops::count_words()`. Pure cleanup, no behavior change intended.

**1b. Unify date computation** — Parameterize `suggest_next_safe_date` to accept occupied dates. Delete `compute_next_publish_date` and `assign_free_date`.

**1c. Unify frontmatter parsing** — Route all callers through `content::frontmatter`. Add `frontmatter::extract_frontmatter_string()` helper. Delete `cleaner::parse_frontmatter` and `utils::parse_frontmatter`.

**1d. Unify slug generation** — Audit each non-canonical caller. Either route through `content::slug` or add named variants there. Delete duplicates.

### Phase 2: Task type registry fix (low risk, structural)

**2a. Register 9 missing task types** in `task_definitions.rs` with correct policies (`cluster_and_link` → AutoEnqueue, etc.)

**2b. Add compile-time guard** — test that every task type with a handler is in definitions and vice versa.

**2c. Add skill to bare agentic steps** — every `StepKind::Agentic` without a skill gets one or a documented exception. `fix_404s`/`fix_redirects` get proper handler plans.

### Phase 3: Delete dead/legacy code (low risk cleanup)

**3a. Remove legacy hub page code** — Delete `hub_page.rs`, remove `create_hub_page`/`refresh_hub_page` from definitions, remove `gather_spoke_briefs()` call.

**3b. Audit social module** — Either integrate or delete the 8 dead-code-flagged files.

**3c. Delete `engine_clean.rs`** — Dead demo file.

### Phase 4: Agent invocation consolidation (structural, higher risk)

**4a. Centralize agent calls** — Add `RigAgenticContext` struct. Route 13 bypasses through central path.

**4b. Centralize prompt construction** — Add typed prompt section builders to `engine::prompts`. Domain modules compose, don't build raw strings.

**4c. Consolidate JSON extraction** — Route all callers through `engine::text::extract_json_as<T>()`. Standardize on Rig `Extractor<T>` for structured output.

### Phase 5: DB access cleanup

**5a. Eliminate raw SQL in exec/ modules** — Route through `article_index` or domain modules.

**5b. Pass connections instead of opening them** — 23 exec/ files should receive connections.

### Phase 6: Handle split

Split `ImplementationHandler` into domain-specific handlers (CTR, content fixes, indexing recovery, campaign management) so new fix types don't depend on match-arm ordering.

---

## Guard Rails (add after Phase 1)

- CI check: every `unwrap()` in production code is flagged
- CI check: no `#[allow(dead_code)]` on new modules
- CI check: no `Connection::open()` in `engine/exec/`
- CI check: no `invoke()` calls outside `tauri.ts`
- CI check: no `task_store::create_task()` outside allowlist
- Test: every task type in definitions has a non-fallback handler
- Test: every `StepKind::Agentic` in handler plans has a skill param
