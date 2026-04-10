# SEO Analysis Upgrade — Feature Spec

Adds dual SEO data provider support (Ahrefs / DataForSEO), readability scoring, competitor content analysis, search intent classification, and opportunity scoring — all in pure Rust.

Reference: [SEO Machine](https://github.com/TheCraigHewitt/seomachine) Python pipeline analysis.

---

## Motivation

The Python SEO Machine repo uses textstat, nltk, scikit-learn, and BeautifulSoup for content analysis features that PageSeeds lacks: readability scoring, competitor word-count comparison, search intent classification, keyword density/stuffing detection, and multi-factor opportunity scoring. It also uses DataForSEO as a paid API that returns precise numeric search volumes and CPC — a significant upgrade over Ahrefs free tier's categorical volume labels.

This spec ports the valuable parts to pure Rust. No Python sidecar.

---

## Decision: Pure Rust, No Python Sidecar

Every Python library SEO Machine uses has a Rust equivalent or is trivially portable:

| Python Library | Purpose | Rust Replacement | Notes |
|---|---|---|---|
| `textstat` | Readability formulas (Flesch, SMOG, etc.) | `writing-analysis` crate (v0.1.1, MIT) | Covers 5 formulas + passive voice, cliche, filter word, sentiment, sentence variety |
| `BeautifulSoup` | HTML parsing + CSS selectors | `scraper` crate (v0.26, 15.8M downloads) | Wraps `html5ever` + `selectors` — browser-grade parsing |
| `nltk` (stop words) | Stop word removal | Hardcoded `HashSet<&str>` (~170 English words) | Embedded at compile time, single file |
| `nltk` (syllables) | Syllable counting for formulas | Built into `writing-analysis` | Already handled |
| `scikit-learn` (TF-IDF) | Term frequency scoring | ~50 lines of Rust math | `(count / total) × log(N / df)` |
| `scikit-learn` (K-means) | Topic clustering | Agentic step | Per AGENTS.md: deterministic step collects/ranks, agentic step interprets. LLM-based clustering on structured keyword data is better than bag-of-words K-means |

A Python sidecar would add:
- A bundled Python runtime (~50–80 MB) or a system Python dependency
- Cross-platform packaging complexity in the Tauri build
- An IPC boundary with serialization overhead
- A second language's worth of maintenance

Pure Rust avoids all of this.

---

## Part 1: Dual SEO Data Provider

### Overview

Add a switchable SEO data backend. The user picks "Ahrefs" or "DataForSEO" in Settings. All keyword research, difficulty checks, and SERP data calls route through whichever provider is active.

### DataForSEO Pricing Context

Pay-as-you-go, $50 minimum deposit. Relevant endpoints:

| Endpoint | Price | Notes |
|---|---|---|
| Keywords Data (Google Ads) — Live | $0.075/task | Up to 1,000 keywords per task. Returns precise volumes, CPC, competition |
| Keywords Data (Google Ads) — Queue | $0.05/task | Same data, 1–3 hour turnaround |
| Labs API (related keywords, suggestions) | $0.01/task + $0.0001/item | ~$110 per 1M keywords |
| Labs API (search intent) | $0.001/task + $0.0001/keyword | ~$101 per 1M keywords |
| SERP API (Google Organic) — Live | varies per SE | Real-time SERP results |

Compared to Ahrefs free tier: DataForSEO returns exact numeric volumes (e.g. 12,400) instead of categorical labels (e.g. "MoreThanThousand"). It also provides CPC data, competition scores, and SERP features — none of which are available through the free Ahrefs endpoint.

### Rust Design

#### 1. Provider trait — `src-tauri/src/seo/provider.rs`

```rust
use async_trait::async_trait;
use crate::error::Result;
use crate::seo::keywords::{KeywordIdeasResult, KeywordDifficultyResult};

/// Unified interface for SEO data backends.
#[async_trait]
pub trait SeoDataProvider: Send + Sync {
    /// Generate keyword ideas (regular + question) for a seed keyword.
    async fn keyword_ideas(&self, keyword: &str, country: &str) -> Result<KeywordIdeasResult>;

    /// Get keyword difficulty + SERP overview.
    async fn keyword_difficulty(&self, keyword: &str, country: &str) -> Result<KeywordDifficultyResult>;

    /// Batch keyword difficulty for multiple keywords.
    async fn batch_keyword_difficulty(
        &self,
        keywords: &[String],
        country: &str,
    ) -> Result<Vec<KeywordDifficultyResult>>;

    /// Provider name for display ("ahrefs" | "dataforseo").
    fn name(&self) -> &'static str;
}
```

#### 2. Ahrefs implementation — `src-tauri/src/seo/ahrefs.rs`

Extracts the existing logic from `seo/keywords.rs` into a struct implementing `SeoDataProvider`. The CapSolver flow stays identical. No behavior change.

#### 3. DataForSEO implementation — `src-tauri/src/seo/dataforseo.rs`

New module. Uses Basic auth (login:password base64-encoded) over `reqwest`.

Key endpoints to implement:

- `POST /v3/dataforseo_labs/google/keyword_suggestions/live` — keyword ideas
- `POST /v3/dataforseo_labs/google/keyword_ideas/live` — related keywords
- `POST /v3/dataforseo_labs/google/bulk_keyword_difficulty/live` — difficulty
- `POST /v3/dataforseo_labs/google/serp_competitors/live` — SERP overview
- `POST /v3/dataforseo_labs/google/search_intent/live` — intent classification

Response mapping: DataForSEO returns richer data than Ahrefs. The provider maps it into the shared `KeywordIdea` / `KeywordDifficultyResult` structs. Extra fields (precise volume as `i64`, CPC as `f64`, competition as `f64`) are added as `Option<T>` to the existing structs so both providers can populate what they have.

#### 4. Shared struct changes — `src-tauri/src/seo/keywords.rs`

```rust
pub struct KeywordIdea {
    pub keyword: String,
    pub idea_type: String,
    pub difficulty: Option<String>,
    pub volume: Option<String>,       // categorical (Ahrefs) — keep for backward compat
    pub volume_exact: Option<i64>,    // NEW: precise number (DataForSEO)
    pub cpc: Option<f64>,             // NEW: cost per click (DataForSEO)
    pub competition: Option<f64>,     // NEW: competition score 0–1 (DataForSEO)
    pub country: Option<String>,
}
```

#### 5. Provider resolution — `src-tauri/src/seo/mod.rs`

```rust
/// Build the active SeoDataProvider based on project settings.
pub fn resolve_provider(provider_name: &str, env: &EnvResolver) -> Result<Box<dyn SeoDataProvider>> {
    match provider_name {
        "dataforseo" => {
            let login = env.get("DATAFORSEO_LOGIN")?;
            let password = env.get("DATAFORSEO_PASSWORD")?;
            Ok(Box::new(DataForSeoProvider::new(login, password)))
        }
        _ => {
            let capsolver_key = env.get("CAPSOLVER_API_KEY")?;
            Ok(Box::new(AhrefsProvider::new(capsolver_key)))
        }
    }
}
```

#### 6. Settings storage

Add a column to the `projects` table:

```sql
-- MIGRATION_Vn
ALTER TABLE projects ADD COLUMN seo_provider TEXT NOT NULL DEFAULT 'ahrefs';
```

Valid values: `"ahrefs"`, `"dataforseo"`.

#### 7. Settings UI — `src/components/settings/Settings.tsx`

Add a "SEO Data Provider" card to the existing Settings page. Two-option radio/select toggle:

- **Ahrefs (Free)** — Requires `CAPSOLVER_API_KEY`. Categorical volume labels. No CPC.
- **DataForSEO (Paid)** — Requires `DATAFORSEO_LOGIN` + `DATAFORSEO_PASSWORD`. Precise volumes, CPC, competition.

Switching provider calls a new `set_seo_provider(project_id, provider)` command that updates the SQLite column. The secrets status section already surfaces missing keys — `DATAFORSEO_LOGIN` and `DATAFORSEO_PASSWORD` get added to `REQUIRED_SECRETS` conditionally when DataForSEO is selected.

#### 8. TypeScript changes

`src/lib/types.ts`:
```typescript
export interface KeywordIdea {
  keyword: string
  idea_type: string
  difficulty: string | null
  volume: string | null
  volume_exact: number | null  // NEW
  cpc: number | null           // NEW
  competition: number | null   // NEW
  country: string | null
}
```

`src/lib/tauri.ts`:
```typescript
export async function setSeoProvider(projectId: string, provider: string): Promise<void> {
  return invoke('set_seo_provider', { projectId, provider })
}

export async function getSeoProvider(projectId: string): Promise<string> {
  return invoke('get_seo_provider', { projectId })
}
```

#### 9. Commands — `src-tauri/src/commands/seo.rs`

Existing keyword commands (`keyword_generator`, `keyword_difficulty`, `batch_keyword_difficulty`) switch from calling `seo::keywords::*` directly to resolving the active provider first and calling the trait method. Thin wrappers, no logic change.

```rust
#[tauri::command]
pub async fn keyword_generator(project_id: String, keyword: String, country: String, ...) -> Result<KeywordIdeasResult, String> {
    let provider = resolve_provider_for_project(&project_id, &state)?;
    provider.keyword_ideas(&keyword, &country).await.map_err(|e| e.to_string())
}
```

---

## Part 2: Readability Scoring

### Overview

Add readability analysis to the content audit pipeline. Uses the `writing-analysis` Rust crate — no custom formulas needed.

### New module — `src-tauri/src/content/readability.rs`

```rust
use writing_analysis::{analyze_all, AnalysisResult};
use crate::error::Result;

pub struct ReadabilityReport {
    pub flesch_reading_ease: f64,
    pub flesch_kincaid_grade: f64,
    pub smog_index: f64,
    pub coleman_liau_index: f64,
    pub automated_readability_index: f64,
    pub passive_voice_percentage: f64,
    pub sentence_variety_score: f64,
    pub avg_sentence_length: f64,
    pub cliche_count: usize,
    pub filter_word_percentage: f64,
}

/// Analyze readability of MDX body text (frontmatter stripped).
pub fn analyze_readability(body_text: &str) -> Result<ReadabilityReport> {
    let result = analyze_all(body_text)
        .map_err(|e| crate::error::Error::Other(format!("Readability analysis failed: {e}")))?;
    Ok(ReadabilityReport {
        flesch_reading_ease: result.readability.flesch_reading_ease,
        flesch_kincaid_grade: result.readability.flesch_kincaid_grade,
        smog_index: result.readability.smog_index,
        coleman_liau_index: result.readability.coleman_liau_index,
        automated_readability_index: result.readability.automated_readability_index,
        passive_voice_percentage: result.passive_voice.percentage,
        sentence_variety_score: result.sentence_variety.structure_variety,
        avg_sentence_length: result.sentence_variety.avg_length,
        cliche_count: result.cliches.count,
        filter_word_percentage: result.filter_words.percentage,
    })
}
```

### Integration with content audit

Add readability scores to `audit_one_article()` in `engine/exec/content_audit.rs`:

1. Strip MDX frontmatter using existing `parse_frontmatter()`.
2. Strip MDX/JSX components from body text (regex: `<[A-Z][^>]*>.*?</[A-Z][^>]*>` and `import` lines).
3. Call `content::readability::analyze_readability()` on the cleaned text.
4. Include scores in the per-article audit JSON output.
5. Add two new penalty checks to the 13-check audit:
   - **Readability too low**: Flesch Reading Ease < 30 → penalty weight 8 (targets college+ level content that should be simplified)
   - **Excessive passive voice**: passive voice > 20% → penalty weight 5

### New command

```rust
#[tauri::command]
pub async fn analyze_article_readability(project_id: String, slug: String) -> Result<ReadabilityReport, String>
```

For on-demand readability analysis of a single article from the UI (e.g. in the ArticleTable detail view).

### Cargo.toml addition

```toml
writing-analysis = "0.1"
```

---

## Part 3: Competitor Content Analysis

### Overview

Scrape SERP competitor pages for a target keyword, extract word counts and heading structures, and compare against the user's article. Uses `scraper` + `reqwest`.

### New module — `src-tauri/src/content/competitor.rs`

Implements two features from SEO Machine's `content_length_comparator.py` and `competitor_gap_analyzer.py`:

#### 3a. Word count comparison

1. Accept a keyword and optional URL of the user's article.
2. Fetch Google SERP for the keyword via DataForSEO SERP API (if active) or parse SERP data from the keyword difficulty response.
3. For each of the top 10 organic results:
   - Fetch the page HTML via `reqwest`.
   - Parse with `scraper`, extract `<article>`, `<main>`, or `<body>` text content.
   - Count words.
4. Return: median word count, 75th percentile, min, max, and the user's article word count if provided.
5. Recommend a target word count (75th percentile of competitors).

```rust
pub struct CompetitorWordCount {
    pub url: String,
    pub domain: String,
    pub position: i32,
    pub word_count: usize,
}

pub struct WordCountComparison {
    pub keyword: String,
    pub competitors: Vec<CompetitorWordCount>,
    pub median: usize,
    pub p75: usize,
    pub recommended_min: usize,
    pub user_word_count: Option<usize>,
    pub gap: Option<i64>,  // user_count - recommended_min (negative = needs more)
}
```

#### 3b. Heading structure extraction

For each competitor page, extract H1/H2/H3 headings and section word counts. Store as structured data for the agentic competitor gap analysis step.

```rust
pub struct CompetitorSection {
    pub heading: String,
    pub level: u8,       // 1, 2, or 3
    pub word_count: usize,
    pub is_thin: bool,   // < 150 words
}

pub struct CompetitorStructure {
    pub url: String,
    pub domain: String,
    pub sections: Vec<CompetitorSection>,
    pub total_word_count: usize,
}
```

#### Execution mode

This is deterministic — it collects and structures data. The *interpretation* (finding gaps, recommending a beat-them blueprint) is an agentic step that receives this structured data.

### Cargo.toml addition

```toml
scraper = "0.26"
```

### New command

```rust
#[tauri::command]
pub async fn compare_competitor_content(
    project_id: String,
    keyword: String,
    user_url: Option<String>,
) -> Result<WordCountComparison, String>
```

---

## Part 4: Search Intent Classification

### Overview

Classify keywords as informational / navigational / transactional / commercial. Two implementations depending on provider.

### DataForSEO path (preferred)

DataForSEO Labs has a dedicated search intent endpoint: `POST /v3/dataforseo_labs/google/search_intent/live`. Accepts up to 1,000 keywords per request. Returns intent labels + confidence scores per keyword. Cost: $0.0001 per keyword.

Add to the `SeoDataProvider` trait:

```rust
pub struct IntentClassification {
    pub keyword: String,
    pub intent: String,           // "informational" | "navigational" | "transactional" | "commercial"
    pub confidence: Option<f64>,  // DataForSEO only
}

async fn search_intent(&self, keywords: &[String]) -> Result<Vec<IntentClassification>>;
```

### Ahrefs fallback (deterministic pattern matching)

When using Ahrefs (which has no intent API), classify using keyword pattern matching — the same approach SEO Machine's `search_intent_analyzer.py` uses:

| Pattern | Intent |
|---|---|
| `how to`, `what is`, `guide`, `tutorial`, `why`, `tips` | informational |
| `buy`, `price`, `discount`, `coupon`, `deal`, `cheap`, `order` | transactional |
| `best`, `top`, `review`, `vs`, `comparison`, `alternative` | commercial |
| Brand names, `login`, `sign in`, `.com`, specific product names | navigational |

This is a deterministic mapping — no LLM needed. Stored as a static lookup table in `seo/intent.rs`.

### Integration

- Intent shown as a badge on keywords in `KeywordPicker` and `KeywordResearch` components.
- Used as a scoring factor in opportunity scoring (Part 5).

---

## Part 5: Opportunity Scoring

### Overview

Replace the current KD-only pre-selection in `KeywordPicker` with a multi-factor scoring model. Inspired by SEO Machine's `opportunity_scorer.py` (8 weighted factors).

### New module — `src-tauri/src/seo/scoring.rs`

Deterministic scoring function. No LLM.

#### Factors and weights

| Factor | Weight | Source | Notes |
|---|---|---|---|
| Volume | 25% | DataForSEO `volume_exact` or Ahrefs categorical mapping | Ahrefs: map "MoreThanThousand" → 1.0, "HundredToThousand" → 0.6, "LessThanHundred" → 0.2 |
| KD (inverted) | 20% | Both providers | Score = `1.0 - (kd / 100)` |
| Intent alignment | 20% | Part 4 classification | transactional/commercial → 1.0, informational → 0.5, navigational → 0.2 |
| Competition | 15% | DataForSEO `competition` or derive from KD | Ahrefs fallback: use inverse KD as proxy |
| Content gap | 10% | Check if keyword exists in `articles.json` | New keyword → 1.0, partially covered → 0.5, fully covered → 0.0 |
| CPC signal | 5% | DataForSEO only | Higher CPC → higher commercial value. Ahrefs: 0.5 (no data) |
| Freshness | 5% | SERP age signals if available | Default 0.5 when no data |

#### Output

```rust
pub struct OpportunityScore {
    pub keyword: String,
    pub total_score: f64,        // 0.0–1.0
    pub tier: String,            // "high" | "medium" | "low"
    pub factor_scores: HashMap<String, f64>,
}

/// Score keywords with multi-factor model.
pub fn score_opportunities(
    keywords: &[KeywordIdea],
    existing_slugs: &[String],
) -> Vec<OpportunityScore>
```

Tier thresholds: high ≥ 0.7, medium ≥ 0.4, low < 0.4.

#### Integration

- `KeywordPicker` pre-selects "high" tier keywords by default (replacing the current `kd < 50` check).
- Opportunity score column added to the keyword table UI.
- Score computed client-side in the component using the scoring function exposed via a command, or computed in Rust and returned alongside keyword results.

---

## Part 6: Keyword Density & Stuffing Detection

### Overview

Deterministic analysis of keyword usage in article content. Runs as part of the content audit.

### New module — `src-tauri/src/content/keyword_density.rs`

Mirrors SEO Machine's `keyword_analyzer.py` for the deterministic parts:

1. **Keyword density**: Count target keyword occurrences / total words. Flag if > 3% (stuffing) or < 0.5% (underused).
2. **Section distribution**: Check keyword appears in intro, body, and conclusion — not clustered in one section only.
3. **Consecutive sentence detection**: Flag if the same keyword appears in 3+ consecutive sentences.

Does NOT implement TF-IDF clustering or LSI keyword identification — those are better served by the agentic content review step that already exists in the `content_review` workflow.

```rust
pub struct KeywordDensityReport {
    pub keyword: String,
    pub total_words: usize,
    pub keyword_count: usize,
    pub density_percent: f64,
    pub is_stuffed: bool,       // density > 3%
    pub is_underused: bool,     // density < 0.5%
    pub section_distribution: Vec<SectionPresence>,
    pub consecutive_violations: Vec<ConsecutiveViolation>,
}
```

---

## Implementation Order

| Phase | What | New Crates | New Files | Commands |
|---|---|---|---|---|
| 1 | Provider trait + Ahrefs refactor | `async-trait` | `seo/provider.rs`, `seo/ahrefs.rs` | — |
| 2 | DataForSEO implementation | — | `seo/dataforseo.rs` | `set_seo_provider`, `get_seo_provider` |
| 3 | Settings UI toggle | — | — (edit `Settings.tsx`) | — |
| 4 | Readability scoring | `writing-analysis` | `content/readability.rs` | `analyze_article_readability` |
| 5 | Readability in content audit | — | — (edit `content_audit.rs`) | — |
| 6 | Search intent | — | `seo/intent.rs` | `classify_search_intent` |
| 7 | Opportunity scoring | — | `seo/scoring.rs` | `score_keyword_opportunities` |
| 8 | Competitor content analysis | `scraper` | `content/competitor.rs` | `compare_competitor_content` |
| 9 | Keyword density | — | `content/keyword_density.rs` | — (integrated into audit) |

Each phase is independently shippable. Phases 1–3 (dual provider) form one coherent unit. Phases 4–5 (readability) form another. The rest are independent.

---

## Files Changed (Summary)

### New Rust files
- `src-tauri/src/seo/provider.rs` — trait definition
- `src-tauri/src/seo/ahrefs.rs` — Ahrefs provider impl (extracted from `keywords.rs`)
- `src-tauri/src/seo/dataforseo.rs` — DataForSEO provider impl
- `src-tauri/src/seo/intent.rs` — search intent classification
- `src-tauri/src/seo/scoring.rs` — opportunity scoring
- `src-tauri/src/content/readability.rs` — readability analysis
- `src-tauri/src/content/competitor.rs` — competitor page analysis
- `src-tauri/src/content/keyword_density.rs` — density + stuffing checks

### Modified Rust files
- `src-tauri/src/seo/mod.rs` — add new submodules, `resolve_provider()`
- `src-tauri/src/seo/keywords.rs` — add optional fields to `KeywordIdea`, `KeywordDifficultyResult`
- `src-tauri/src/commands/seo.rs` — refactor to use provider trait
- `src-tauri/src/commands/mod.rs` — register new commands
- `src-tauri/src/lib.rs` — register new commands in `generate_handler!`
- `src-tauri/src/db/mod.rs` — new migration for `seo_provider` column
- `src-tauri/src/config/env_resolver.rs` — add `DATAFORSEO_LOGIN`, `DATAFORSEO_PASSWORD` to secret resolution
- `src-tauri/src/engine/exec/content_audit.rs` — integrate readability + keyword density checks
- `src-tauri/Cargo.toml` — add `writing-analysis`, `scraper`, `async-trait`

### Frontend files
- `src/lib/types.ts` — update `KeywordIdea`, add `ReadabilityReport`, `OpportunityScore`, `IntentClassification`
- `src/lib/tauri.ts` — add `setSeoProvider`, `getSeoProvider`, `analyzeArticleReadability`, `compareCompetitorContent`, `classifySearchIntent`, `scoreKeywordOpportunities`
- `src/components/settings/Settings.tsx` — add SEO provider toggle card
- `src/components/seo/KeywordResearch.tsx` — show intent badges, opportunity scores, precise volumes when available
- `src/components/tasks/KeywordPicker.tsx` — update pre-selection to use opportunity tier instead of raw KD
- `src/components/articles/ContentHealth.tsx` — show readability scores

---

## What This Spec Does NOT Cover

- **Google Analytics integration** — SEO Machine uses GA4 for session/engagement data. This is a separate scope with its own OAuth flow.
- **TF-IDF topic clustering** — The agentic content_review step handles semantic analysis better than bag-of-words clustering. Skipped intentionally.
- **LSI keyword suggestions** — Agentic step territory. The LLM already does this during article planning.
- **Full SERP scraping without DataForSEO** — Scraping Google SERPs directly risks IP blocks and violates ToS. Only done through DataForSEO's SERP API or from data already available in keyword difficulty responses.
