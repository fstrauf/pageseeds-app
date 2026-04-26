# DX Improvement Spec

Actionable improvements to reduce friction when building new features. Ordered by impact.

---

## 1. Extract `run_step()` into a Step Registry — ✅ DONE

**Problem**: `executor.rs`'s `run_step()` is a 750+ line match statement. Every new step kind requires editing this monolith. It's the single biggest DX bottleneck — most feature work touches it and it's the hardest file to navigate.

**Solution**: Create a `StepRegistry` that maps step kind strings to handler functions. New steps become a single registration, not a match arm edit.

**Tasks**:
- [x] Define a `StepHandler` trait or function signature: `async fn(ctx: &StepContext, step: &WorkflowStep) -> StepResult`
- [x] Create `engine/step_registry.rs` with a `HashMap<StepKind, HandlerFn>`
- [x] Extract each match arm from `run_step()` into its own registered handler
- [x] Replace the match statement with a registry lookup + dispatch
- [x] Verify all existing step kinds still work (run a task from each major handler family)

**Acceptance**: ~~`run_step()` is under 50 lines.~~ `run_step()` refactored to ~30 lines of registry lookup + dispatch. Adding a new step kind requires one `handlers.insert()` call in the registry.

---

## 2. Shared Frontend Error Infrastructure — ✅ DONE

**Problem**: 20+ components independently maintain `useState<string | null>(null)` for error display. No `ErrorBoundary` in the React tree. No toast system. Each component styles its own error box. A synchronous throw crashes the app.

**Solution**: Add an `ErrorBoundary` at the app root and a shared error display mechanism (toast or notification context).

**Tasks**:
- [x] Add a React `ErrorBoundary` wrapping the main content area in `main.tsx`
- [x] Create a toast/notification system (custom `toast-context.tsx` with auto-dismiss)
- [x] Create a `useErrorHandler()` hook that provides `showError(message)` via toast
- [x] Migrate all 18 components from local `setError()` to shared `showError()` toast pattern:
  - `articles/ContentHealth.tsx`
  - `articles/CtrHealthPanel.tsx`
  - `articles/LinkingMap.tsx`
  - `gsc/GscCoverage.tsx`
  - `gsc/GscDashboard.tsx`
  - `gsc/GscIndexing.tsx`
  - `projects/ProjectModal.tsx`
  - `reddit/RedditSearch.tsx`
  - `social/CampaignCreate.tsx`
  - `tasks/KeywordPicker.tsx`
  - `tasks/RedditOpportunityPicker.tsx`
  - `tasks/TaskCreate.tsx`
  - `tasks/TaskDetail.tsx`
  - `workflow/AgentLog.tsx`
  - `workflow/BatchPanel.tsx`
  - `workflow/PromptPreview.tsx`
  - `workflow/RunHistory.tsx`
  - `workflow/SchedulerConfig.tsx`
- [x] Remove all redundant inline error `<div>` blocks

**Acceptance**: All components use `showError()` — no manual error state or inline error divs remain. App doesn't crash on unhandled throws.

---

## 3. Document the Three-Layer Split — ✅ DONE

**Problem**: Three places logic can live (`commands/`, `engine/exec/`, `engine/workflows/handlers.rs`) with no documentation explaining when to use which. Developers won't know where their code goes. `docs/dev-process.md` is referenced in AGENTS.md but doesn't exist.

**Solution**: Document the layering rules and call out `social/` as the reference implementation.

**Tasks**:
- [x] Add a "Layer Responsibilities" section to AGENTS.md with responsibility matrix
- [x] Add a "Reference Implementation" note pointing to `social/` as the canonical example of a fully modularized domain
- [x] Create `docs/dev-process.md` with worked example
- [x] Add a decision tree: "I have new logic — where does it go?"

**Acceptance**: A developer reading AGENTS.md can identify exactly which file to create for any new piece of logic without studying existing code.

---

## 4. Binding Staleness CI Check — ✅ DONE

**Problem**: If a developer changes a Rust model and forgets `./scripts/sync-bindings.sh`, TypeScript types go stale silently. No compile error, no CI failure.

**Tasks**:
- [x] Create `scripts/check-bindings.sh` that diffs src-tauri/bindings/ against src/lib/bindings/
- [x] Document the check in AGENTS.md under "Pre-Change Checklist"
- [x] Wire into CI workflow `.github/workflows/ci.yml` (`binding-check` job)

**Acceptance**: A PR that changes a Rust model without running `sync-bindings.sh` fails CI before merge.

---

## 5. Make `WorkflowStep.kind` a Rust Enum — ✅ DONE

**Problem**: `WorkflowStep.kind` is a `String`. A typo like `"deterministc"` compiles fine and only fails at runtime. No IDE autocomplete when building steps.

**Solution**: Define a `StepKind` enum with all known variants.

**Tasks**:
- [x] Define a `StepKind` enum in `engine/workflows/step_kind.rs` with 40+ variants (Deterministic, Agentic, Manual, Normalizer, RedditSearch, GscSummarise, SocialGeneratePosts, etc.)
- [x] Implement `Display`, `FromStr`, `AsRef<str>` for backward compat with existing serialized data — includes `Unknown` fallback variant for deserialization safety
- [x] Replace `kind: String` with `kind: StepKind` in `WorkflowStep`
- [x] Update all handler `plan()` methods to use enum variants
- [x] Update executor dispatch to match on enum instead of string

**Acceptance**: `cargo check` catches any invalid step kind at compile time. All existing workflows still execute correctly.

---

## 6. Move Business Logic Out of `commands/tasks.rs` — ✅ DONE

**Problem**: `create_article_tasks_from_keywords()` contains priority calculation, keyword dedup, and artifact construction — business logic that violates the thin-wrapper contract.

**Solution**: Extract business logic into `engine/keyword_selection.rs`.

**Tasks**:
- [x] Create `engine/keyword_selection.rs`
- [x] Move priority scoring (`compute_task_priority`), artifact building (`build_keyword_provenance_artifact`), and description formatting (`build_content_task_description`) into that module
- [x] Move keyword dedup/validation loop into `build_content_tasks_from_keywords()`
- [x] Reduce the command to: parse inputs → call module function → return result

**Acceptance**: `commands/tasks.rs` contains no priority calculation, no keyword normalization, no artifact JSON construction. Command is a thin wrapper.

---

## 7. Expand the Error Enum with Domain Variants — ✅ DONE

**Problem**: `error.rs` has 6 generic variants. All errors become strings via `.map_err(|e| e.to_string())` repeated 30+ times. No domain context survives for debugging.

**Solution**: Add domain-specific variants to the `Error` enum.

**Tasks**:
- [x] Add domain-specific variants to the `Error` enum: `ProjectNotFound`, `TaskNotFound`, `AuthRequired`, `InvalidTaskType`, `ConfigMissing`, `Validation`
- [x] Create `impl From<Error> for String` and `CmdResult<T>` type alias to reduce boilerplate
- [x] Migrate command files to use the new variants

**Acceptance**: Domain errors carry context (e.g., "Project not found: proj_abc123"). Command boilerplate is reduced.

---

## 8. Silent Registration Failures — ✅ DONE

**Problem**: Forgetting to register a command in `lib.rs` → no compile error, silently broken. Forgetting to add a handler to `default_handlers()` → silent fallback to `ManualFallbackHandler`.

**Solution**: Add startup self-checks and tests.

**Tasks**:
- [x] Add a startup self-check that logs registered handler and command counts at INFO level
- [x] Add a `#[cfg(debug_assertions)]` test (`all_task_types_have_non_fallback_handler`) that asserts every task type in `config::TASK_TYPES` has a non-fallback handler match
- [x] Test coverage is sufficient; no need for `inventory` crate

**Acceptance**: A missing handler for a known task type is caught by `cargo test`, not by a user clicking a button.

---

## 9. Frontend Data Refresh Pattern — ✅ DONE

**Problem**: Multiple components use `setRefreshKey(k => k + 1)` to trigger re-fetches after mutations. No cache invalidation layer.

**Solution**: Create custom `useQuery` + `useMutation` hooks with cache invalidation.

**Tasks**:
- [x] Create custom `useQuery` + `useMutation` hooks in `src/hooks/useQuery.ts` with in-memory cache + subscriber pattern + `invalidateQueries` option
- [x] Migrate TaskBoard to `useQuery` + `useMutation`
- [x] Migrate remaining components — no components still use `refreshKey`

**Acceptance**: Mutations automatically invalidate related queries. No manual refresh triggers.

---

## 10. Cleanup Migrations V11/V12 — ✅ DONE

**Problem**: V11 and V12 both create `skill_embeddings` with the same schema — a leftover from an incomplete refactor.

**Tasks**:
- [x] Make V11 a no-op with clear comment explaining it was an incomplete migration
- [x] Add a comment to V12 explaining it supersedes V11 as the canonical schema
- [x] Verify on a fresh DB and an existing DB that migrations still pass

**Acceptance**: Migration intent is clear. No developer confusion about duplicate schemas.

---

## 11. URL-Based View State — ✅ DONE

**Problem**: Reloading the app loses the active view and project selection. All routing is component state.

**Solution**: Sync `activeView` and `activeProject` to URL hash.

**Tasks**:
- [x] Sync `activeView` and `activeProject` to URL hash via `window.history.replaceState()`
- [x] Restore view state from URL on app load via `parseUrlHash()`
- [x] Ensure deep links work (e.g., `#/tasks?project=abc`)

**Acceptance**: Refreshing the app returns to the same view and project.

---

## Summary

| # | Item | Status | Remaining Work |
|---|------|--------|----------------|
| 1 | Step registry | ✅ Done | — |
| 2 | Frontend error infra | ✅ Done | All 18 components migrated to `showError()` |
| 3 | Document three-layer split | ✅ Done | — |
| 4 | Binding staleness check | ✅ Done | CI job exists and runs on PR/push |
| 5 | StepKind enum | ✅ Done | — |
| 6 | Move task business logic | ✅ Done | `commands/tasks.rs` is 145 lines, thin wrapper |
| 7 | Expand error enum | ✅ Done | — |
| 8 | Silent registration check | ✅ Done | — |
| 9 | Data refresh pattern | ✅ Done | No `refreshKey` patterns remain |
| 10 | Cleanup migrations | ✅ Done | — |
| 11 | URL-based view state | ✅ Done | — |
