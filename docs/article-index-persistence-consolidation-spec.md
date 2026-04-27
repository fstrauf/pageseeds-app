# Article Index Persistence Consolidation Spec

Status: Draft  
Created: 2026-04-28  
Owner: PageSeeds app

## Problem

PageSeeds currently uses both SQLite and `.github/automation/articles.json` as article indexes. SQLite backs much of the app UI and publishing flow, while many workflow executors still read or mutate `articles.json` directly.

That split creates source-of-truth confusion:

- UI article lists come from SQLite.
- Setup and several workflows treat `articles.json` as required runtime input.
- Some workflows mutate `articles.json` without updating SQLite.
- Some DB mutations export `articles.json`, but not consistently.
- Rich metadata such as GSC metrics and audit output often lives only in JSON artifacts.

## Decision

Make SQLite the canonical article index for app runtime behavior.

Keep `.github/automation/articles.json` as a Git-tracked import/export projection for workspace projects. It remains useful for repo history, external tools, older automation flows, and human inspection, but app workflows should not treat it as the primary runtime store.

## Non-Goals

- Do not remove `articles.json` from workspace projects.
- Do not change MDX content ownership. Source files remain the canonical article body/content store.
- Do not migrate live-site inventory into `articles`; live-site projects should continue using `live_site_pages` unless a later product decision says otherwise.
- Do not rewrite unrelated workflow orchestration.

## Target Model

### SQLite Owns Runtime Article Metadata

The app should read article records from SQLite for:

- article list UI
- workflow input context
- status transitions
- publish dates
- target keywords
- review state
- file paths
- GSC summary metrics needed by app workflows
- audit state needed for skip/retry behavior

### MDX Owns Article Content

MDX/frontmatter remains canonical for:

- article body
- page-level frontmatter content such as title/date/description when syncing from disk
- file existence and file structure

### articles.json Is A Projection

`articles.json` should be generated from SQLite and imported into SQLite through explicit sync paths.

It should be used for:

- Git-tracked article inventory snapshots
- external tooling compatibility
- manual inspection
- recovery/import when a repo has article data not yet in the local app DB

It should not be used directly by internal workflows once this migration is complete.

## Source Ownership Rules

- SQLite is authoritative for article rows during app execution.
- MDX files are authoritative for content body and file existence.
- `articles.json` is authoritative only at import time, before its values are merged into SQLite.
- Any DB mutation that changes exported fields must schedule or perform a projection export.
- Any direct file-system cleanup of article records must update SQLite first, then export JSON.
- Unknown JSON fields must be preserved until they are either migrated into schema or intentionally dropped by a documented migration.

## Current High-Risk Paths

These paths should be migrated away from direct `articles.json` access:

- `src-tauri/src/content/ops.rs`
  - `sync_and_validate`
  - `clean_stale_articles_json`
  - `ingest_orphan_files`
- `src-tauri/src/engine/exec/gsc/sync.rs`
- `src-tauri/src/engine/exec/keywords/research_pipeline.rs`
- `src-tauri/src/engine/exec/coverage.rs`
- `src-tauri/src/engine/exec/content_audit.rs`
- `src-tauri/src/engine/exec/ctr_audit/context.rs`
- `src-tauri/src/engine/exec/cannibalization_audit.rs`
- `src-tauri/src/engine/exec/content/cluster_link.rs`
- `src-tauri/src/engine/exec/content/review.rs`
- `src-tauri/src/engine/post_actions.rs`
- `src-tauri/src/content/publish.rs`

## Implementation Plan

### Phase 0: Baseline And Guardrails

- [ ] Add tests that capture the current expected import/export behavior for `articles.json`.
- [ ] Add a failing regression test for JSON-only cleanup leaving SQLite stale.
- [ ] Add a failing regression test for publish date changes patching MDX from stale JSON.
- [ ] Add a search/CI guard or lint-like test that lists direct runtime reads of `articles.json` outside approved modules.
- [ ] Update this spec with any additional direct JSON paths found during implementation.

Acceptance criteria:

- [ ] Existing behavior is covered before refactors begin.
- [ ] Approved direct JSON access is limited to import/export/projection modules and setup diagnostics.

### Phase 1: Introduce Article Index Service

Create a single backend article-index boundary, likely in `src-tauri/src/content/article_index.rs` or a similarly named domain module.

Responsibilities:

- [ ] Load workspace article records from SQLite.
- [ ] Import `articles.json` into SQLite.
- [ ] Export SQLite records to `articles.json`.
- [ ] Preserve unknown/custom JSON fields during export.
- [ ] Provide workflow-ready article summaries.
- [ ] Provide file-resolution helpers using the existing content locator/resolver.
- [ ] Provide stale-file cleanup that updates SQLite first.
- [ ] Provide orphan-file ingestion that updates SQLite first.

Suggested API shape:

```rust
pub fn list_articles(conn: &Connection, project_id: &str) -> Result<Vec<Article>>;
pub fn import_projection(conn: &Connection, project_id: &str, project_path: &Path) -> Result<ImportSummary>;
pub fn export_projection(conn: &Connection, project_id: &str, project_path: &Path) -> Result<ExportSummary>;
pub fn clean_stale_articles(conn: &Connection, project_id: &str, project_path: &Path) -> Result<CleanSummary>;
pub fn ingest_orphans(conn: &Connection, project_id: &str, project_path: &Path) -> Result<IngestSummary>;
pub fn sync_metadata_from_disk(conn: &Connection, project_id: &str, project_path: &Path) -> Result<SyncSummary>;
```

Acceptance criteria:

- [ ] Commands remain thin and call the service.
- [ ] Existing `db::export` behavior is either wrapped or moved without widening command-layer logic.
- [ ] No workflow executor needs to know the physical `articles.json` path for article records.

### Phase 2: Persist Rich Article Metadata

Decide how to represent fields currently living only in JSON/artifacts.

Option A: typed columns on `articles`:

- [ ] Add typed GSC metric columns if the app needs to query them often.
- [ ] Add typed quality summary columns if the UI should sort/filter by them.

Option B: sidecar metadata table:

- [ ] Add `article_metadata` table keyed by `(project_id, article_id, namespace)`.
- [ ] Store namespace payloads such as `gsc`, `quality`, `analytics`, and `custom` as JSON.
- [ ] Merge sidecar metadata back into `articles.json` on projection export.

Recommended first step:

- [ ] Add a sidecar metadata table for flexible JSON metadata.
- [ ] Promote only high-query fields to typed columns later.

Acceptance criteria:

- [ ] `gsc` data written by GSC sync is visible to DB-backed workflows.
- [ ] Exported `articles.json` still includes preserved custom fields.
- [ ] Import does not destroy unknown fields.

### Phase 3: Migrate Read Paths To SQLite

Replace direct workflow reads of `articles.json` with article-index service calls.

- [ ] Coverage load uses SQLite for workspace projects.
- [ ] Keyword research existing-keyword filtering uses SQLite.
- [ ] Content audit uses SQLite article records.
- [ ] CTR audit uses SQLite article records plus DB-backed GSC metadata.
- [ ] Cannibalization audit uses SQLite article records.
- [ ] Cluster/link scan and strategy use SQLite article records.
- [ ] Content review recommendation uses SQLite article records and DB review state.
- [ ] Content health checks compare SQLite article records with MDX/frontmatter.

Acceptance criteria:

- [ ] Running workflows with a stale `articles.json` but fresh SQLite uses the SQLite state.
- [ ] Running workflows with fresh `articles.json` but stale SQLite warns or requires import instead of silently using JSON.
- [ ] Direct JSON reads remain only in projection/import/export/setup diagnostics.

### Phase 4: Migrate Write Paths To SQLite First

Replace direct `articles.json` mutations with DB updates followed by projection export.

- [ ] GSC sync writes GSC metrics to SQLite/metadata table first, then exports projection.
- [ ] Stale article cleanup removes or archives SQLite rows first, then exports projection.
- [ ] Review-state changes update SQLite first, then export projection.
- [ ] Content write/orphan ingestion updates SQLite first, then export projection.
- [ ] Article path repair updates SQLite first, then export projection.
- [ ] Date fixes update SQLite first, then export projection.

Acceptance criteria:

- [ ] There is no code path where `articles.json` changes article records without the DB changing too.
- [ ] Projection export is idempotent.
- [ ] Unknown/custom fields survive round trips.

### Phase 5: Fix Publish And Frontmatter Sync Ordering

Current risk: publish updates SQLite, then patches MDX dates through a helper that reads expected dates from the old `articles.json`, then exports the new JSON.

Tasks:

- [ ] Change frontmatter date patching to use updated SQLite article records directly.
- [ ] Or export the projection before any helper that still reads JSON, then remove that dependency in a later step.
- [ ] Add a regression test where publishing assigns a new date and verifies both MDX frontmatter and `articles.json` contain that same date.
- [ ] Ensure date-policy validation runs against SQLite state.

Acceptance criteria:

- [ ] Publishing cannot leave SQLite, MDX frontmatter, and `articles.json` with different dates.
- [ ] Publish warnings distinguish projection export failures from content patch failures.

### Phase 6: Drift Detection And Sync UX

Add explicit detection when repo projection and local DB diverge.

Suggested schema additions:

- [ ] `articles_meta.last_imported_hash`
- [ ] `articles_meta.last_exported_hash`
- [ ] `articles_meta.last_synced_at`
- [ ] `articles_meta.projection_dirty`

Tasks:

- [ ] Hash `articles.json` on import.
- [ ] Hash `articles.json` on export.
- [ ] Detect external JSON changes before running workspace workflows.
- [ ] Show a clear UI warning when repo projection changed externally.
- [ ] Offer explicit actions: `Import repo index`, `Export app index`, `Review drift`.

Acceptance criteria:

- [ ] Stale DB vs stale JSON is visible to users.
- [ ] Workflows do not silently choose the wrong side when drift exists.

### Phase 7: Update Frontend Labels And Behavior

- [ ] Rename vague `Sync` UI copy to `Import repo index` where it imports JSON into SQLite.
- [ ] Rename `Export` copy to `Export app index to repo` where it writes JSON projection.
- [ ] Update setup warnings to explain that `articles.json` is a repo projection, not the app runtime store.
- [ ] Update article table empty states so missing JSON and empty DB are not conflated.
- [ ] Update publish success copy to state which stores were updated.

Acceptance criteria:

- [ ] UI language no longer implies there are two equal sources of truth.
- [ ] Users can tell whether they are importing into the app DB or exporting to the repo.

### Phase 8: Documentation Cleanup

- [ ] Update `docs/DATA_PERSISTENCE.md` from dual-source language to DB-canonical/projection language.
- [ ] Replace stale `content_automation/` references with `.github/automation/` where applicable.
- [ ] Update `CONTRACTS.md` workflow descriptions that say steps consume `articles.json`.
- [ ] Update `AGENTS.md` if the module map changes.
- [ ] Add a short architecture note explaining SQLite, MDX, and projection ownership.

Acceptance criteria:

- [ ] Docs match the implemented persistence model.
- [ ] Future agents can identify the approved article-index boundary quickly.

### Phase 9: Tests And Verification

Required backend checks:

- [ ] `cargo check`
- [ ] `cargo test`
- [ ] targeted tests for import/export round trip
- [ ] targeted tests for stale cleanup DB/JSON consistency
- [ ] targeted tests for GSC sync DB/projection consistency
- [ ] targeted tests for publish date consistency

Required frontend checks if UI copy or behavior changes:

- [ ] `pnpm run lint`
- [ ] `pnpm exec tsc -b`
- [ ] `pnpm test`
- [ ] `pnpm run build`

Acceptance criteria:

- [ ] All affected tests pass.
- [ ] No direct workflow article reads from `articles.json` remain outside the approved boundary.

## Open Decisions

- [ ] Should stale article cleanup delete rows or mark them with a status such as `missing_file`?
- [ ] Should `articles.json` export happen synchronously after every DB mutation or through a dirty flag and explicit export?
- [ ] Should GSC metrics be typed columns, sidecar JSON metadata, or both?
- [ ] Should import overwrite DB fields unconditionally, or should it use drift-aware merge rules?
- [ ] Should live-site projects ever export an `articles.json` projection, or remain DB-only?

## Rollout Strategy

Recommended rollout order:

1. Add tests and article-index boundary.
2. Fix publish/date consistency first because it can corrupt user-visible content state.
3. Migrate direct write paths next because they create stale DB rows.
4. Migrate read paths in batches by workflow family.
5. Add drift detection after import/export behavior is centralized.
6. Update UI copy and docs once behavior is stable.

## Definition Of Done

- [ ] SQLite is the only runtime source for workspace article metadata.
- [ ] MDX/frontmatter sync reads expected metadata from SQLite or an article-index service, not raw JSON.
- [ ] `articles.json` is generated from SQLite and imported through explicit sync only.
- [ ] Direct `articles.json` access is limited to import/export/projection/setup diagnostics.
- [ ] Workflow executors no longer parse `articles.json` for article input.
- [ ] GSC/review/audit metadata required by workflows is available through SQLite-backed APIs.
- [ ] Drift between local DB and repo projection is visible and actionable.
- [ ] Documentation and UI copy describe one source of truth clearly.