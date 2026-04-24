# Live Site Project Mode Spec

**Status:** Proposed  
**Date:** 2026-04-23

## Problem

PageSeeds is currently optimized for developer-operated websites.

The strongest workflows already exist:
- keyword research
- SEO analysis
- GSC integration
- content quality analysis

But the product assumes the user can:
- point the app at a local repository
- maintain a markdown content directory
- keep `.github/automation` files on disk
- treat `articles.json` as the source of truth
- manually publish generated content into their website stack

That makes the app powerful for internal use, but awkward for a normal website owner who has:
- a live website
- little or no familiarity with repos
- no desire to manage markdown files locally
- interest in SEO outcomes, not implementation details

We need a product path that supports non-developer users without breaking the existing repo-based workflow that is already valuable for developer users.

---

## Product Direction

Do **not** fork the app into separate developer and non-developer products yet.

Instead, introduce **two project modes** inside the same product:

1. **Workspace Project**
   Existing repo-backed mode.
   Uses local files, markdown content, `.github/automation`, and `articles.json`.

2. **Live Site Project**
   New mode for normal users.
   Uses a website URL, sitemap discovery, page crawling, optional GSC, and later CMS integrations.

This preserves the current developer workflow while making room for a friendlier onboarding and a hosted-service model.

---

## Core Decision

The main architectural change is:

**`articles.json` + local markdown files must stop being the universal source of truth.**

They remain one valid content source for `workspace` projects, but we add a normalized internal inventory layer so both project modes can run the same SEO workflows.

### New abstraction

Introduce a normalized **site inventory** model stored in SQLite.

Each inventory record should represent one page/article and include, at minimum:
- canonical URL
- slug/path
- title / H1
- meta description when available
- publish date / modified date when available
- headings
- cleaned body text or excerpted content
- internal links out
- internal links in (derived)
- detected page type
- target keyword if known
- source type (`workspace_markdown`, `live_crawl`, `cms_api`, etc.)
- source identifier (file path, URL, CMS ID)
- sync status / last synced time

The current repo-based flow becomes one adapter that populates this inventory.
The new live-site flow becomes another adapter.

---

## Goals

### Primary goals

- Let a non-developer user start with just a website URL.
- Analyze an existing live website without requiring a local repo.
- Reuse as much of the existing keyword research and SEO logic as possible.
- Keep workspace projects fully supported for developer users.
- Create a roadmap toward hosted API-backed and subscription-backed operation.

### Secondary goals

- Make the UI outcome-oriented instead of setup-oriented.
- Support draft export before direct CMS publishing exists.
- Make GSC and provider credentials understandable for non-technical users.

### Non-goals for the first version

- Fully automatic publishing to every CMS
- Browser extension or plugin ecosystem
- Multi-tenant cloud backend in the first milestone
- Replacing the current developer workflow

---

## User Experience Shift

### Current setup

The user is asked for:
- a local path
- a content directory
- workspace initialization
- automation files

### Target setup for live-site users

The user is asked for:
- website URL
- sitemap URL if auto-detection fails
- optional GSC connection
- optional CMS connection later
- optional content generation API key if we do not proxy requests server-side yet

### Current mental model

Projects, tasks, files, setup checks, repo state.

### Target mental model

Site, pages, issues, opportunities, drafts, publish/export.

### UX principle

Normal users should see actions like:
- Import Site
- Connect Search Console
- Find Keyword Opportunities
- Review Existing Pages
- Find Internal Link Gaps
- Generate Article Draft
- Export Draft
- Publish Draft

Developer-specific concepts should move behind:
- Advanced Settings
- Workspace Project mode
- Developer labels and diagnostics

---

## Proposed Architecture

### Project modes

Add a project mode field:
- `workspace`
- `live_site`

`workspace` projects continue to require a local path.

`live_site` projects require:
- site URL
- optional sitemap URL
- optional GSC property ID
- optional CMS connector settings

### Content source adapters

Implement a `content source` abstraction with at least these adapters:

1. **Workspace adapter**
   Reads markdown and `articles.json` from the existing local workspace.

2. **Live site adapter**
   Fetches sitemap URLs and crawls selected pages from the live site.

3. **CMS adapter**
   Future phase. Pulls content via WordPress, Webflow, Shopify, Ghost, etc.

### Shared site inventory

All adapters populate a common SQLite inventory so downstream workflows can run against a stable internal model.

### Workflow strategy

Refactor workflows into:
- inventory/source loading
- structured deterministic analysis
- optional agentic interpretation or writing
- output destination handling

This keeps the SEO engine reusable across both project modes.

---

## Feature Scope

### In scope

- Create `live_site` projects without a repo path
- Import a site from sitemap + crawl
- Build page inventory in SQLite
- Analyze existing pages and internal linking from live pages
- Run keyword research for both project modes
- Overlay GSC data onto live-site inventory
- Generate article drafts for non-developer users
- Export drafts in usable formats before direct publishing exists
- Preserve repo-based workflows for current users

### Out of scope for the first implementation wave

- Full server-hosted multi-user SaaS control plane
- Billing implementation
- Team permissions
- Automatic publishing integrations for every CMS
- Full visual site crawler beyond sitemap + page fetch

---

## Workstreams

## 1. Data Model

We need a data model that can represent both repo-backed and live-site-backed content.

### Tasks

- [x] Add `project_mode` to the `projects` table and Rust/TypeScript models
- [ ] Add optional `sitemap_url` and `cms_type` fields for live-site projects
- [x] Introduce a normalized `site_inventory_pages` table in SQLite
- [x] Introduce a `site_inventory_links` table or equivalent derived structure
- [ ] Track source metadata for each inventory record (`file path`, `url`, `cms id`, `source type`)
- [ ] Track last sync timestamps and sync status per page
- [x] Define a stable internal struct for a normalized page/article record
- [ ] Keep existing `articles` table working during migration
- [ ] Decide whether `articles` becomes a compatibility cache, generated view, or legacy-only store

### Acceptance

- [ ] Both project modes can store page-level content inventory without requiring markdown files

---

## 2. Project Setup and Onboarding

We need separate onboarding flows for developer and non-developer users.

### Tasks

- [x] Update the project creation UI to let the user choose `Workspace Project` or `Live Site Project`
- [x] Keep the existing local-path form for `workspace` mode
- [x] Add a URL-first onboarding form for `live_site` mode
- [x] Add automatic sitemap detection for live-site setup
- [x] Add manual sitemap override when detection fails
- [ ] Add first-run validation tailored to live-site projects
- [x] Hide repo/workspace setup warnings for live-site projects
- [ ] Rename user-facing setup copy away from developer jargon where possible
- [ ] Move advanced diagnostics into an expandable advanced section

### Acceptance

- [x] A non-developer can create a project with only a website URL and no filesystem path

---

## 3. Live Site Import

We need a deterministic path from website URL to usable site inventory.

### Tasks

- [x] Reuse or extend sitemap fetching logic for live-site setup
- [x] Add a native `import_live_site` command/workflow
- [x] Fetch sitemap URLs and normalize them to the project domain
- [x] Crawl a bounded set of pages safely with timeouts and content-type checks
- [x] Extract page title, headings, text content, meta description, and canonical URL
- [x] Extract internal links from each crawled page
- [x] Store results into the shared site inventory tables
- [ ] Add incremental resync support so later imports do not rebuild everything unnecessarily
- [ ] Expose an import summary in the UI: pages found, pages imported, failures, skipped URLs
- [ ] Handle common failure modes clearly: blocked sitemap, no sitemap, non-HTML pages, timeouts, redirects

### Acceptance

- [x] A live-site project can build a usable page inventory from its sitemap and crawled HTML

---

## 4. Existing Content Analysis

The current content workflows rely heavily on local markdown and `articles.json`.
We need live-site equivalents that operate on inventory pages.

### Tasks

- [ ] Refactor readability analysis to accept normalized page content, not only local files
- [x] Refactor internal linking analysis to run on inventory link graphs
- [x] Refactor content health checks so they can run on live-site inventory where applicable
- [x] Add a page-level overview UI for imported site pages
- [x] Add filters for thin content, missing metadata, weak headings, stale pages, and weak interlinking
- [x] Distinguish deterministic facts from agentic recommendations in the UI
- [x] Keep the richer workspace-only checks available when local files exist

### Acceptance

- [x] Live-site users can audit existing pages without needing markdown or `articles.json`

---

## 5. Keyword Research and Opportunity Mapping

Keyword research is already one of the strongest parts of the product.
The main change is attaching opportunities to live-site inventory rather than only repo content.

### Tasks

- [x] Ensure keyword research flows can run for both project modes
- [x] Replace `articles.json`-only coverage assumptions with coverage derived from the shared site inventory
- [ ] Add URL/page matching so opportunities map to existing live pages where possible
- [ ] Mark opportunities as `new article`, `refresh existing page`, or `landing page` based on inventory coverage
- [x] Keep developer workflows and task creation intact for workspace mode
- [ ] Design a simpler opportunity review UI for non-developer users

### Acceptance

- [x] Keyword research works for live-site projects without a local article registry

---

## 6. GSC for Non-Developer Projects

GSC is useful to both modes, but the setup and data application should feel simpler for live-site users.

### Tasks

- [x] Allow GSC to attach directly to live-site projects with no repo manifest dependency
- [x] Persist site URL / property metadata in the project record or project config tables
- [x] Sync Search Analytics data into the shared site inventory
- [ ] Sync URL inspection results against live-site URLs
- [ ] Surface indexing and performance issues in site/page language rather than file/workspace language
- [x] Reuse existing GSC internals where possible

### Acceptance

- [x] A live-site project can connect GSC and annotate imported pages with performance data

---

## 7. Draft Generation and Output Handling

For non-developer users, content generation must not end at a markdown file sitting on disk.

### First output model

Before direct publishing exists, support:
- markdown export
- HTML export
- rich text / copy-friendly export
- draft preview in app
- clipboard copy

### Later output model

Add CMS draft publishing for selected platforms.

### Tasks

- [ ] Decouple article generation from repo file creation
- [ ] Create a normalized `draft output` model stored in SQLite
- [ ] Add export actions for markdown, HTML, and copy-to-clipboard
- [ ] Add a draft preview screen for non-technical users
- [ ] Add a “create CMS draft” abstraction separate from “publish live”
- [ ] Prototype first CMS integration target and choose rollout order
- [ ] Preserve existing write-to-workspace behavior for developer projects

### Acceptance

- [ ] A non-developer user can generate a draft and do something useful with it without touching a repo

---

## 8. Product and UI Simplification

The app currently exposes a lot of implementation detail.
Live-site users need a clearer, more guided surface.

### Tasks

- [ ] Add a simplified project overview for live-site projects
- [ ] Group views into outcome-oriented language: Site Audit, Opportunities, Pages, Drafts, Search Console, Settings
- [ ] Reduce exposure of task engine details in the default live-site UX
- [ ] Keep advanced task views available for power users and workspace projects
- [ ] Replace developer-centric copy like `repo root`, `content_dir`, and `articles.json` in the live-site flow
- [ ] Add guided empty states and next-step prompts after import, after GSC connect, and after draft generation

### Acceptance

- [ ] A non-developer can understand the product without knowing what a repo or markdown content directory is

---

## 9. Hosted / Subscription Readiness

This does not need to ship in the first milestone, but the architecture should not block it.

### Product direction

Two service models should remain possible:

1. **Local-first desktop mode**
   The user brings their own API keys.

2. **Hosted subscription mode**
   The app calls PageSeeds-managed servers for research, generation, and possibly publishing connectors.

### Tasks

- [ ] Identify which provider calls can remain local and which are better proxied through PageSeeds services
- [ ] Define a provider abstraction that can route requests to either direct APIs or PageSeeds-managed backend services
- [ ] Separate user credentials from provider configuration in the UI
- [ ] Avoid baking repo/file assumptions into future hosted flows
- [ ] Define a migration path from local API keys to subscription-backed service usage

### Acceptance

- [ ] The architecture supports both local API usage and future hosted service routing

---

## 10. Compatibility and Migration

This work must preserve existing value for developer users.

### Tasks

- [ ] Keep all current workspace project flows functional during the transition
- [ ] Add compatibility layers where workflows currently assume `articles.json`
- [ ] Migrate shared logic toward inventory-based inputs one workflow at a time
- [ ] Mark which features are `workspace only`, `live_site only`, or `both`
- [ ] Add targeted migration notes to affected specs and docs as implementation starts

### Acceptance

- [ ] Existing repo-backed projects still work while live-site functionality is added incrementally

---

## Recommended Implementation Order

### Phase 1: Foundation

- [x] Add `project_mode` and live-site project creation
- [x] Add shared site inventory tables and models
- [x] Add sitemap import and bounded page crawl
- [x] Show imported pages in the UI

### Phase 2: Read-only Value

- [x] Run page analysis and internal linking on live inventory
- [x] Connect GSC to live-site projects
- [ ] Map keyword opportunities against live-site inventory

### Phase 3: Drafts

- [ ] Generate drafts into a normalized draft store
- [ ] Add export and preview flows

### Phase 4: Publishing Integrations

- [ ] Add CMS draft connectors
- [ ] Add optional direct publish flow where safe

### Phase 5: Hosted Service Readiness

- [ ] Add service routing abstractions for subscription-backed provider calls

---

## First Milestone Definition

The first milestone should be intentionally narrow.

### Milestone: `live_site_read_only`

Ship a version where a non-developer can:
- create a project from a site URL
- import pages from a sitemap
- see an inventory of pages
- connect GSC
- run keyword research
- identify content gaps and weak internal linking

The first milestone does **not** need:
- CMS publishing
- server-side subscriptions
- complete parity with every workspace-only content workflow

### Milestone tasks

- [x] Add live-site project mode to project model and onboarding
- [x] Add sitemap import and crawl pipeline
- [x] Add shared inventory persistence
- [x] Add live-site pages UI
- [x] Add live-site compatible audit views
- [x] Add GSC-to-inventory sync
- [ ] Add keyword opportunity mapping to inventory pages

---

## Risks

### Risk 1: Scope explosion

Trying to solve crawling, CMS publishing, hosted billing, and UX redesign simultaneously will stall delivery.

### Risk 2: Hidden `articles.json` coupling

Many workflows currently assume repo files exist. That coupling will need to be removed carefully and incrementally.

### Risk 3: Weak crawl fidelity

Some sites render content poorly without JS execution. The first live-site import should prefer sitemap-driven HTML extraction and accept that some sites will need CMS integrations later.

### Risk 4: UI duplication

If we build a second parallel UI everywhere instead of shared components with mode-specific behavior, the maintenance cost will rise quickly.

---

## Open Questions

- [ ] Should `live_site` projects allow an optional local export folder, or should exports remain app-managed only?
- [ ] What should be the first CMS integration target: WordPress, Webflow, Shopify, or Ghost?
- [ ] Should GSC be required, recommended, or optional for live-site onboarding?
- [ ] Do we want to keep the task engine visible to non-developer users at all, or only surface generated recommendations and actions?
- [ ] Should provider API keys remain user-managed in the first non-developer version, or should that wait for hosted routing?

---

## Success Criteria

- [ ] A non-developer user can create a project from a URL with no repo path
- [ ] The app can build a page inventory from a live sitemap and crawl
- [x] Existing-page analysis works on live-site content
- [ ] Keyword opportunity mapping works without `articles.json`
- [ ] GSC data can attach to live-site pages
- [ ] Draft generation produces exportable output without requiring markdown file workflows
- [ ] Existing workspace projects remain fully supported
