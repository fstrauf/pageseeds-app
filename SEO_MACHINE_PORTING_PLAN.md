# SEO Machine → PageSeeds Porting Plan

**Created:** 2026-04-06  
**Source:** https://github.com/TheCraigHewitt/seomachine  
**Goal:** Port deterministic SEO analysis modules from Python to Rust

---

## Executive Summary

SEO Machine is a Claude Code workspace with sophisticated **deterministic Python analysis modules** for SEO content analysis. This plan outlines porting the most valuable modules to Rust for integration into PageSeeds' workflow system.

**Key Principle:** Port the **deterministic analysis logic**, not the Python architecture. The LLM-heavy agent system is not needed—PageSeeds already has a superior workflow/task architecture.

---

## Phase Overview

| Phase | Focus | Effort | Priority |
|-------|-------|--------|----------|
| 1 | Search Intent Classification | ✅ **COMPLETE** | 🔴 Critical |
| 2 | Content Length Comparator | ✅ **COMPLETE** | 🔴 Critical |
| 3 | Landing Page CRO Analysis | 4-5 days | 🔴 Critical |
| 4 | Opportunity Scoring | 2-3 days | 🟡 High |
| 5 | Readability Scoring | 2-3 days | 🟡 High |
| 6 | Topic Clustering | 4-5 days | 🟢 Medium |
| 7 | Content Quality Scoring | 3-4 days | 🟢 Medium |

**Total Estimated Effort:** 20-25 days

---

## Phase 1: Search Intent Classification

### Source
- File: `data_sources/modules/search_intent_analyzer.py`
- Lines: 374

### What to Port

#### 1.1 Intent Signal Definitions
```rust
// src/seo/intent.rs
pub enum SearchIntent {
    Informational,
    Navigational,
    Transactional,
    Commercial,
}

pub struct IntentSignals {
    informational: Vec<&'static str>,
    navigational: Vec<&'static str>,
    transactional: Vec<&'static str>,
    commercial: Vec<&'static str>,
}
```

**Signal Keywords:**
- Informational: `what`, `why`, `how`, `when`, `where`, `who`, `guide`, `tutorial`, `learn`, `tips`, `explained`
- Navigational: `login`, `sign in`, `website`, `official`, `home page`, `account`, `dashboard`
- Transactional: `buy`, `purchase`, `order`, `download`, `pricing`, `cost`, `free trial`, `sign up`
- Commercial: `best`, `top`, `review`, `vs`, `versus`, `compare`, `alternative`, `better than`

#### 1.2 Scoring Algorithm
- Pattern matching on keyword text
- SERP feature detection (if available)
- Confidence score calculation (0-100%)
- Primary + secondary intent detection

#### 1.3 Integration Points
- **Add to:** `src-tauri/src/seo/keywords.rs`
- **Use in:** `keyword_discovery.md` prompt for filtering
- **Workflow:** Pre-filter keywords by intent before LLM selection

### Acceptance Criteria
- [ ] Classifies keywords into 4 intent types
- [ ] Returns confidence scores per intent
- [ ] Detects secondary intent when close
- [ ] Provides content recommendations per intent type

---

## Phase 2: Content Length Comparator

### Source
- File: `data_sources/modules/content_length_comparator.py`
- Lines: 362

### What to Port

#### 2.1 SERP Content Fetching
```rust
// src/seo/competitor_content.rs
pub struct ContentLengthComparator;

impl ContentLengthComparator {
    pub async fn analyze_competitor_lengths(
        &self,
        keyword: &str,
        serp_results: Vec<SerpResult>,
    ) -> Result<LengthAnalysis> {
        // Fetch top 10 URLs
        // Extract main content (remove nav, footer, ads)
        // Count words using regex: r'\b[a-zA-Z]{2,}\b'
    }
}
```

#### 2.2 Content Extraction Strategy
- Use CSS selectors: `article`, `main`, `[role="main"]`, `.content`, `#content`
- Fallback to `<body>`
- Remove: `script`, `style`, `nav`, `footer`, `header`, `aside`
- Clean and count words

#### 2.3 Statistical Analysis
- Calculate: min, max, mean, median, mode, std_dev
- Percentiles: 25th, 75th
- Target length: `max(75th_percentile, median * 1.2)`

#### 2.4 Output Structure
```rust
pub struct LengthAnalysis {
    pub keyword: String,
    pub competitors_analyzed: usize,
    pub statistics: LengthStatistics,
    pub competitor_lengths: Vec<CompetitorLength>,
    pub recommendation: LengthRecommendation,
}

pub struct LengthRecommendation {
    pub recommended_min: usize,
    pub recommended_optimal: usize,
    pub recommended_max: usize,
    pub reasoning: String,
}
```

### Acceptance Criteria
- [x] Fetches top 10 SERP results (uses existing Ahrefs data)
- [x] Extracts word count from each page
- [x] Calculates statistics (median, 75th percentile)
- [x] Recommends target word count
- [x] Categorizes length distribution

### Future Enhancement: DataForSEO Integration
**Cost:** ~$0.006/keyword (~$6 per 1000 keywords)

**Benefits over current approach:**
- More reliable SERP data (no CapSolver needed for data)
- Faster response times
- Additional metrics (search volume, difficulty, CPC)
- Better uptime/reliability

**Implementation:**
- Add `DataForSEO` client in `src/seo/dataforseo.rs`
- Modify `analyze_competitor_content` to accept data source parameter
- Add config option for DataForSEO credentials

---

## Phase 3: Landing Page CRO Analysis

### Source
- Files: 
  - `data_sources/modules/landing_page_scorer.py` (782 lines)
  - `data_sources/modules/above_fold_analyzer.py` (512 lines)
  - `data_sources/modules/cta_analyzer.py` (550 lines)
  - `data_sources/modules/trust_signal_analyzer.py` (567 lines)

### What to Port

#### 3.1 Landing Page Scorer (Main Framework)

```rust
// src/seo/landing_pages.rs
pub enum PageType {
    Seo,  // 1500-2500 words, 3-5 CTAs
    Ppc,  // 400-800 words, 2-3 CTAs
}

pub enum ConversionGoal {
    Trial,
    Demo,
    Lead,
}

pub struct LandingPageScorer {
    page_type: PageType,
    conversion_goal: ConversionGoal,
}
```

**Scoring Categories & Weights:**

| Category | SEO Weight | PPC Weight |
|----------|------------|------------|
| Above-the-fold | 25% | 30% |
| CTAs | 25% | 30% |
| Trust Signals | 20% | 25% |
| Structure | 15% | 15% |
| SEO | 15% | 0% |

#### 3.2 Above-the-Fold Analyzer

**Elements to Check (first 700 chars):**

| Element | Weight | Patterns |
|---------|--------|----------|
| Headline (H1) | 35% | Strong: starts with number, question, benefit verb, pain removal |
| Value Proposition | 25% | "help you", "grow your", "save", "easiest way" |
| CTA Visibility | 25% | Must appear above fold |
| Trust Signal | 15% | Customer count, testimonial, rating |

**Headline Quality Patterns:**
- **Weak (penalize):** `^Welcome to`, `^The best`, `^Everything you need`, `^We help`
- **Strong (reward):** `^\d+`, `\?$`, `(without|no more)`, `(save|grow|increase)`

#### 3.3 CTA Analyzer

**Goal-Specific Patterns:**

```rust
pub struct CTAPatterns {
    trial: Vec<Regex>,
    demo: Vec<Regex>,
    lead: Vec<Regex>,
}

// Trial patterns:
// - "start (your )?free trial"
// - "try (it )?free"
// - "get started (for )?free"
// - "free for \d+ days"
// - "no credit card"

// Demo patterns:
// - "(book|schedule|request) a demo"
// - "talk to (sales|an expert)"
// - "see it in action"
```

**CTA Quality Scoring:**
- Action verb strength (strongest: start, get, claim, unlock, discover)
- Benefit words (free, instant, today, unlimited)
- Urgency words (now, today, limited)
- Specificity (\d+-day, \d+%, \$\d+)

**Distribution Scoring:**
- Above fold (< 20%): +20 points
- Mid page (30-70%): +20 points
- Closing (> 80%): +20 points

#### 3.4 Trust Signal Analyzer

**Categories:**

| Category | Score Weight | Patterns |
|----------|--------------|----------|
| Testimonials | 35% | Quoted text with attribution |
| Social Proof | 30% | Customer counts, specific results (\d+% increase) |
| Risk Reversal | 25% | Free trial, no credit card, cancel anytime, guarantee |
| Authority | 10% | Media mentions, awards, years in business |

**Testimonial Detection:**
- Quoted text: `"[^"]{20,300}"`
- Attribution: `—\s*\*?\*?([A-Z][a-z]+(?:\s+[A-Z]\.?)?)`
- Quality: attributed + specific data = "strong"

#### 3.5 Integration Points

- **New Module:** `src-tauri/src/seo/landing_pages.rs`
- **New Command:** `analyze_landing_page` in `commands.rs`
- **New Workflow:** `landing_page_audit` handler
- **Frontend:** Landing page audit UI component

### Acceptance Criteria
- [ ] Scores landing pages 0-100
- [ ] Supports SEO vs PPC modes
- [ ] Supports trial/demo/lead goals
- [ ] Detects above-fold issues
- [ ] Analyzes CTA quality and distribution
- [ ] Identifies trust signals
- [ ] Provides prioritized recommendations

---

## Phase 4: Opportunity Scoring

### Source
- File: `data_sources/modules/opportunity_scorer.py`
- Lines: 515

### What to Port

#### 4.1 Multi-Factor Scoring Model

```rust
// src/seo/opportunity.rs
pub struct OpportunityScorer;

pub struct OpportunityScore {
    pub final_score: f64,        // 0-100
    pub priority: Priority,      // CRITICAL, HIGH, MEDIUM, LOW, SKIP
    pub primary_factor: String,  // Which factor drove the score
    pub breakdown: ScoreBreakdown,
}

pub struct ScoreBreakdown {
    pub volume_score: f64,       // 25% - Search demand
    pub position_score: f64,     // 20% - Proximity to target
    pub intent_score: f64,       // 20% - Commercial value
    pub competition_score: f64,  // 15% - Ranking difficulty (inverted)
    pub cluster_score: f64,      // 10% - Strategic topic value
    pub ctr_score: f64,          // 5% - CTR improvement potential
    pub freshness_score: f64,    // 5% - Update requirements
    pub trend_score: f64,        // 5% - Rising/declining interest
}
```

#### 4.2 Scoring Logic

**Volume Score (0-100):**
- 5000+ impressions: 100
- 2000-4999: 90
- 1000-1999: 80
- 500-999: 65
- 250-499: 50
- 100-249: 35
- 50-99: 20
- <50: 10

**Position Score (Quick Win: 11-20):**
- Position 11-12: 100 (very close to page 1)
- Position 13-15: 85
- Position 16-18: 70
- Position 19-20: 55

**Competition Score (inverted difficulty):**
- Difficulty ≤20: 100
- Difficulty 21-35: 85
- Difficulty 36-50: 70
- Difficulty 51-65: 50
- Difficulty 66-80: 30
- Difficulty >80: 10

**CTR Score (expected vs actual):**
- Uses expected CTR by position table
- High gap = high opportunity score

#### 4.3 Priority Determination

| Final Score | Priority |
|-------------|----------|
| ≥80 | CRITICAL |
| 65-79 | HIGH |
| 45-64 | MEDIUM |
| 25-44 | LOW |
| <25 | SKIP |

#### 4.4 Traffic Potential Calculator

```rust
pub fn calculate_traffic_potential(
    current_position: f64,
    target_position: i32,
    impressions: i64,
    current_clicks: i64,
) -> TrafficProjection {
    // Expected CTR table (position 1-20)
    // Calculate potential clicks at target position
}
```

#### 4.5 Integration Points

- **Add to:** `src-tauri/src/seo/opportunity.rs`
- **Use in:** Keyword research workflow for prioritization
- **Frontend:** Opportunity score display in keyword tables

### Acceptance Criteria
- [ ] Implements 8-factor scoring model
- [ ] Calculates traffic potential
- [ ] Assigns priority levels
- [ ] Explains primary scoring factor
- [ ] Handles all opportunity types (quick win, improvement, new content)

---

## Phase 5: Readability Scoring

### Source
- File: `data_sources/modules/readability_scorer.py`
- Lines: 505

### What to Port

#### 5.1 Core Metrics

```rust
// src/content/readability.rs
pub struct ReadabilityAnalysis {
    pub overall_score: f64,              // 0-100
    pub grade: ReadabilityGrade,
    pub reading_level: f64,              // Grade level (target: 8-10)
    pub metrics: ReadabilityMetrics,
    pub structure: StructureAnalysis,
    pub complexity: ComplexityAnalysis,
}

pub struct ReadabilityMetrics {
    pub flesch_reading_ease: f64,      // Target: 60-70
    pub flesch_kincaid_grade: f64,
    pub gunning_fog: f64,
    pub smog_index: f64,
    pub coleman_liau_index: f64,
}
```

#### 5.2 Flesch Reading Ease Formula

```rust
// Flesch Reading Ease = 206.835 - (1.015 × avg_sentence_length) - (84.6 × avg_syllables_per_word)
// Grade Level = (0.39 × avg_sentence_length) + (11.8 × avg_syllables_per_word) - 15.59
```

#### 5.3 Structure Analysis

| Metric | Target | Penalty if exceeded |
|--------|--------|---------------------|
| Avg sentence length | <20 words | -5 to -20 points |
| Sentences per paragraph | 2-4 | -5 to -10 points |
| Long sentences (25+) | minimize | -3 per sentence |
| Very long sentences (35+) | 0 | -3 per sentence |

#### 5.4 Complexity Analysis

- **Passive voice detection:** `\b(?:is|are|was|were|been|being)\s+\w+ed\b`
- **Transition words:** however, moreover, therefore, consequently, etc.
- **Complex words:** 3+ syllables

#### 5.5 Scoring Weights

| Factor | Weight |
|--------|--------|
| Flesch Reading Ease | 30 points |
| Grade Level | 25 points |
| Sentence Length | 20 points |
| Paragraph Structure | 10 points |
| Passive Voice | 10 points |
| Transition Words | 5 points |

#### 5.6 Integration Points

- **Add to:** `src-tauri/src/content/readability.rs`
- **Use in:** Content audit workflow
- **Frontend:** Readability score in article editor

### Acceptance Criteria
- [ ] Calculates Flesch Reading Ease
- [ ] Calculates Flesch-Kincaid Grade Level
- [ ] Detects passive voice ratio
- [ ] Analyzes sentence length distribution
- [ ] Checks paragraph structure
- [ ] Provides specific recommendations

---

## Phase 6: Topic Clustering

### Source
- File: `research_topic_clusters.py`
- Lines: 566

### What to Port

#### 6.1 ML-Based Clustering (Optional)

```rust
// src/seo/topic_clusters.rs
// Note: Requires ML crate like linfa or rust-bert

pub struct TopicClusterer {
    n_clusters: usize,
}

impl TopicClusterer {
    pub fn cluster_keywords_ml(
        &self,
        keywords: Vec<RankingKeyword>,
    ) -> Vec<TopicCluster> {
        // TF-IDF vectorization
        // K-means clustering
        // Extract topic names from cluster centers
    }
}
```

#### 6.2 Simple Pattern-Based Clustering (Primary)

```rust
pub struct TopicPatterns {
    patterns: HashMap<String, Vec<String>>,
}

impl TopicPatterns {
    pub fn new() -> Self {
        let mut patterns = HashMap::new();
        patterns.insert("Pricing".to_string(), vec!["price", "pricing", "cost", "plan"]);
        patterns.insert("Tutorials".to_string(), vec!["how to", "guide", "tutorial"]);
        patterns.insert("Comparisons".to_string(), vec!["vs", "compare", "alternative"]);
        // ... etc
        Self { patterns }
    }
}
```

#### 6.3 Authority Score Calculation

```rust
pub struct TopicAuthority {
    pub topic: String,
    pub authority_score: i32,      // 0-100
    pub authority_level: AuthorityLevel, // Minimal, Weak, Moderate, Strong
    pub keyword_count: usize,
    pub avg_position: f64,
    pub total_impressions: i64,
}

// Scoring:
// Coverage (50%): keywords ranking for
// Position Quality (30%): avg position
// Demand (20%): total impressions
```

#### 6.4 Gap Analysis

- Find related keywords using Ahrefs API
- Filter out keywords already ranking
- Sort by search volume

#### 6.5 Integration Points

- **New Module:** `src-tauri/src/seo/topic_clusters.rs`
- **New Workflow:** `analyze_topic_clusters` handler
- **Frontend:** Topic cluster visualization

### Acceptance Criteria
- [ ] Groups keywords into topic clusters
- [ ] Calculates authority score per topic
- [ ] Identifies weak topics with high demand
- [ ] Finds coverage gaps
- [ ] Provides cluster building recommendations

---

## Phase 7: Content Quality Scoring

### Source
- File: `data_sources/modules/content_scorer.py`
- Lines: 850

### What to Port

#### 7.1 Multi-Dimensional Scoring

```rust
// src/content/quality.rs
pub struct ContentQualityScore {
    pub composite_score: f64,      // 0-100
    pub passed: bool,              // >= 70
    pub threshold: f64,            // 70
    pub dimensions: DimensionScores,
    pub priority_fixes: Vec<PriorityFix>,
}

pub struct DimensionScores {
    pub humanity: DimensionScore,        // 30% - Human tone, personality
    pub specificity: DimensionScore,     // 25% - Concrete examples
    pub structure_balance: DimensionScore, // 20% - Prose-to-list ratio
    pub seo: DimensionScore,             // 15% - SEO compliance
    pub readability: DimensionScore,     // 10% - Flesch score
}
```

#### 7.2 Humanity Scoring

**AI Phrase Detection (penalize):**
- `in today's (digital|modern|fast-paced)`
- `when it comes to`
- `it's important to (note|remember|understand)`
- `let's dive (in|into)`
- `furthermore`, `moreover`, `additionally`
- `leverage`, `utilize`, `synergy`
- `unlock(ing)? (the )?(power|potential)`

**Conversational Devices (reward):**
- Contractions: don't, can't, you're, it's
- Questions: `\?(?:\s|$)`
- Casual openers: "Look", "Here's the thing", "Trust me"

#### 7.3 Specificity Scoring

**Vague Words (penalize):**
- many, some, various, numerous, several
- often, sometimes, usually, generally
- significant, substantial, considerable
- very, really, quite, rather

**Specificity Indicators (reward):**
- Percentages: `\d{1,3}%`
- Dollar amounts: `\$[\d,]+`
- Years: `\d{4}`
- Counts: `\d+(?:,\d{3})* (downloads|listeners|users)`

#### 7.4 Structure Balance

**Target:** 50-75% prose (rest is lists, tables, headers)

```rust
pub struct StructureBalance {
    pub prose_ratio: f64,
    pub list_ratio: f64,
    pub table_ratio: f64,
}
```

#### 7.5 Integration Points

- **Add to:** `src-tauri/src/content/quality.rs`
- **Use in:** Content audit workflow
- **Frontend:** Quality score dashboard

### Acceptance Criteria
- [ ] Scores content across 5 dimensions
- [ ] Detects AI phrases
- [ ] Measures specificity
- [ ] Checks structure balance
- [ ] Provides prioritized fixes
- [ ] Composite score >= 70 to pass

---

## Implementation Strategy

### New File Structure

```
src-tauri/src/
├── seo/
│   ├── mod.rs
│   ├── keywords.rs
│   ├── intent.rs              # NEW: Phase 1
│   ├── competitor_content.rs  # NEW: Phase 2
│   ├── landing_pages.rs       # NEW: Phase 3
│   ├── opportunity.rs         # NEW: Phase 4
│   └── topic_clusters.rs      # NEW: Phase 6
├── content/
│   ├── mod.rs
│   ├── readability.rs         # NEW: Phase 5
│   └── quality.rs             # NEW: Phase 7
└── engine/
    └── workflows/
        └── handlers.rs        # Add new handlers
```

### New Workflow Handlers

| Handler | Phase | Description |
|---------|-------|-------------|
| `analyze_search_intent` | 1 | Classify keyword intent |
| `analyze_competitor_content` | 2 | Compare content length |
| `audit_landing_page` | 3 | Full CRO analysis |
| `score_opportunities` | 4 | Multi-factor prioritization |
| `analyze_readability` | 5 | Readability metrics |
| `analyze_topic_clusters` | 6 | ML-based clustering |
| `score_content_quality` | 7 | 5-dimension quality score |

### Commands to Add

```rust
// src-tauri/src/commands.rs

#[tauri::command]
pub async fn analyze_search_intent(
    keyword: String,
) -> Result<IntentAnalysis, String> { ... }

#[tauri::command]
pub async fn analyze_competitor_content(
    keyword: String,
    serp_results: Vec<SerpResult>,
) -> Result<LengthAnalysis, String> { ... }

#[tauri::command]
pub async fn audit_landing_page(
    content: String,
    page_type: PageType,
    goal: ConversionGoal,
) -> Result< LandingPageAudit, String> { ... }
```

### Frontend Components

| Component | Phase | Description |
|-----------|-------|-------------|
| `IntentBadge` | 1 | Display intent with confidence |
| `CompetitorLengthChart` | 2 | Bar chart of competitor lengths |
| `LandingPageScoreCard` | 3 | Overall CRO score display |
| `AboveFoldChecklist` | 3 | 5-second test results |
| `CTAAnalysisPanel` | 3 | CTA quality and distribution |
| `OpportunityScoreRing` | 4 | Circular score visualization |
| `ReadabilityGauge` | 5 | Flesch score gauge |
| `TopicClusterMap` | 6 | Cluster visualization |
| `QualityDimensionRadar` | 7 | Radar chart of 5 dimensions |

---

## Dependencies to Add

### Cargo.toml Additions

```toml
[dependencies]
# For Phase 2 (content fetching)
scraper = "0.19"

# For Phase 6 (clustering) - optional
linfa = "0.7"
linfa-clustering = "0.7"

# For regex patterns (all phases)
regex = "1.10"  # likely already present

# For text statistics
syllable = "0.2"  # or implement manually
```

---

## Testing Strategy

### Unit Tests

Each module should have comprehensive unit tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_classification() {
        assert_eq!(
            classify_intent("how to start a podcast"),
            SearchIntent::Informational
        );
        assert_eq!(
            classify_intent("best podcast hosting"),
            SearchIntent::Commercial
        );
    }

    #[test]
    fn test_flesch_calculation() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let score = calculate_flesch_reading_ease(text);
        assert!(score > 50.0);
    }
}
```

### Integration Tests

Test the full workflow:

```rust
#[tokio::test]
async fn test_landing_page_audit_workflow() {
    let content = load_test_landing_page();
    let result = audit_landing_page(content, PageType::Seo, ConversionGoal::Trial).await;
    assert!(result.overall_score > 0.0);
    assert!(!result.category_scores.is_empty());
}
```

---

## Migration Checklist

### Pre-Implementation
- [ ] Review all source Python files in detail
- [ ] Set up regex pattern testing environment
- [ ] Design TypeScript types for frontend

### Per Phase
- [ ] Port Python logic to Rust
- [ ] Add comprehensive unit tests
- [ ] Create command handler
- [ ] Add workflow handler integration
- [ ] Build frontend component
- [ ] Write documentation
- [ ] Update AGENTS.md

### Post-Implementation
- [ ] Integration testing
- [ ] Performance benchmarking
- [ ] Documentation review
- [ ] User acceptance testing

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| ML clustering complexity (Phase 6) | Implement pattern-based fallback first |
| Content fetching reliability (Phase 2) | Add timeouts, retries, fallback to cached data |
| Regex performance | Compile patterns once, use lazy_static |
| Syllable counting accuracy | Use simple heuristic (vowel groups) |
| Python-to-Rust logic parity | Extensive unit tests with same inputs |

---

## Success Metrics

After porting is complete:

1. **Keyword Research:** Intent classification for 100% of keywords
2. **Content Planning:** Competitor length analysis for all target keywords
3. **Landing Pages:** CRO audit capability for landing page content
4. **Prioritization:** Opportunity scores for all keyword candidates
5. **Content Quality:** Automated quality gates before publishing

---

## References

- SEO Machine Repository: https://github.com/TheCraigHewitt/seomachine
- Flesch Reading Ease Formula: https://en.wikipedia.org/wiki/Flesch%E2%80%93Kincaid_readability_tests
- TF-IDF + K-means Clustering: https://scikit-learn.org/stable/modules/clustering.html
