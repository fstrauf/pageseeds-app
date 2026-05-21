# Unified SEO Audit — Feature Specification

## Problem

PageSeeds has four separate audit tasks (`content_audit`, `ctr_audit`, `cannibalization_audit`, `indexing_health_campaign`) that produce structured data. The Health Dashboard reads their artifacts and tries to compose a unified view. But the composition is frontend-only — there is no backend synthesis step that connects the dots.

The prototype investigation of daystoexpiry.com found that the most valuable insights came from **cross-referencing data sources**. The template rewrites source titles. Pages 404 but remain in the sitemap. Six articles share identical body content. These are individually detectable by existing checks, but no single step says: "Your template is rewriting titles AND 6 pages 404 AND your sitemap is stale — fix the template first, then clean up the orphans."

The existing "Run Full Audit" button creates two separate tasks and displays their independent outputs. There is no unified synthesis.

## Three Detection Layers

### Layer 1 — Source Content (already built: `content_audit`)

Deterministic. Reads MDX frontmatter + body. No network calls.

| Check | Status |
|---|---|
| Missing/empty title | ✅ In `content_audit` |
| Title >60 chars (SERP truncation) | ✅ |
| Token duplication ≥2× (brand dup, keyword stuffing) | ✅ (fixed from ≥3) |
| Literal template variables (`\| Brand \|`, `{Brand}`) | ✅ |
| Temporal URLs (month/year/seasonal in slug) | ✅ |
| Page bloat (file size, images, tables, code blocks) | ✅ |
| Exact duplicate body content (SHA-256) | ✅ |
| Missing meta description | ✅ |
| Readability, passive voice | ✅ |
| Missing H2 structure | ✅ |
| Internal links count, broken links | ✅ |

**→ 21 checks. Deterministic. Writes `content_audit.json`.**

### Layer 2 — Rendered SERP (partially built: `ctr_audit`)

Fetches live HTML, extracts rendered `<title>`, `<meta>`, compares to source.

| Check | Status |
|---|---|
| Source title ≠ rendered title | ✅ `CtrRenderedSerpAudit` |
| Source meta ≠ rendered meta | ✅ |
| Brand duplicated in rendered title | ✅ `CtrTemplateDetect` |
| Pages returning HTTP 404 | ❌ Not surfaced — crawl skips errors silently |
| Template rewrites titles (not just appends brand) | ❌ No similarity score between source and rendered |
| SSR fallback / error-page detection | ✅ Partial |
| FAQ schema missing from rendered page | ✅ |

**→ Needs: 404 surfacing, source-vs-rendered title similarity score.**

### Layer 3 — Site Architecture (built but fragmented)

Reads GSC, sitemap, link graph, indexing data.

| Check | Where |
|---|---|
| Cannibalization clusters | `cannibalization_audit` |
| Sitemap orphans | `indexing_diagnostics` |
| Missing redirects | `consolidate_cluster` |
| Orphaned articles (no incoming links) | `interlinking` |
| Indexing status | `indexing_health_campaign` |
| CTR underperformance | `ctr_audit` |
| GSC plateau detection (period-over-period stagnation) | ❌ Not built |

**→ Needs: plateau detection, unified 404 surfacing.**

---

## Proposed Architecture

### Single Task: `seo_audit`

A unified task that runs all layers as steps, ending with an agentic synthesis.

```
seo_audit
  ├─ Step 1 (deterministic): content_audit
  │     Runs 21 checks on all published articles
  │     Writes content_audit.json
  │
  ├─ Step 2 (deterministic): rendered_serp_audit
  │     Crawls live HTML for all article URLs
  │     Extracts rendered title, meta, canonical, h1, schema
  │     Compares to source frontmatter
  │     NEW: surfaces HTTP 404s as structured data
  │     NEW: computes title similarity score (source vs rendered)
  │     Writes rendered_serp_audit.json
  │
  ├─ Step 3 (deterministic): site_architecture
  │     Reads cannibalization_strategy.json (cached, or runs inline if stale)
  │     Reads sitemap + indexing status
  │     Reads internal link graph
  │     Reads GSC movers (period-over-period)
  │     NEW: detects page-level GSC plateau (flat impressions >90 days)
  │     Writes site_architecture.json
  │
  └─ Step 4 (agentic): synthesize_findings
        Input: content_audit.json + rendered_serp_audit.json + site_architecture.json
        Skill: "seo-audit-synthesis"
        Output contract: structured SeoAuditReport with priority-ranked findings
        Agent connects dots across layers:
          - "Template is rewriting all titles AND 6 pages 404 — fix template first"
          - "Articles A, B, C share identical body content — consolidate or redirect"
          - "Impressions flat for 90 days despite 150 articles — cannibalization likely"
        Writes seo_audit_report.json
```

### Follow-Up Actions

After synthesis, `post_actions.rs` spawns fix tasks:

| Finding | Fix Task |
|---|---|
| Template bugs (title rewrite, brand dup) | `fix_ctr_site_template` |
| 404 pages | `fix_404s` |
| Cannibalization clusters | `consolidate_cluster` (from user selection) |
| Content quality issues | `fix_content_article` (per article) |
| Missing redirects | Feature spec to target repo |
| Code-level issues (framework files) | Feature spec to target repo |

### Data Flow

```
Run Full Audit (user clicks button or scheduler triggers)
         │
         ▼
┌─────────────────────────────────────────────────────────┐
│ Step 1: content_audit (deterministic, ~2s for 150 pages) │
│   → .github/automation/content_audit.json                │
├─────────────────────────────────────────────────────────┤
│ Step 2: rendered_serp_audit (deterministic, ~30s crawl)  │
│   → .github/automation/rendered_serp_audit.json          │
│   NEW fields: 404_urls, title_similarity_score           │
├─────────────────────────────────────────────────────────┤
│ Step 3: site_architecture (deterministic, ~5s)           │
│   → .github/automation/site_architecture.json            │
│   Reads: cannibalization_strategy.json, indexing status   │
│   NEW: plateau_detection, orphaned_articles               │
├─────────────────────────────────────────────────────────┤
│ Step 4: synthesize_findings (agentic, ~20s)               │
│   → .github/automation/seo_audit_report.json              │
│   Skill: "seo-audit-synthesis"                            │
│   Agent reads all 3 JSON files, finds cross-layer patterns│
└─────────────────────────────────────────────────────────┘
         │
         ▼
Health Dashboard reads seo_audit_report.json
  → Priority issues panel (top 5 findings)
  → Layer breakdowns (content, rendered, architecture)
  → Diff vs previous audit
  → "Ask AI" panel for follow-up investigation
```

---

## New Code Required

### Backend

| File | Change | Lines |
|---|---|---|
| `config/task_definitions.rs` | Add `seo_audit` task definition | ~15 |
| `engine/workflows/step_kind.rs` | Add `SeoAuditRendered`, `SeoAuditArchitecture`, `SeoAuditSynthesize` | ~10 |
| `engine/workflows/handlers.rs` | Add `SeoAuditHandler` planning 4 steps | ~40 |
| `engine/step_registry.rs` | Register new step kinds | ~10 |
| `engine/exec/seo_audit/mod.rs` | New module | ~5 |
| `engine/exec/seo_audit/rendered.rs` | New — wraps `compare_rendered_titles`, adds 404 surfacing + similarity score | ~100 |
| `engine/exec/seo_audit/architecture.rs` | New — reads cannibalization + indexing + link graph + GSC movers, adds plateau detection | ~120 |
| `engine/exec/seo_audit/synthesize.rs` | New — agentic: reads all 3 JSON files, runs LLM synthesis with "seo-audit-synthesis" skill | ~80 |
| `engine/post_actions.rs` | Add `seo_audit` success hook — spawns fix tasks from synthesis output | ~40 |
| `.github/skills/seo-audit-synthesis/SKILL.md` | New — skill: output contract, analysis rules, cross-referencing instructions | ~60 |
| `commands/seo_audit.rs` | Thin command wrapper: spawns `seo_audit` task | ~20 |

### Changes to Existing Files

| File | Change | Lines |
|---|---|---|
| `engine/exec/ctr_audit/rendered.rs` | Make `compare_rendered_titles` write to JSON file (not just return inline) | ~20 |
| `engine/exec/content_audit.rs` | None — already produces complete output | 0 |
| `HealthDashboard.tsx` | Read `seo_audit_report.json` as primary data source (still fall back to individual artifacts) | ~30 |

### Removed / Superseded

| File | Reason |
|---|---|
| `commands/health.rs` `run_health_audit` | Superseded by `seo_audit` task |
| `docs/SEO_AUDIT_ENGINE_SPEC.md` | Superseded by this spec |
| `docs/SEO_AUDIT_INTEGRATION_PLAN.md` | Already superseded; delete |
| `docs/SEO_AUDIT_FRONTEND_SPEC.md` | Superseded by this spec |

### Total: ~550 new lines. 1 new task type. 4 new step kinds. No new business logic — all steps wrap existing functions.

---

## What Does NOT Change

- **The CLI tools** (`pageseeds-cli`) remain for ad-hoc KimiCode investigation
- **Individual tasks** (`content_audit`, `ctr_audit`, etc.) remain for targeted runs
- **The InvestigationPanel** ("Ask AI") stays in the Health Dashboard
- **Feature spec generation** stays in `post_actions.rs`

The new `seo_audit` task is a **composition** of existing capabilities with an **agentic synthesis** step added at the end. It does not replace anything — it adds the unified view.

---

## Skill: `seo-audit-synthesis`

```
You are an SEO audit synthesizer. You receive three structured JSON inputs:

1. content_audit.json — 21 deterministic checks per article (source content health)
2. rendered_serp_audit.json — live HTML crawl results (what Google actually sees)
3. site_architecture.json — cannibalization, indexing, links, GSC plateaus

Your job: find connections BETWEEN these layers that individual checks miss.

Cross-referencing rules:
- If rendered title ≠ source title AND the template is rewriting titles → flag as "title control gap" (content team writes one thing, Google sees another)
- If articles have 404 status AND they appear in sitemap → flag as "sitemap hygiene" (Google indexing dead pages)
- If GSC impressions are flat for >90 days AND cannibalization clusters exist → flag as "cannibalization stall"
- If articles share identical body content AND have different URLs → flag as "duplicate content dilution"
- If 72/150 titles are >60 chars AND template rewrites are happening → flag as "double truncation" (source already long, template makes it worse)

Output: ranked priority list. Each finding has:
- title, severity, description, evidence (which layers support it), fix_type, suggested_task
```

---

## Success Metrics

1. `seo_audit` runs end-to-end in <90 seconds for a 150-page site
2. The agentic synthesis step finds cross-layer patterns (template rewrite + 404 + plateau = prioritized fix order)
3. The "Run Full Audit" button in the Health Dashboard runs this single task
4. Previous audit reports are diffed (new/resolved/worsened findings)
5. Post-actions spawn the correct fix tasks based on synthesis output
6. Users get a single prioritized list, not four separate reports

---

## Related Docs

- `AGENTIC_INVESTIGATION_SPEC.md` — the CLI tools and investigation panel (ad-hoc exploration)
- `content_audit.rs:1` — existing 21-check deterministic audit
- `ctr_audit/rendered.rs:1` — existing rendered SERP audit
- `cannibalization_audit.rs:1` — existing cannibalization pipeline
- `indexing_health_campaign.rs:1` — existing indexing health checks
