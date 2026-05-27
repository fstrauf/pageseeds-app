# SEO Plateau Breakthrough: Problem Statement & Action Plan

## For: Days to Expiry (daystoexpiry.com) + PageSeeds App

---

## 1. Problem Statement

### Current State (Google Search Console, Last 3 Months)

| Metric | Value | Benchmark (Position 6-7) | Gap |
|--------|-------|-------------------------|-----|
| Total Impressions | 664,000 | — | — |
| Total Clicks | 757 | ~29,000 | **35x below expected** |
| **CTR** | **0.1%** | **3-4.4%** | **30-44x underperformance** |
| Avg. Position | 6.9 | — | Page 1, but near bottom |
| Daily Impressions | 5,000-10,000 | Flatlined | Zero growth for 3 months |
| Daily Clicks | 5-20 | ~300-450 | Stagnant |

**The core issue**: Days to Expiry ranks on page 1 (position 6.9) but almost nobody clicks. This is not a ranking problem — it is a click-through rate problem compounded by structural content issues.

### Why This Matters

At 0.1% CTR, the site captures **less than 3% of its potential traffic** for its ranking position. Even reaching a conservative 2% CTR (still below benchmark) would mean **20x more clicks** — from ~8/day to ~160/day. The ceiling is artificial and fixable.

---

## 2. Root Cause Analysis

### The Three Compounding Constraints

The plateau is caused by three constraints operating simultaneously. Fixing only one will not break through.

```
Low Topical Authority (25 articles)
          |
          v
    Lower Rankings (position 6-10)
          |
          v
  AI Overview Exposure (40-90% of queries)
          |
          v
      Zero-Click Answers
          |
          v
    Few Clicks + Weak Engagement
          |
          v
   Stalled Authority Growth ---> (loop back to start)
```

#### Constraint 1: AI Overview Compression

Google AI Overviews now answer informational queries directly in the SERP, eliminating the need to click.

- **41-91%** of finance educational queries trigger AI Overviews (BrightEdge 2026)
- Organic CTR on AIO-present queries collapsed **65%** (Seer Interactive, 2.43B impressions studied)
- Days to Expiry's content ("what is a covered call," "how to wheel strategy") is almost entirely informational — the most vulnerable category
- Being cited in an AIO gives **+35% organic clicks**; not being cited means near-zero clicks

#### Constraint 2: Keyword Cannibalization

Multiple articles on the site compete against each other for the same keywords, preventing any single page from ranking in the top 3.

**Evidence**:
- 4 covered call articles published in 4 days (Apr 22-25, 2026)
- 2+ 0DTE articles competing for overlapping terms
- 2+ iron condor articles with similar targeting
- Average position stuck at 6.9 — the classic "cannibalization dead zone"

**Impact**: Instead of one strong page ranking at position 3-4, four weak pages rank at positions 6-10, each getting minimal clicks.

#### Constraint 3: Topical Authority Deficit

Google does not see Days to Expiry as a comprehensive authority on options trading.

| Site | Article Count | Strategy Coverage |
|------|--------------|-------------------|
| Days to Expiry | ~25 | 8-10 strategies |
| Option Alpha | 100+ | 30+ strategies |
| tastylive | 200+ | Full curriculum |
| Investopedia | 500+ | Comprehensive |

**Missing topics**: credit spreads, strangles, butterflies, volatility skew, gamma scalping, portfolio margin, earnings trades, diagonal spreads, ratio spreads, and 20+ more.

### Secondary Amplifying Factors

| Factor | Current State | Impact |
|--------|--------------|--------|
| Title tags | Generic, no numbers/brackets/specificity | Missing 50-200% CTR lift |
| FAQ schema | Not implemented | Missing +25-40% CTR and AIO citation boost |
| Author credentials | Not visible | Weak E-E-A-T for finance YMYL content |
| Comparison tables | Minimal | Missing table snippet opportunities (80% success rate) |
| Original research | None published | Missing 3.2x AI citation advantage |
| Interactive tools | Not exposed as pages | Missing AIO-resistant traffic source |
| YouTube presence | None | Missing 29.5% of AIO citations (YouTube) |
| Branded search volume | Low | Broken authority feedback loop |

---

## 3. The Solution: 60-Day Coordinated Campaign

**Principle**: A sequential approach (fix one thing at a time) will fail. All workstreams must run simultaneously within a 60-day window to create compounding signals.

### Phase 1: Quick Wins (Days 1-14)

Highest impact, lowest effort. These alone can deliver 5-10x CTR improvement.

#### 3.1.1 Rewrite All Title Tags

**Problem**: Current titles are generic and don't stand out in SERPs.

**Rules for new titles**:
- Include a number: "7 Strategies for..." (outperforms non-numbered by 36%)
- Use brackets: "[2026 Guide]" or "[With Examples]" (outperforms by 38%)
- Add specificity: "for Small Accounts" or "Under $10K"
- Use negative framing where appropriate: "5 Mistakes to Avoid" (+10-20% CTR)
- Include current year for freshness: "(2026)"
- Keep to 50-60 characters

**Example rewrites**:

| Current Title | Optimized Title |
|--------------|----------------|
| Covered Call Strategy | 7 Covered Call Strategies That Actually Work [2026] |
| 0DTE Strategy Radar | 0DTE Strategy Radar: 5 High-Probability Setups [Backtested] |
| Wheel Strategy Complete DTE Playbook | The Wheel Strategy Playbook: DTE Timing Rules for Consistent Income |
| Iron Condor Strategy | Iron Condor Strategy: When to Enter, Adjust, and Exit [Data-Driven] |
| Options Greeks Explained | Options Greeks & DTE: The Complete Timing Reference [With Charts] |

**Expected impact**: +50-200% CTR lift

#### 3.1.2 Implement FAQ Schema on All Educational Pages

**Problem**: No FAQPage schema means no rich results, no PAA inclusion, reduced AIO citation likelihood.

**Actions**:
1. Add 3-5 FAQ questions to each blog post (use the most common questions traders ask about that topic)
2. Wrap in FAQPage JSON-LD schema
3. Questions should be specific, not generic ("What is the best DTE for a covered call on a $50 stock?" beats "What is a covered call?")

**Expected impact**: +25-40% CTR; 3.2x more likely to be cited in AI Overviews

#### 3.1.3 Add HowTo and Article Schema

**Actions**:
1. Mark up all step-by-step guides with HowTo schema
2. Add Article schema with `author` field populated (include credentials: "Options Trader & Portfolio Analyst")
3. Add `dateModified` schema to show freshness

**Expected impact**: Rich result eligibility; faster indexing

#### 3.1.4 Fix Heading Hierarchy + Add Comparison Tables

**Actions**:
1. Ensure every page has exactly one H1, logical H2/H3 progression
2. Add HTML comparison tables for any "vs" or "alternative" content (Born To Sell Alternative page should have a feature comparison table)
3. Tables should use proper `<table>` markup (not divs/images) to qualify for table snippets

**Expected impact**: Table snippets have 80% success rate for comparison queries

---

### Phase 2: Content Consolidation (Days 7-28)

Merge competing pages to free up ranking signals.

#### 3.2.1 Merge Covered Call Articles

**Current state**: 4+ articles competing for "covered call screener" and related terms.

**Action**:
1. Identify the best-performing URL (highest impressions in GSC)
2. Merge the best content from all 4 articles into this URL
3. 301 redirect the other 3 to the primary URL
4. Update all internal links to point to the merged page
5. Target: one authoritative 3,000-5,000 word pillar page

#### 3.2.2 Consolidate 0DTE and Iron Condor Content

Same process: merge overlapping articles into single pillar pages per topic.

#### 3.2.3 Implement Pillar-Cluster Architecture

Going forward, organize content as:

| Pillar Page | Cluster Pages (Distinct Intent) |
|-------------|-------------------------------|
| Covered Call Strategy (comprehensive guide) | Covered Call Screener Setup, Covered Call Taxes, Rolling Covered Calls, Covered Calls for Small Accounts |
| 0DTE Trading Guide | 0DTE Theta Decay, 0DTE Risk Management, 0DTE Entry Signals |
| Iron Condor Strategy | Iron Condor DTE Optimization, Iron Condor Adjustments, Iron Condor vs. Iron Butterfly |
| Wheel Strategy | Wheel Strategy DTE Rules, Wheel Strategy Taxes, Wheel vs. Buy-and-Hold |

**Rule**: One pillar per strategy. Cluster pages only when targeting a clearly different search intent.

**Expected impact**: +100-200% traffic within 4 weeks (ORKA Socials: +200% in <1 month; Backlinko: +466%)

---

### Phase 3: Original Research + Tool Content (Days 14-42)

#### 3.3.1 Publish First Proprietary Research Piece

**Concept**: Leverage Days to Expiry's actual backtesting data — content AI Overviews cannot generate on their own.

**Title ideas**:
- "We Backtested 10,000 Covered Calls Over 24 Months: The Data on What Actually Works"
- "Covered Call Income by DTE: A 12-Month Backtested Analysis [With Charts]"
- "The Real Win Rate of 0DTE Spreads: Platform Data From 6 Months of Trades"

**Why this works**: Original research with statistics is cited **3.2x more** by AI Overviews than generic educational content. It also earns backlinks naturally.

#### 3.3.2 Create Embeddable Tool Pages

**Concept**: Turn parts of the Days to Expiry app into standalone, shareable web pages.

**Tool ideas** (9% AIO trigger rate vs 91% for definitions):
- Free Covered Call Calculator (enter stock, strike, DTE → see projected return)
- Options DTE Visualizer (see theta decay curve by DTE)
- Position Size Calculator for Credit Spreads

**Each tool page should**:
- Be a standalone landing page with its own URL
- Include explanatory content below the tool (200-500 words)
- Have clear CTAs to sign up for the full platform
- Include "Embed this calculator" option (link building)

**Expected impact**: 2.5x more backlinks than blog posts; 3x longer dwell time; AIO-resistant traffic

#### 3.3.3 Add Real Trade Case Studies

**Actions**:
- Publish 2-3 case studies showing actual trades from the platform (with P&L screenshots)
- Format: "How I Made $X on [Strategy] With [Stock] at [DTE] Days to Expiry"
- Include entry/exit rationale, what went right/wrong, lessons learned

**Why this works**: Demonstrates first-hand experience (E-E-A-T), which AI cannot replicate. Content with real trade data gets 40% more AI citations.

---

### Phase 4: Topical Expansion + Authority Building (Days 30-60)

#### 3.4.1 Publish 2-3x Weekly on Missing Topics

**Priority topic list** (all currently uncovered):

| Topic | Why It Matters | Target Keyword |
|-------|---------------|----------------|
| Credit Spread Strategies | High volume, core strategy | "credit spread strategy" |
| Strangle vs. Straddle | Classic comparison query | "strangle vs straddle options" |
| Butterfly Spread Guide | Underserved niche | "butterfly spread strategy" |
| Volatility Skew Explained | Advanced topic, less competition | "what is volatility skew" |
| Gamma Scalping | High-intent advanced traders | "gamma scalping strategy" |
| Portfolio Margin Requirements | YMYL content, high trust needed | "portfolio margin requirements" |
| Options Earnings Plays | Timely, recurring content | "options earnings strategy" |
| Diagonal Spreads | Gap in coverage | "diagonal spread options" |
| Ratio Spreads | Advanced income strategy | "ratio spread strategy" |
| LEAPS vs. Short-Term Options | Comparison content | "LEAPS vs short term options" |

**Publishing cadence**: 2-3 articles per week minimum. Sites publishing 16+ articles/month get 3.5x more traffic than those publishing 0-4.

#### 3.4.2 Launch YouTube Video Versions

- Convert top 5 blog posts into 8-12 minute YouTube videos
- YouTube accounts for **29.5% of all AI Overview citations**
- Videos should include actual platform walkthroughs (screen recordings)
- Embed videos in corresponding blog posts

#### 3.4.3 Activate Reddit Community Strategy

**Subreddits**: r/options, r/Optionswheel, r/thetagang, r/InteractiveBrokers

**Rules**:
- First 4-8 weeks: pure organic participation, no self-promotion
- Answer questions, share insights, build karma
- After establishing presence: share original research and tool pages (not product pitches)
- Reddit CPC is 60-75% cheaper than Google Ads if you do paid promotion later

#### 3.4.4 Build Broker Integration Content Hub

- Create content around the IBKR integration: "How to Sync Your Interactive Brokers Portfolio"
- This is a unique differentiator no competitor can replicate
- Target keywords: "options portfolio tracker IBKR", "Interactive Brokers options analytics"

---

## 4. What PageSeeds Should Automate

These are feature recommendations for the PageSeeds app based on gaps identified in this analysis.

### Priority 1: Content Decay Detection

**What it should do**:
- Monitor GSC data for pages with declining CTR (>20% drop over 30 days)
- Flag pages with position drops of 3+ spots
- Alert when impressions are flat but CTR is declining (SERP feature problem)
- Generate automated "refresh priority" scores

**Why it matters**: Content decay is the #1 hidden cause of plateaus. HubSpot saw +106% traffic from refresh vs. new publishing. Currently, PageSeeds has no automated decay detection.

### Priority 2: AI Overview Citation Tracker

**What it should do**:
- Track which queries trigger AI Overviews for your tracked keywords
- Monitor whether your pages are cited in those AIOs
- Report citation rate trends over time
- Identify AIO trigger growth (which queries are newly getting AIOs)

**Why it matters**: 92% of marketers plan to do GEO but only 40.6% are. This is a massive market opportunity for PageSeeds to be the first desktop SEO tool with native GEO tracking.

### Priority 3: Cannibalization Auto-Detection

**What it should do**:
- Scan GSC data for multiple URLs ranking for the same query
- Flag keyword overlap above a threshold (e.g., >60% shared keywords between two pages)
- Recommend which pages to merge (based on GSC performance data)
- Auto-generate 301 redirect plans

**Why it matters**: Days to Expiry had 4 competing covered call articles. This pattern is common and hard to spot manually in GSC.

### Priority 4: Title Tag + Meta Description Optimizer

**What it should do**:
- Score existing titles against CTR best practices (numbers, brackets, specificity, length)
- Suggest optimized versions using templates
- Track CTR before/after changes
- A/B test framework for title variations

**Why it matters**: Days to Expiry's 0.1% CTR was partly caused by generic titles. Automated title optimization is the highest-ROI feature for low-CTR sites.

### Priority 5: Content Gap Analyzer

**What it should do**:
- Compare your content against competitors' topic coverage
- Identify missing subtopics and strategy guides
- Generate a prioritized content calendar based on search volume × competition × relevance
- Track topical authority score over time

**Why it matters**: Days to Expiry is missing 30+ topics that competitors cover. A content gap analyzer would have flagged this immediately.

### Priority 6: Schema Markup Validator + Generator

**What it should do**:
- Scan pages for missing schema (FAQ, HowTo, Article, SoftwareApplication)
- Auto-generate JSON-LD markup for FAQ sections
- Validate existing schema for errors
- Track rich result eligibility

**Why it matters**: Missing FAQ schema alone cost Days to Expiry +25-40% CTR. Automated schema detection would have caught this.

### Priority 7: Link Building Opportunity Finder

**What it should do**:
- Identify sites linking to competitors but not to you
- Find broken links on relevant finance sites where your content could replace
- Track unlinked brand mentions
- Generate outreach email templates

**Why it matters**: Off-site SEO is the biggest gap for Days to Expiry and most PageSeeds users. Currently no link building features exist.

---

## 5. 60-Day Execution Checklist

### Week 1 (Days 1-7)

| Day | Action | Owner | Done |
|-----|--------|-------|------|
| 1 | Audit all title tags; create rewrite list for every page | SEO | [ ] |
| 2 | Rewrite top 10 highest-impression page titles | SEO | [ ] |
| 3 | Add FAQPage schema to top 5 pages; validate with Google Rich Results Test | Dev | [ ] |
| 4 | Identify all cannibalizing page groups via GSC | SEO | [ ] |
| 5 | Begin merging first content group (covered calls) | SEO + Dev | [ ] |
| 6 | Add Article schema with author credentials to all posts | Dev | [ ] |
| 7 | Set up GSC monitoring dashboard; screenshot baseline metrics | SEO | [ ] |

### Week 2 (Days 8-14)

| Day | Action | Owner | Done |
|-----|--------|-------|------|
| 8 | Rewrite remaining title tags | SEO | [ ] |
| 9 | Add FAQ schema to remaining educational pages | Dev | [ ] |
| 10 | Publish first consolidated pillar page (covered calls) | SEO | [ ] |
| 11 | 301 redirect merged pages to primary URLs | Dev | [ ] |
| 12 | Add comparison tables to Born To Sell Alternative page | SEO | [ ] |
| 13 | Implement HowTo schema on step-by-step guides | Dev | [ ] |
| 14 | **Review**: Measure CTR change from title/schema updates | SEO | [ ] |

### Week 3-4 (Days 15-28)

| Action | Owner | Done |
|--------|-------|------|
| Merge 0DTE articles into single pillar | SEO + Dev | [ ] |
| Merge iron condor articles into single pillar | SEO + Dev | [ ] |
| Publish first original research piece (backtested data) | SEO | [ ] |
| Build first embeddable tool page (covered call calculator) | Dev | [ ] |
| Add 2-3 real trade case studies | SEO | [ ] |
| Create content calendar for 30 missing topics | SEO | [ ] |

### Week 5-8 (Days 29-60)

| Action | Owner | Done |
|--------|-------|------|
| Publish 2-3 articles per week from content calendar | SEO | [ ] |
| Launch tool page with embed/share functionality | Dev | [ ] |
| Publish first YouTube video (top blog post adaptation) | Content | [ ] |
| Begin organic Reddit participation (no promotion) | Community | [ ] |
| Build broker integration content hub | SEO | [ ] |
| Refresh 3 oldest blog posts with updated data | SEO | [ ] |
| **Review (Day 60)**: Compare GSC metrics to baseline | SEO | [ ] |

---

## 6. Success Metrics & Expected Outcomes

### Targets by Week

| Week | Target | Metric |
|------|--------|--------|
| 2 | FAQ schema live on 100% of edu pages | Validation |
| 4 | CTR improved to **0.5%** (5x) | GSC |
| 8 | CTR at **1.5%+** (15x); daily impressions **>10,000** | GSC |
| 12 | First AI Overview citation tracked; branded search trending up | GEO Tool + GSC |

### 90-Day Projections (Conservative)

| Metric | Current | 30 Days | 60 Days | 90 Days |
|--------|---------|---------|---------|---------|
| Daily Impressions | 7,400 | 8,500 | 12,000 | 15,000 |
| CTR | 0.1% | 0.5% | 1.5% | 2.0% |
| Daily Clicks | 8 | 42 | 180 | 300 |
| Avg. Position | 6.9 | 6.5 | 5.5 | 5.0 |

### 90-Day Projections (Optimistic)

| Metric | Current | 30 Days | 60 Days | 90 Days |
|--------|---------|---------|---------|---------|
| Daily Impressions | 7,400 | 9,000 | 14,000 | 20,000 |
| CTR | 0.1% | 0.8% | 2.5% | 3.5% |
| Daily Clicks | 8 | 72 | 350 | 700 |
| Avg. Position | 6.9 | 6.0 | 4.5 | 3.5 |

---

## 7. Risk Factors & Mitigation

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Google algorithm update during campaign | Medium | Diversify traffic (YouTube, Reddit, direct); don't rely solely on organic |
| Content consolidation causes temporary traffic dip | Medium | 301 redirects preserve authority; monitor daily; rollback plan ready |
| Original research doesn't earn AI citations | Low-Medium | Publish on multiple platforms; outreach to finance bloggers; promote on Reddit |
| Publishing 2-3x/week strains resources | Medium | Use PageSeeds AI content generation for first drafts; human edit for quality |
| Competitors publish similar content faster | Low | Proprietary data advantage (backtesting, IBKR integration) cannot be replicated |

---

## 8. Summary: The Core Insight

> **Days to Expiry's SEO plateau is not a ranking problem — it is a compounding problem. Three constraints (AI Overview compression, keyword cannibalization, topical authority deficit) simultaneously create an artificial ceiling. A coordinated 60-day campaign addressing all three together — not sequentially — is the only path to breaking through. The PageSeeds app should evolve to detect and automate prevention of these specific issues.**

---

*Generated: 2026-05-28*
*Based on: 12-dimension deep research analysis with 80+ cited sources*
