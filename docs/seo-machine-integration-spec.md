# SEO Machine Integration Spec

## Overview

Integrate proven SEO analysis modules from [SEO Machine](https://github.com/TheCraigHewitt/seomachine) into PageSeeds to enhance keyword research, content quality assessment, and topic clustering — without requiring new data sources or external APIs.

---

## Problem Statement

Current gaps in PageSeeds:

1. **Keyword research lacks intent classification** — Users see volume and KD but must manually judge if a keyword needs a blog post vs landing page
2. **Content quality is pass/fail** — The 13-rule audit identifies issues but doesn't provide an overall quality score or publishing readiness signal
3. **Topic clusters lack authority scoring** — Coverage analysis shows article counts but not which clusters are strong vs weak
4. **No priority scoring for keywords** — Users must mentally calculate volume vs KD tradeoffs

---

## Phased Implementation

### Phase 1: Search Intent Classification

**Goal:** Automatically classify keyword intent and display intent badges in the keyword picker.

**What it does:**
- Analyzes keyword text patterns to classify as: `informational`, `commercial`, `transactional`, `navigational`
- Provides confidence score (0-100)
- Enables intent-based filtering in UI

**Ported from:** `search_intent_analyzer.py`

**Implementation:**

| Component | File | Change |
|-----------|------|--------|
| Intent Classifier | `engine/exec/intent_classifier.rs` | New module with pattern matching |
| Research Models | `models/research.rs` | Add `intent: Option<String>` and `intent_confidence: Option<f64>` to `ScoredKeyword` |
| Keyword Pipeline | `engine/exec/keywords.rs` | Call classifier after keyword discovery, before final selection |
| Keyword Picker | `components/tasks/KeywordPicker.tsx` | Add intent badges and filter dropdown |

**Signal Patterns:**
```rust
// Informational: what, why, how, guide, tutorial, learn, tips
// Commercial: best, top, vs, comparison, alternative, review
// Transactional: buy, price, discount, free trial, subscribe
// Navigational: login, website, official, dashboard

// Question patterns boost informational score
// List patterns ("10 best", "top 5") boost commercial score
```

**UI Changes:**
- Intent badge per keyword (color-coded: blue=informational, green=commercial, orange=transactional, gray=navigational)
- Filter dropdown: "All Intents" | "Informational (Blog)" | "Commercial (Comparison)" | "Transactional (Landing)"

**Dependencies:** None — pure text analysis

**Estimated Effort:** 1 day

---

### Phase 2: Content Quality Rating

**Goal:** Provide comprehensive SEO quality scores (0-100) for articles with publishing readiness gates.

**What it does:**
- Rates content across 6 categories: content, keywords, meta, structure, links, readability
- Provides overall score (0-100) and letter grade (A-F)
- Identifies critical issues, warnings, and suggestions
- Determines publishing readiness (score ≥80 + no critical issues)

**Ported from:** `seo_quality_rater.py`

**Implementation:**

| Component | File | Change |
|-----------|------|--------|
| Quality Rater | `engine/exec/quality_rater.rs` | New module with scoring logic |
| Article Models | `models/article.rs` | Add quality fields to `Article` struct |
| Content Audit | `engine/exec/content_audit.rs` | Call rater and merge results into audit artifact |
| Content Health | `components/articles/ContentHealth.tsx` | Show quality scores and readiness badges |

**Scoring Categories:**

| Category | Weight | Checks |
|----------|--------|--------|
| Content | 20% | Word count, paragraph length |
| Keywords | 25% | Density, H1/H2 placement, first 100 words |
| Meta | 15% | Title/description length, keyword presence |
| Structure | 15% | H1 count, H2 sections, hierarchy |
| Links | 15% | Internal/external link counts |
| Readability | 10% | Sentence length, list usage |

**Publishing Readiness Criteria:**
- Overall score ≥ 80
- Zero critical issues
- Keyword in H1 and first 100 words
- At least 3 internal links
- Meta title 50-60 chars, description 150-160 chars

**UI Changes:**
- Quality score column in article table (circular progress + grade)
- Publishing readiness badge ("Ready" / "Needs Work")
- Expandable breakdown showing category scores
- Critical issues alert blocking publish action

**Triggered By:** `content_review` and `content_audit` workflows

**Dependencies:** None — analyzes existing MDX files

**Estimated Effort:** 1.5 days

---

### Phase 3: Topic Clustering Enhancement

**Goal:** Add authority scoring and gap identification to existing keyword coverage analysis.

**What it does:**
- Calculates authority score (0-100) for each topic cluster
- Classifies authority level: Strong (75-100), Moderate (50-74), Weak (25-49), Minimal (0-24)
- Identifies coverage gaps (related keywords not yet covered)
- Provides recommended action per cluster

**Ported from:** `research_topic_clusters.py` (simplified — no ML clustering, enhance existing coverage logic)

**Implementation:**

| Component | File | Change |
|-----------|------|--------|
| Coverage Analysis | `engine/exec/coverage.rs` | Add authority calculation and gap detection |
| Article Models | `models/article.rs` | Add cluster metadata fields |
| Keyword Coverage | `components/seo/KeywordCoverage.tsx` | Show authority levels and recommendations |

**Authority Score Formula:**
```
Coverage (50%):  How many keywords in cluster
  50+ keywords = 100
  30-49 = 80
  15-29 = 60
  8-14 = 40
  4-7 = 20
  <4 = 10

Position (30%): Average ranking position
  ≤5 = 100
  ≤10 = 80
  ≤20 = 60
  ≤30 = 40
  ≤50 = 20
  >50 = 10

Demand (20%): Total impressions
  10,000+ = 100
  5,000-9,999 = 80
  2,000-4,999 = 60
  1,000-1,999 = 40
  500-999 = 20
  <500 = 10
```

**Gap Detection:**
- For each cluster's primary keyword, fetch related keywords (from existing Ahrefs integration)
- Compare against existing articles
- Return top 10 uncovered keywords per cluster

**Recommended Actions:**
| Authority Level | Action |
|-----------------|--------|
| Strong (75-100) | Maintain and expand |
| Moderate (50-74) | Strengthen coverage |
| Weak (25-49) | Build comprehensive cluster |
| Minimal (0-24) | Major opportunity or ignore |

**UI Changes:**
- Authority badge per cluster (color-coded)
- Sort clusters by authority level (weakest first)
- Expandable gap list showing uncovered keywords
- "Create article" button for gap keywords

**Dependencies:** Existing GSC sync for position/impression data

**Estimated Effort:** 1 day

---

### Phase 4: Opportunity Scoring (Low Priority / Future)

**Status:** Documented but deferred pending GSC integration improvements.

**Goal:** Provide unified priority score for keywords using volume, intent, and position data.

**Why Deferred:**
- Full implementation requires robust GSC position data for existing content
- Current simplified version (Volume + Intent + KD) provides limited value over manual filtering
- Better to ship Phases 1-3 first and validate value

**Future Implementation (when ready):**

| Component | File | Change |
|-----------|------|--------|
| Opportunity Scorer | `engine/exec/opportunity_scorer.rs` | New module |
| Research Models | `models/research.rs` | Add opportunity fields to `ScoredKeyword` |
| Keyword Picker | `components/tasks/KeywordPicker.tsx` | Sort and filter by opportunity score |

**Simplified 3-Factor Score:**
```
Volume Score (40%): Based on monthly search volume buckets
Intent Score (30%): Commercial=100, Informational=70, Transactional=80, Navigational=30
Competition Score (30%): 100 - KD (inverse difficulty)

For existing content (requires GSC):
Position Score (20%): Proximity to page 1
CTR Gap Score (10%): Underperformance vs expected CTR
```

**Priority Buckets:**
- CRITICAL (80-100): Immediate action
- HIGH (65-79): Strong opportunity
- MEDIUM (45-64): Moderate value
- LOW (25-44): Background priority
- SKIP (0-24): Not worth pursuing

---

## Data Requirements

| Phase | Requires | Available? |
|-------|----------|------------|
| 1. Intent Classification | Keyword text only | ✅ Yes |
| 2. Quality Rating | MDX file content | ✅ Yes |
| 3. Clustering Enhancement | GSC position + impressions | ✅ Partial (needs sync) |
| 4. Opportunity Scoring | GSC position for existing content | ⚠️ Needs robust GSC integration |

---

## Implementation Order

**Recommended Sequence:**

1. **Phase 2 (Quality Rating)** — Standalone, no dependencies, immediate value for content review
2. **Phase 1 (Intent Classification)** — Feeds into opportunity score if/when implemented
3. **Phase 3 (Clustering Enhancement)** — Builds on existing coverage analysis
4. **Phase 4 (Opportunity Scoring)** — After GSC integration is more robust

---

## Success Metrics

| Phase | Metric | Target |
|-------|--------|--------|
| 1 | Keywords with intent classified | 100% of research results |
| 1 | User filter usage | Intent filter used in 30%+ of research sessions |
| 2 | Articles with quality scores | 100% after content_review runs |
| 2 | Publishing readiness accuracy | <10% false positives (marked ready but underperforms) |
| 3 | Clusters with authority levels | 100% of clusters in coverage report |
| 3 | Gap keyword click-through | 20% of gap keywords clicked to create article |

---

## Open Questions

1. **Quality Rating Timing:** Should we also rate articles immediately after `write_article` generation, or only during review workflows?

2. **Intent in Prompts:** Should we update `final_selection_keywords.md` to explicitly consider intent when selecting keywords?

3. **Clustering ML:** Should we eventually implement ML-based clustering (TF-IDF + K-means) vs the current rule-based approach?

---

## Changelog

### 2026-04-07
- Initial spec created
- Phases 1-3 approved for implementation
- Phase 4 documented but deferred

---

## References

- SEO Machine Repository: https://github.com/TheCraigHewitt/seomachine
- Ported Modules:
  - `search_intent_analyzer.py` → Phase 1
  - `seo_quality_rater.py` → Phase 2
  - `research_topic_clusters.py` → Phase 3
  - `opportunity_scorer.py` → Phase 4 (deferred)
