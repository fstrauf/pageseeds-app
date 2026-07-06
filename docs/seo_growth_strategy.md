# SEO Growth Strategy — Days to Expiry (daystoexpiry.com)

> Living document. Captures the data-driven diagnosis and the open questions we're iterating on.
> Supersedes the static `seo_action_plan.md` (2026-05-28) with current GSC data.
> Last updated: 2026-07-06

---

## Part 1 — The Diagnosis (data-backed)

### Current state (GSC, 2026-04-07 → 2026-07-05, 89 days)

| Metric | Value | Verdict |
|---|---|---|
| Published articles | 162 | Volume is not the problem |
| Articles with impressions | 105 | 57 get zero — dead weight |
| Articles on page 1 (pos 1-10) | 43 | The opportunity zone |
| Articles on page 2 (pos 11-20) | 35 | Slipped — recoverable |
| Avg position | 11.6 (was 6.9) | **Regression — the core problem** |
| CTR (page-1 articles) | ~0.22% | Should be 1-3% — clicks are broken |
| Indexed (PASS) | 101 | NEUTRAL: 48, UNKNOWN: 26 |

### Position distribution (the whole story)

| Position band | Articles | Implication |
|---|---|---|
| 1-3 (top) | 3 | Only pages capturing real value |
| 4-6 (p1 lower) | 12 | Healthy but CTR underperforming |
| **7-10 (p1 bottom)** | **28** | **Biggest lever — small push doubles visibility** |
| 11-20 (p2) | 35 | Slipped from p1 — recoverable |
| 21+ (p3+) | 27 | Unlikely to recover |

### The reframe

**This is not a content volume problem.** It's three compounding failures:
1. Page-1 content barely gets clicked (CTR collapse)
2. Articles are slipping from p1 → p2 (position regression)
3. A third of the catalog is dead weight diluting topical authority

Publishing more onto this base deepens all three.

---

## Part 2 — The Five Levers (prioritized)

### Lever 1 — Fix the page-1 CTR collapse (highest ROI, fastest)

Striking-distance articles with thousands of impressions but near-zero clicks:

| Article | Impressions | Position | CTR | Clicks |
|---|---|---|---|---|
| theta-decay-dte-guide | 3,758 | 7.1 | 0.03% | ~1 |
| naked-puts-vs-csp | 1,975 | 7.5 | 0.05% | ~1 |
| covered-call-tax-rules | 1,727 | 10.0 | 0.06% | ~1 |
| best-stocks-wheel-strategy | 3,005 | 9.5 | 0.27% | ~8 |
| spx-section-1256-tax | 1,414 | 9.7 | 0.14% | ~2 |

449 `fix_ctr_article` tasks ran but CTR only moved 0.1% → 0.22%. Two possible causes — **must diagnose before more effort:**
1. Title fixes haven't re-rendered in SERPs (Google takes 2-6 weeks)
2. AI Overviews absorbing clicks (no title fix can recover a zero-click SERP)

**Status:** ⏳ Open — see Q1/Q2 below. Need to check live SERPs.

### Lever 2 — Push the 18 striking-distance articles (positions 7-13)

~17,000 combined impressions, all within a small push of doubling visibility. Top targets: theta-decay-dte-guide, best-stocks-wheel-strategy, naked-puts-vs-csp, covered-call-tax-rules, spx-section-1256-tax, interactive-brokers-flex-query.

**Action:** Internal-link boost + depth expansion on these 18 specifically. Run `indexing_health_campaign` + `cluster_and_link` targeted at them.

### Lever 3 — Recover the page-2 slip (35 articles at 11-20)

Regression took the site 6.9 → 11.6. Three likely causes: cannibalization (being fixed), quality dilution (100 articles Jan-Apr), engagement death spiral (low CTR → low dwell → lower rank).

**Action:** After consolidations land, monitor these 35 for recovery over 4-6 weeks. If no recovery → quality signal problem, not cannibalization.

### Lever 4 — Cut the dead weight (25+ indexed-but-zero-impression articles)

25 articles indexed (PASS) but zero impressions: `options-trading-for-dummies`, `protective-put-option`, `bear-call-spread-strategy`, etc. They target keywords Investopedia dominates and cannot win. They dilute the site's topical authority signal.

**Status:** ⏳ Open — but see Q3/Q4. The user's concern: noindexing is treating the symptom. The root cause is the keyword research targeting unwinnable keywords. **Need systematic fix in the research workflow, not just cleanup.**

### Lever 5 — Strategic pivot: content AI Overviews can't replicate

Shift from educational content (AIO-vulnerable, Investopedia-dominated) to proprietary content:
- **Original research** (backtested data) — 3.2x more AIO citations
- **Tool/calculator pages** — AIO trigger rate 9% vs 91% for definitions
- **IBKR integration guides** — unique, no competitor can replicate
- **Real trade case studies** — E-E-A-T signal for YMYL finance

Evidence this works: top earners are already data/list content (`best-stocks-for-selling-cash-secured-puts` 6,511; `best-stocks-wheel-strategy` 3,005). Best CTR on high-volume page is platform-specific (`interactive-brokers-flex-query` 0.51%).

**Status:** ⏳ Open — see Q5. Need to review the keyword research + article generation prompts to rephrase for this pivot.

---

## Part 3 — Open Questions (investigated 2026-07-06)

### Root-cause synthesis

Your instinct is right: noindexing dead weight treats the symptom. The root cause is **the research→write pipeline has no concept of "winnability."** It selects keywords by a single hardcoded heuristic (`KD ≤ 30` + volume sort, with a silent fallback that drops KD entirely), and the article writer receives no competitive context at all. So the pipeline *systematically produces* content that targets keywords Investopedia dominates and AI Overviews absorb. Fixing this upstream prevents the next 25 dead-weight articles from ever being created.

The pipeline investigation confirmed 8 specific gaps (see Appendix). The five questions below address them.

---

### Q1 — How do we pull the actual SERP? Where does the data come from?

**Answer: DataForSEO SERP API. No webbridge needed.**

DataForSEO has a dedicated SERP API the app doesn't use yet. The app currently only calls DataForSEO **Labs** endpoints (keyword suggestions, bulk KD). The SERP API is a separate product:

| Endpoint | What it returns | Relevance |
|---|---|---|
| `/v3/serp/google/organic/live/advanced/` | Full SERP including **AI Overviews, featured snippets, PAA, shopping** + organic results | AIO risk detection |
| `/v3/serp/google/ai_mode/live/advanced/` | Google AI Mode results directly | AIO citation tracking |
| `/v3/ai_optimization/llm_mentions/` | Whether your domain is cited in ChatGPT/Claude/Gemini/Perplexity | GEO / AIO citation tracking |

**Google Search Console does NOT help here** — it only has your own impressions/clicks, not competitor SERP features or AIO presence for queries you don't rank for. So GSC can't tell you "does this keyword trigger an AIO."

**Cost note:** DataForSEO is paid per-task, but the SERP Live Advanced endpoint is ~$0.001-0.006 per keyword. Scoring a research batch of 50 keywords costs pennies. The bigger cost is the existing Labs calls — adding SERP feature detection is marginal on top.

**The Ahrefs situation (resolved):** The "Ahrefs" provider is **not the Ahrefs API** — it's a CapSolver-bypassed scraper of Ahrefs' free public tools (`seo/mod.rs:36-119` solves Cloudflare Turnstile, then scrapes `v4/stGetFreeKeywordIdeas`). It's the **default** provider (`projects.seo_provider` defaults to `'ahrefs'`) but it's a fragile scraper that (a) discards all non-organic SERP features (`seo/keywords.rs:680`) and (b) breaks whenever Ahrefs changes their captcha. DataForSEO is your only real API integration. **Recommendation: make DataForSEO the sole provider and remove the Ahrefs scraper path** (see Game Plan WS2).

---

### Q2 — Can the agent do that? Do we need a webbridge?

**No webbridge needed.** Since SERP data comes from the DataForSEO API, we just add a `serp_features()` method to the `SeoDataProvider` trait and call it during research. No browser automation, no scraping, no captcha-solving. The agent (internal or external) calls the same tool deterministically.

The `kimi-webbridge` option is only useful for one-off manual spot-checks of a live SERP (e.g., "let me see what Google actually shows for this query right now"). It's not the systematic path. Skip it unless DataForSEO disagrees with reality and you need to debug.

---

### Q3 — Can we note in the keyword research workflow which keywords we're giving up on?

**Finding:** No such concept exists in the codebase. The research selector (`engine/exec/research/autocomplete.rs:146-270`) uses:
- `KD ≤ 30` (hardcoded, `autocomplete.rs:154`) — and a **silent fallback** (`autocomplete.rs:181-198`) that drops the KD filter entirely if it filters too much
- Volume sort
- Intent ≠ navigational
- Coverage-gap filter (skips already-covered topics)

There is **no AIO risk score, no competitor-authority signal, no "unwinnable" classification, and no "avoid" marking.** The legacy prompt `prompts/final_selection_keywords.md` had richer criteria (KD < 40, volume > 500) but it's unused — replaced by the deterministic function that's simpler.

**The systematic fix — a winnability classifier:**

Add a `WinnabilityScore` computed per keyword during research, combining:

| Factor | Source | Signal |
|---|---|---|
| AIO presence | DataForSEO SERP API (Option C above) | Does the SERP trigger an AI Overview? |
| Competitor authority | Ahrefs SERP overview (stop filtering organic-only) | Are top-3 results DR 90+ domains (Investopedia, tastylive)? |
| Intent type | `intent_classifier.rs` (exists) | Informational = high AIO risk; comparison/tool = lower |
| Authority gap | Site DR vs competitor DR | Can this site realistically compete? |
| Existing coverage | `coverage_filter.rs` (exists) | Already covered? Skip. |

Classify each keyword into one of three buckets:

| Bucket | Meaning | Action |
|---|---|---|
| **Target** | Winnable: low AIO risk, competitors are beatable, intent matches our assets | Create article |
| **Differentiate** | Winnable ONLY with a proprietary angle (backtest data, tool, IBKR-specific) | Create article with explicit differentiation directive |
| **Avoid** | Unwinnable: AIO-dominated, Investopedia owns it, or authority gap too large | Mark "avoid" in research artifact; don't create article |

Store the bucket + score in the `research_final_selection` artifact so the picker UI shows it and the writer receives it. This is the systematic root-cause fix — it prevents dead-weight articles at the source.

**No fallbacks.** The current selector has a silent fallback (`autocomplete.rs:181-198`) that drops the KD filter if it filters too much, returning low-quality keywords rather than failing. **Remove it.** If a research run finds no keywords that meet the winnability bar, it should return empty with a clear message ("no winnable keywords found for these seeds — refine the territory or seeds"), not fabricate candidates. Iterate on the inputs, don't lower the bar. The classifier uses DataForSEO SERP API data (Q1) as a primary signal, not the Ahrefs scraper.

---

### Q4 — How do we do this systematically? (noindexing is treating symptoms)

**You're right — noindexing is cleanup, not prevention.** The two-layer approach:

**Layer 1 — Prevention (the root cause):** Implement the winnability classifier (Q3). Future keyword research won't propose unwinnable keywords. This stops the bleeding.

**Layer 2 — Remediation (the existing dead weight):** For the 25 indexed-but-zero-impression articles already published, the noindex/consolidate decision should be driven by the *same* winnability score, computed retroactively:
- If a keyword is now "Avoid" → noindex the article (it will never rank)
- If a keyword is "Differentiate" → rewrite the article with a proprietary angle rather than noindexing
- If a keyword is "Target" but the article ranks poorly → it's a content quality / internal-linking problem, not a keyword problem (Lever 2/3)

This means **don't noindex anything yet.** First build the classifier, run it retroactively over the existing 56 zero-impression articles, and let the score decide — rather than blanket-noindexing based on "zero impressions" alone (which could kill articles that just need better internal linking or a content refresh).

---

### Q5 — Review the prompts leading to article creation

**Finding — three critical gaps in the article-generation prompts:**

**Gap 1: `write_article` loads NO skill.** This is the most surprising finding. `ContentHandler::plan()` (`handlers.rs:220-225`):
```rust
if is_hub {
    vec![step.with_param(step_params::SKILL, "hub-write")]
} else {
    vec![step]  // <-- no skill param
}
```
Regular articles fall through to a generic boilerplate prompt (`handlers.rs:1194-1213`) that contains only: task ID, target keyword, KD, volume, and mechanical format rules (MDX structure, filename convention, publish date, internal link format). **There is no content strategy, no differentiation directive, no competitive context, no instruction about content type.**

**Gap 2: Computed insights are discarded.** During research, the pipeline computes `recommended_title`, `selection_reason`, traffic estimates, and themes. But the provenance artifact passed to `write_article` carries only `{"keyword": "X"}` (`keyword_selection.rs:578`). Everything strategic is dropped before the writer sees it.

**Gap 3: The writer gets no SERP/competitive landscape.** Even the hub-write skill (`skills/hub-write/SKILL.md`) is purely structural (MDX format, frontmatter, word count). Neither skill tells the writer *who they're competing against, what the SERP looks like, or how to differentiate.*

**The fix — three changes:**

1. **Create a `content-write` skill** (`skills/content-write/SKILL.md`) for regular articles, covering:
   - Content-type directive: when to write educational vs. proprietary/data-driven/tool content (driven by the winnability bucket from Q3)
   - Differentiation requirement: "Do not produce generic educational content that duplicates Investopedia. For 'Differentiate' keywords, lead with proprietary data, real examples, or platform-specific angles."
   - E-E-A-T directives: first-person experience, real data, credentials
   - Structural quality: comparison tables (80% snippet success), FAQ schema hooks, original data points

2. **Thread the winnability context through the data plumbing:**
   - `build_content_task_description` (`keyword_selection.rs:546`) → add winnability bucket, competitor landscape, recommended angle
   - `build_keyword_provenance_artifact` (`keyword_selection.rs:578`) → carry the full context, not just `{"keyword":"X"}`
   - `ContentHandler::plan()` → load the `content-write` skill for all articles (not just hubs)

3. **Stop the Ahrefs parser from discarding SERP features** (`seo/keywords.rs:680-684`): capture AIO/snippet/PAA presence so the winnability classifier has the data.

---

## Game Plan (concise)

Prioritized workstreams. Each has a clear acceptance test.

### WS1 — `write_article` skill + competitive context  `[priority: highest]`
The article writer currently loads **no skill** and sees only `keyword + KD + volume`. Fix the prompts and the data plumbing.

- **Create `skills/content-write/SKILL.md`** — tone (authoritative, first-person where applicable), differentiation directive ("do not duplicate Investopedia; for 'Differentiate' keywords lead with proprietary data / real examples / platform angles"), content-type guidance (educational vs data-driven vs tool), E-E-A-T requirements, structural quality (comparison tables, FAQ hooks, original data points).
- **Thread context through the pipeline**: `build_keyword_provenance_artifact` (`keyword_selection.rs:578`) currently passes only `{"keyword":"X"}` — carry winnability bucket, competitor landscape, recommended angle, intent. `ContentHandler::plan()` (`handlers.rs:220`) loads the skill for all articles, not just hubs.
- **Acceptance test**: run `write_article` once on a striking-distance keyword (e.g. `theta decay dte`). Compare the output to the existing article. Does it differentiate? Use proprietary angles? Avoid generic educational prose? If yes → ship.
- **Effort**: ~1-2 days. **Status**: ready to start.

### WS2 — DataForSEO-only + SERP feature API + winnability classifier  `[priority: high]`
The systematic root-cause fix. Replaces the fragile Ahrefs scraper + removes the fallback.

- **Add `serp_features()` to `SeoDataProvider` trait** → call DataForSEO `/v3/serp/google/organic/live/advanced/`. Returns AIO presence, featured snippets, PAA, + organic results with competitor domains.
- **Make DataForSEO the sole provider**: change `projects.seo_provider` default to `'dataforseo'`; remove/deprecate the Ahrefs CapSolver scraper path (`seo/ahrefs.rs`, `seo/keywords.rs:680` organic-only filter, `seo/mod.rs:36-119` captcha solver). This also kills the SERP-feature-discard bug at the source.
- **Implement winnability classifier** (Q3): score each keyword Target / Differentiate / Avoid using AIO risk + competitor authority + intent + authority gap.
- **Remove the silent fallback** (`autocomplete.rs:181-198`): fail hard with a clear message if no keywords meet the bar. Iterate on seeds, don't fabricate.
- **Acceptance test**: a `research_keywords` run returns only Target/Differentiate keywords with AIO scores; no Avoid keywords leak through; no silent fallback fires; zero dependency on CapSolver.
- **Effort**: ~3-4 days. **Status**: design ready (this doc); blocked on WS1 for the writer-side consumption of the new data.

### WS3 — Cannibalization merges  `[priority: high, in progress]`
- Rebuild app (slug fix committed but not yet in running binary) → apply the 14/16 correct merges via the review queue.
- **Acceptance**: merges produce canonical URLs; consolidated clusters track position recovery over 4-6 weeks.
- **Effort**: rebuild + ~1 day to process queue.

### WS4 — Dead-weight remediation  `[priority: medium, after WS2]`
- Run the winnability classifier (WS2) retroactively over the 56 zero-impression articles.
- Disposition per score: **Avoid** → noindex; **Differentiate** → rewrite with proprietary angle; **Target** (but underperforming) → internal-link boost, not removal.
- **Do NOT blanket-noindex before WS2.** The score decides, not "zero impressions" alone — some articles may just need linking/refresh.
- **Acceptance**: every zero-impression article has a documented disposition + action.
- **Effort**: ~1 day after WS2 ships.

### WS5 — Striking-distance push  `[priority: medium, parallel]`
- Internal-link boost + depth expansion on the 18 articles at positions 7-13 (~17,000 combined impressions).
- **Acceptance**: weighted avg position moves 11.6 → toward 8-9 within 4-6 weeks.

---

### The number to watch
**Weighted average position.** Climbing 11.6 → 6-7 = death spiral reversing. Stuck at 11+ = the quality-signal problem is deeper and WS4/WS5 must lead before any new publishing.

### Sequencing
```
WS1 (write_article skill)  ──┐
                             ├──▶ WS2 (winnability + DataForSEO SERP) ──▶ WS4 (dead-weight remediation)
WS3 (merges, rebuild)      ──┘                                          ║
                                                                       ║
WS5 (striking-distance push) ──────────────────────────────────────────▶ (parallel, ongoing)
```
Start WS1 and WS3 now (independent). WS2 is the root-cause build — start once WS1's data-plumbing shape is settled (they share the `keyword_selection.rs` → `write_article` pipeline). WS4 follows WS2. WS5 runs in parallel throughout.

---

## Appendix — Pipeline gaps (from investigation)

| Gap | Status | Impact |
|---|---|---|
| A. No SERP feature data | Ahrefs parser discards non-organic results (`keywords.rs:680`) | Can't detect AIO risk |
| B. No competitor authority signal | `SerpEntry` has domain but no DR/UR | Can't assess winnability |
| C. No "unwinnable" classification | Only KD≤30, silently bypassed | Dead weight produced systematically |
| D. No AIO-risk scoring | Zero references in code | CTR collapse undiagnosed |
| E. Article writer gets no competitive context | `write_article` loads no skill | Generic content produced |
| F. Single-variable selection | KD + volume only | Multi-factor winnability impossible |
| G. Shallow intent classification | Bag-of-keywords matcher | AIO risk not correlated with intent |
| H. Computed insights discarded | Provenance artifact = `{"keyword":"X"}` only | Writer blind to research findings |

### Key file paths
- Research selector: `src-tauri/src/engine/exec/research/autocomplete.rs:146`
- Article handler (no skill): `src-tauri/src/engine/workflows/handlers.rs:220`
- Generic fallback prompt: `src-tauri/src/engine/workflows/handlers.rs:1194`
- Ahrefs SERP filter (organic-only): `src-tauri/src/seo/keywords.rs:680`
- Provenance artifact (keyword only): `src-tauri/src/engine/keyword_selection.rs:578`
- SEO provider trait (no SERP method): `src-tauri/src/seo/provider.rs:17`
