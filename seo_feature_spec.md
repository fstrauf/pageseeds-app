# SEO Feature Specification

Generated: 2026-05-22 01:12 UTC
Triggered by: Feature spec from content_review audit (task-7d64c863-fb1a-4e8d-84d8-2252b4008689)

## Executive Summary

The site has 108 pages not indexed by Google, with 12 flagged for content fixes due to zero word counts and zero internal links, indicating a critical content rendering or generation failure. Additionally, 6 articles use temporal URLs with year-specific slugs that will require ongoing maintenance, and 21 articles suffer from title token duplication that dilutes ranking signals.

## P0 — Code Changes Required (Developer)

### Zero Word Count on Not-Indexed Pages

- **Problem**: 12 not-indexed pages report `word_count: 0`, suggesting content is not being rendered or extracted properly.
- **Evidence**: 
  - `https://brewedlate.com/blog/234-what-is-coffee-blooming-how-to-bloom-pour-over-coffee`
  - `https://brewedlate.com/blog/45-best-affordable-coffee-brewing-setup`
  - `https://brewedlate.com/blog/265-hub-coffee-beans`
  - `https://brewedlate.com/blog/how-to-make-coffee-without-a-coffee-maker`
  - `https://brewedlate.com/blog/monthly-coffee-subscription-australia`
  - `https://brewedlate.com/blog/cheapest-coffee-beans-australia-2026`
  - `https://brewedlate.com/blog/44-coffee-freshness-by-origin`
  - `https://brewedlate.com/blog/australia-coffee-deals-guide`
  - `https://brewedlate.com/blog/62-best-cold-brew-coffee-maker-australia`
  - `https://brewedlate.com/blog/63-best-stovetop-coffee-maker-moka-pot`
  - `https://brewedlate.com/blog/41-best-coffee-beans-for-cold-brew`
  - `https://brewedlate.com/blog/49-trending-specialty-coffee-2025`
- **Root Cause**: The word count extraction logic or MDX-to-HTML rendering pipeline is failing to parse content body, possibly due to malformed frontmatter, missing body delimiters, or a regression in the `count_words()` utility.
- **Fix**: 
  - Audit `src-tauri/src/content/ops.rs` → `count_words()` and `read_file_metadata()` to ensure they handle edge cases (empty body, malformed frontmatter, non-standard delimiters).
  - Audit the MDX rendering pipeline in the frontend/static generator to confirm content is not being stripped by a layout or template error.
- **Estimated Effort**: medium

### Page Bloat from Excessive Tables

- **Problem**: 10 articles have 31–59 tables each, inflating HTML payload and degrading mobile performance.
- **Evidence**: 
  - `./src/blog/posts/06_cheapest_coffee_beans_australia_2026.mdx` → 31 tables, 36,879 bytes
  - `./src/blog/posts/272_pour_over_coffee_maker.mdx` → 59 tables, 33,598 bytes
  - `./src/blog/posts/284_french_press_coffee_grind.mdx` → 56 tables, 24,539 bytes
  - `./src/blog/posts/285_coffee_to_water_ratio_french_press.mdx` → 55 tables, 18,086 bytes
- **Root Cause**: Content generation pipeline or skill templates are overusing markdown tables for structured data instead of semantic HTML, lists, or dedicated components.
- **Fix**: 
  - Update the content generation skill/template in `.github/skills/` or `src-tauri/src/skills/` to discourage table-heavy output for non-tabular data.
  - Add a post-generation deterministic step in `engine/exec/content/fix_apply.rs` or similar to convert simple two-column tables to definition lists or plain paragraphs.
- **Estimated Effort**: small

## P1 — Content Fixes (PageSeeds Can Handle)

### Title Token Duplication

- **Problem**: 21 articles have titles with repeated tokens (e.g., "Coffee" appearing twice), diluting topical relevance signals.
- **Affected Pages**:
  - `src/blog/posts/how-to-make-coffee-without-a-coffee-maker.mdx`
  - `src/blog/posts/235_can_you_reuse_coffee_grounds_what_you_need_to_know.mdx`
  - `src/blog/posts/03_coffee_subscription_gift_guide_australia.mdx`
  - `src/blog/posts/105_french_press_coffee_brewing_guide_step_by_step_for_perfect_extraction.mdx`
  - `src/blog/posts/152_light_roast_vs_dark_roast.mdx`
  - `src/blog/posts/71_yirgacheffe_coffee_guide_to_ethiopia_s_signature_region.mdx`
  - `src/blog/posts/72_sidamo_coffee_rich_complex_flavors_from_ethiopia_s_highlands.mdx`
  - `src/blog/posts/165_home_coffee_roasting_beginners_guide.mdx`
  - `src/blog/posts/2025-10-06-learnedlate-coffee-brand-and-platform-overview.mdx`
  - `src/blog/posts/best_coffee_beans_2025.mdx`
  - `src/blog/posts/best_coffee_grinder_2025.mdx`
  - `src/blog/posts/subscription-vs-one-off-cost-breakdown.mdx`
  - `src/blog/posts/240_coffee_roasters_auckland.mdx`
  - `src/blog/posts/245_coffee_roast_analyzer_guide.mdx`
  - `./src/blog/posts/269_water_filter.mdx`
  - `src/blog/posts/67_how_to_use_a_moka_pot.mdx`
  - `src/blog/posts/68_light_roast_vs_dark_roast_coffee.mdx`
  - `./src/blog/posts/286_bialetti_moka_pot.mdx`
  - `src/blog/posts/243_coffee_gifts.mdx`
  - `src/blog/posts/108_how_to_fix_espresso_channeling_step_by_step_puck_prep_guide.mdx`
  - `src/blog/posts/242_pour_over_coffee_cafes_auckland.mdx`
- **Fix Action**: Run the `fix_content_article` task family with the `title_deduplication` skill parameter to rewrite titles, removing redundant tokens while preserving click-through appeal.

### Thin Content on Not-Indexed Pages

- **Problem**: 12 pages are not indexed and have zero word count, indicating missing or empty body content.
- **Affected Pages**:
  - `https://brewedlate.com/blog/234-what-is-coffee-blooming-how-to-bloom-pour-over-coffee`
  - `https://brewedlate.com/blog/45-best-affordable-coffee-brewing-setup`
  - `https://brewedlate.com/blog/265-hub-coffee-beans`
  - `https://brewedlate.com/blog/how-to-make-coffee-without-a-coffee-maker`
  - `https://brewedlate.com/blog/monthly-coffee-subscription-australia`
  - `https://brewedlate.com/blog/cheapest-coffee-beans-australia-2026`
  - `https://brewedlate.com/blog/44-coffee-freshness-by-origin`
  - `https://brewedlate.com/blog/australia-coffee-deals-guide`
  - `https://brewedlate.com/blog/62-best-cold-brew-coffee-maker-australia`
  - `https://brewedlate.com/blog/63-best-stovetop-coffee-maker-moka-pot`
  - `https://brewedlate.com/blog/41-best-coffee-beans-for-cold-brew`
  - `https://brewedlate.com/blog/49-trending-specialty-coffee-2025`
- **Fix Action**: Trigger `write_article` or `fix_content_article` tasks for each slug to regenerate or expand body content. Ensure `count_words()` is validated first (see P0).

### Missing Internal Links

- **Problem**: 12 not-indexed pages have zero incoming internal links, orphaning them from crawl paths.
- **Affected Pages**: Same 12 pages listed under Thin Content above.
- **Fix Action**: Run `cluster_and_link` or `add_internal_links` to inject contextual links from high-authority indexed pages (e.g., hub pages, category overviews) to these orphaned articles.

## P2 — Structural Changes (Architecture)

### Temporal URLs Requiring Annual Migration

- **Problem**: 6 articles use year-specific slugs that will become stale and require annual 301 redirects.
- **Affected Pages**:
  - `best-decaf-coffee-beans-australia` → `src/blog/posts/60_best_decaf_coffee_beans_australia.mdx`
  - `can-you-reuse-coffee-grounds-what-you-need-to-know` → `src/blog/posts/235_can_you_reuse_coffee_grounds_what_you_need_to_know.mdx`
  - `how-decaf-coffee-is-made` → `src/blog/posts/61_how_decaf_coffee_is_made.mdx`
  - `coffee-deals-this-week` → `src/blog/posts/250_coffee_deals_this_week.mdx`
  - `best-coffee-deals-september-2025` → `src/blog/posts/best_coffee_deals_september_2025.mdx`
  - `best-reusable-coffee-pods-australia-2026-complete-buying-guide` → `src/blog/posts/109_best_reusable_coffee_pods_australia_2026_complete_buying_guide.mdx`
- **Migration Plan**:
  1. Adopt evergreen slugs for time-sensitive content (e.g., `best-coffee-deals` instead of `best-coffee-deals-september-2025`).
  2. Implement a redirect rule in the hosting platform (Cloudflare Pages, Vercel, or nginx) mapping old year-specific slugs to the evergreen equivalents.
  3. Update internal links in `articles.json` and MDX files to point to evergreen URLs.
  4. For recurring content (e.g., "this week" deals), consider a single hub page with dynamic or frequently updated sections rather than annual slug churn.

## Issue Matrix

| Issue | Priority | Type | Count | Status |
|---|---|---|---|---|
| Zero word count on not-indexed pages | P0 | Code | 12 | Open |
| Page bloat from excessive tables | P0 | Code | 10 | Open |
| Title token duplication | P1 | Content | 21 | Open |
| Thin content on not-indexed pages | P1 | Content | 12 | Open |
| Missing internal links (orphaned pages) | P1 | Content | 12 | Open |
| Temporal URLs requiring annual migration | P2 | Structural | 6 | Open |
