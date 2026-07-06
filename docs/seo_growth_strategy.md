# SEO Growth Strategy — Days to Expiry (daystoexpiry.com)

> Last updated: 2026-07-07 — 5 workstreams, 2 complete, 3 ready to execute.

---

## Status at 2026-07-07 (handoff)

### Completed (code written, verified, committed)

**WS1 — `write_article` skill + agentic file operations** ✅
- New `skills/content-write/SKILL.md` — differentiation directive, E-E-A-T, tone, structure
- `ContentHandler::plan` loads it for all non-hub articles (was: no skill at all)
- Bridge fs support (`kimi-acp-openai-bridge`): `initialize` capabilities fix, `fs/read_text_file` + `fs/write_text_file` handlers, `X-Kimi-Workdir` header
- Workdir threading (PageSeeds): `project_path` flows through 7-function chain to the bridge
- `write_article_smoke` binary for prompt iteration without the app
- **Acceptance test passed**: agent read 4 existing articles, wrote a 2,149-word differentiated gamma-scalping article with real internal links

**WS2 — DataForSEO-only + SERP API + winnability classifier** ✅
- `serp_features()` on `SeoDataProvider` trait → DataForSEO `/v3/serp/google/organic/live/advanced/` (AIO, snippets, PAA, competitor domains)
- DataForSEO as sole default provider (factory + new projects + all fallbacks). Ahrefs accepted for backwards compat but `serp_features()` returns error on Ahrefs.
- `seo/winnability.rs`: scores keywords Target / Differentiate / Avoid (6 unit tests)
- No-fallback selection: hard fail if no keywords meet the quality bar
- Pipeline integration: `enrich_with_winnability()` wires into `exec_research_final_selection` (non-fatal — keywords without scores pass through unchanged)

### Ready to execute (needs your action — app rebuild)

**WS3 — Cannibalization merges** ⏳
- Slug fix committed to main (`9dcc866`). Fresh audit 2026-07-06: 16 clusters, 14/16 correct, zero non-canonical URLs.
- **To do**: rebuild the PageSeeds app, restart the kimi-acp-bridge, process the `consolidate_cluster` review queue in the UI. Cancel the 35 stale May–Jun tasks.

**WS4 — Dead-weight remediation** ⏳
- 56 zero-impression articles categorized: ~15 Avoid, ~20 Differentiate, ~21 Target
- **Decision**: don't noindex yet. Rebuild first, run `research_keywords` (which now auto-scores winnability via WS2), then let the scores decide.

**WS5 — Striking-distance push** ⏳
- 18 articles at positions 7-13 identified with priorities (see §WS5 below)
- Top 5 by impact: `theta-decay-dte-guide` (3,758), `best-stocks-wheel-strategy` (3,005), `naked-puts-vs-csp` (1,975), `covered-call-tax-rules` (1,727), `interactive-brokers-flex-query` (1,362)

### How to pick this up

**Branches:**
| Repo | Branch | Status |
|---|---|---|
| `pageseeds-app` | `feat/seo-growth-strategy` | 4 commits ahead of main — ready to merge |
| `kimi-acp-openai-bridge` | `main` | 1 commit with fs handlers — already on main |

**Rebuild steps:**
```bash
# 1. PageSeeds app
cd pageseeds-app
git checkout feat/seo-growth-strategy
cargo build --manifest-path src-tauri/Cargo.toml  # or npm run tauri dev

# 2. Restart the bridge (to pick up fs handlers)
kill $(lsof -ti:8080)
nohup kimi-acp-bridge > /tmp/kimi-bridge.log 2>&1 &

# 3. In the app UI, go to Overview → Cannibalization → process review queue
# 4. Run research_keywords (now produces winnability scores)
# 5. Compare the generated article quality using write_article_smoke
```

### Key files changed

- `src-tauri/skills/content-write/SKILL.md` — new content strategy skill
- `src-tauri/src/seo/winnability.rs` — new winnability classifier
- `src-tauri/src/bin/write_article_smoke.rs` — new isolated test tool
- `src-tauri/src/seo/provider.rs` — `serp_features()` on the trait
- `src-tauri/src/seo/dataforseo.rs` — DataForSEO SERP API implementation
- `src-tauri/src/seo/mod.rs` — factory defaults to DataForSEO
- `src-tauri/src/engine/exec/research/autocomplete.rs` — enrichment + fallback removal
- `src-tauri/src/engine/agent.rs` — workdir threading
- `src-tauri/src/rig/compat/kimi.rs` — `X-Kimi-Workdir` header
- `kimi-acp-openai-bridge/src/kimi_acp_bridge/acp_client.py` — fs handlers + capabilities fix
- `kimi-acp-openai-bridge/src/kimi_acp_bridge/server.py` — workdir header extraction

### The number to watch
**Weighted average position** — currently 11.6. Climbing toward 6-7 = death spiral reversing. Stuck at 11+ = WS4/5 must lead before new publishing.

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

### WS1 — `write_article` skill + competitive context  `[DONE]`
~~The article writer currently loads **no skill** and sees only `keyword + KD + volume`.~~

**Completed 2026-07-07.** What shipped:
- **`skills/content-write/SKILL.md`** — tone, differentiation directive, E-E-A-T, content-type guidance, structural quality
- **`ContentHandler::plan`** loads `content-write` for all non-hub articles (was: no skill at all)
- **Bridge fs support** (kimi-acp-openai-bridge): fixed `initialize` capabilities (`clientCapabilities.fs`), implemented `fs/read_text_file` + `fs/write_text_file` handlers with path safety, extract `X-Kimi-Workdir` header for session cwd
- **Workdir threading** (PageSeeds): `project_path` flows through the 7-function chain to the `X-Kimi-Workdir` header
- **`write_article_smoke` binary** — isolated CLI tool for testing the full pipeline without the app

**Acceptance test result**: agent read 4 existing articles for context, wrote a 2,149-word differentiated gamma-scalping article with real internal links, worked examples, comparison tables, and honest E-E-A-T risk assessment. Dramatically better than the old generic output.

**Deferred to WS2**: threading winnability bucket/competitor data through the provenance artifact (the data doesn't exist yet).

### WS2 — DataForSEO-only + SERP feature API + winnability classifier  `[DONE]`
**Completed 2026-07-07.** The systematic root-cause fix for the dead-weight article problem. What shipped:

- **`serp_features()` on `SeoDataProvider` trait** → DataForSEO calls `/v3/serp/google/organic/live/advanced/`. Detects AI Overviews, featured snippets, PAA, collects competitor domains.
- **DataForSEO as sole default** → factory + new projects + all fallbacks default to `dataforseo`. Ahrefs accepted for backwards compat but `serp_features()` returns an error on Ahrefs.
- **Winnability classifier** (`seo/winnability.rs`): scores keywords Target / Differentiate / Avoid based on AIO risk, authority competitor count, KD, intent. 6 unit tests.
- **No-fallback selection** → hard fail if no keywords meet the quality bar. Iterate on seeds, don't fabricate.
- **Pipeline integration** → `enrich_with_winnability()` calls `serp_features()` for each selected keyword, scores via `assess()`, attaches bucket + reason to the picker output. Non-fatal: keywords without scores pass through unchanged.

### WS3 — Cannibalization merges  `[READY — needs app rebuild]`
- Slug fix is committed to main (`9dcc866`). Fresh audit ran 2026-07-06: 16 merge clusters, 14/16 keeper decisions correct, zero non-canonical URLs.
- **To apply**: rebuild the PageSeeds app (`npm run tauri build` or dev run), restart the kimi-acp-bridge (the fs handlers need the updated binary), then process the `consolidate_cluster` review queue in the UI.
- The 35 stale consolidate_cluster tasks from May–Jun should be cancelled/replaced by the fresh audit output.
- **Acceptance**: merges produce canonical URLs; consolidated clusters track position recovery over 4-6 weeks.

### WS4 — Dead-weight remediation  `[READY — needs app rebuild for SERP scoring]`

The 56 zero-impression articles fall into three likely categories (rough categorization without SERP data — the winnability classifier gives precise scores once the app is rebuilt):

**Likely Avoid (~15 articles):** Informational queries Investopedia / AIO-dominated. `what-are-greeks-options`, `what-are-greeks-faq`, `options-trading-for-dummies`, `protective-put-option`, `dividend-income-strategy`, `how-to-make-money-with-stocks-beginner`, `sell-covered-calls-guide`, `selling-puts-for-income`, `income-from-idle-cash`, etc.

**Likely Differentiate (~20 articles):** Calculator/tool pages that need the actual tool embedded. `covered-call-screener-*` (5 variants from the Apr 2026 sprint), `options-calculator`, `option-price-calculator`, `options-profit-calculator`, `cboe-options-calculator`, `long-call-option-calculator`, `portfolio-visualizer`, `covered-call-portfolio-tracking`. These are the right strategy (proprietary tools) but have zero impressions — likely because they're thin pages without the actual calculator embedded, OR they're cannibalizing each other.

**Likely Target (~21 articles):** Strategy-specific articles that should rank but don't. `0dte-options-strategy`, `bear-call-spread-strategy`, `call-spreads-vs-put-spreads`, `straddle-vs-strangle`, `leaps-options-strategy`, `wheel-options-trading-strategy-pdf`, etc. These likely need internal-link boost + cannibalization resolution (WS3) to gain visibility, not removal.

**Decision:** Do NOT noindex yet. Rebuild the app, run a `research_keywords` run (which now scores winnability via WS2), and apply the classifier retroactively to these 56. Then noindex the Avoid bucket, rewrite the Differentiate bucket with the content-write skill, and boost the Target bucket via internal links.

### WS5 — Striking-distance push  `[READY — can run in the app]`

The 18 articles at positions 7-13 with >200 impressions. Total combined: ~17,000 impressions. These are the highest-leverage targets for a ranking push:

| Article | Impr | Pos | CTR% | Type | Priority |
|---|---|---|---|---|---|
| theta-decay-dte-guide | 3,758 | 7.1 | 0.03 | CTR fix (AIO?) | Highest |
| best-stocks-wheel-strategy | 3,005 | 9.5 | 0.27 | Internal links | High |
| naked-puts-vs-csp | 1,975 | 7.5 | 0.05 | CTR fix | High |
| covered-call-tax-rules | 1,727 | 10.0 | 0.06 | CTR fix | High |
| spx-section-1256-tax | 1,414 | 9.7 | 0.14 | CTR + links | High |
| interactive-brokers-flex-query | 1,362 | 8.0 | **0.51** | Replicate this pattern! | Medium |
| best-brokers-options-trading | 993 | 13.0 | 0.0 | Needs attention | Medium |
| best-stocks-to-sell-put-options-june-202 | 726 | 10.6 | 0.0 | Stale (june) — refresh or redirect | Low |
| best-stocks-iron-condors | 656 | 11.3 | 0.76 | Healthy, just needs push | Medium |
| best-stocks-to-sell-put-options-screener | 616 | 10.9 | 0.97 | Healthy, just needs push | Medium |
| best-stocks-pmcc-leaps | 577 | 8.1 | 0.17 | CTR + links | Medium |
| credit-spread-width-dte | 543 | 9.5 | 0.0 | CTR fix | Medium |
| wheel-strategy-guide | 486 | 8.2 | 0.41 | Internal links | Medium |
| how-to-sell-csp-ib | 483 | 9.8 | 0.41 | Platform-specific — extend | Medium |
| interactive-brokers-portfolio-analysis | 340 | 9.8 | 0.0 | Needs attention | Medium |
| covered-call-screener-tools | 335 | 12.5 | **1.49** | Replicate this pattern! | Medium |
| best-stocks-to-sell-put-options-may-202 | 328 | 12.6 | 0.0 | Stale (may) — refresh or redirect | Low |
| wheel-options-trading-strategy-review | 245 | 10.0 | 0.0 | CTR fix | Low |

**Top 5 by impact:** theta-decay-dte-guide (3,758), best-stocks-wheel-strategy (3,005), naked-puts-vs-csp (1,975), covered-call-tax-rules (1,727), interactive-brokers-flex-query (1,362).

**Pattern observation:** The highest-CTR articles at page-1-bottom are `covered-call-screener-tools` (1.49%) and `interactive-brokers-flex-query` (0.51%) — both are TOOL/PLATFORM-specific content. The lowest-CTR articles are informational (`theta-decay-dte-guide` 0.03%, `covered-call-tax-rules` 0.06%). This directly validates the Lever 5 strategy: tool/platform content captures clicks that informational content loses to AIOs.

**Action:** Run `indexing_health_campaign` + `cluster_and_link` on the top 10 by impact. Refresh the stale monthly stock-pick articles (june/may) or consolidate them into a single evergreen list.

---

### The number to watch
**Weighted average position.** Climbing 11.6 → 6-7 = death spiral reversing. Stuck at 11+ = the quality-signal problem is deeper and WS4/WS5 must lead before any new publishing.

### Sequencing
```
WS1 (write_article skill)  ✅ DONE
WS2 (winnability + DataForSEO SERP) ──▶ ✅ DONE ──▶ WS4 (dead-weight remediation, now unblocked)
WS3 (merges, rebuild)      ──▶ ready, needs app rebuild
WS5 (striking-distance push) ──────────────────────────────────────────▶ (parallel, ongoing)
```
WS1 and WS2 are complete. WS4 is now unblocked — the winnability classifier can be run retroactively over the 56 zero-impression articles. WS3 needs the app rebuilt for the cannibalization merges. WS5 runs in parallel.

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
