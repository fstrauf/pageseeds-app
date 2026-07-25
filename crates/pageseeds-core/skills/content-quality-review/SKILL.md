# Content Quality Review

<!-- skill-version: 1 -->

Review a newly written MDX article against the site quality bar: it must be
useful, visual, SEO-clean, and fit the site's cluster structure. You are the
gate between article generation and linking/publishing — score honestly; a
borderline article that fails here is cheaper to fix now than after indexing.

## Input

You receive a JSON context object with:

- `file`, `title`, `description`, `h1`, `target_keyword`, `slug`, `canonical`,
  `image` — frontmatter fields (may be empty strings when missing).
- `word_count`, `internal_link_count` — computed metrics.
- `body_excerpt` — the first ~4,000 characters of the article body. Judge from
  the excerpt; do not assume content beyond it exists.

## Scoring Criteria (1–100 each)

1. **usefulness_score** — Does the article answer a specific question with
   original examples, data, or first-hand insight? Would a reader learn
   something not found in the top 3 Google results? Generic restated
   definitions and padded listicles score low.
2. **image_score** — Does the article include at least one relevant, genuinely
   useful image, diagram, chart, or screenshot (a real `image` frontmatter
   field or body image, not a decorative stock placeholder)?
3. **seo_score** — Clean title (<60 chars), meta description present, H1
   aligned with the target keyword, clean slug, canonical URL, and internal
   links (`internal_link_count` > 0). Missing critical fields (title,
   description, target keyword) must drag this score below 60.
4. **cluster_fit_score** — Does the article clearly map to a pillar/cluster
   and reference related content on the site via its internal links? An orphan
   article that links nowhere scores low.

## Output Contract

Return a `ContentQualityReview` JSON object:

- The four scores above, each 1–100.
- `overall_pass`: `true` only if **all four scores are ≥ 60** and no critical
  SEO field (title, description, target keyword) is missing.
- `checks`: an array with one entry per failed or borderline criterion, using
  ids `usefulness`, `image`, `seo_basics`, `cluster_fit`. Each entry states
  what is wrong and how to fix it concretely.
- `signal_score`: your single 1–100 overall quality signal for downstream
  topic-health aggregation.
