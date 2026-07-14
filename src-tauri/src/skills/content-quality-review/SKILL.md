# Content Quality Review

Review a freshly written MDX article against the four-bar quality gate from the SEO baseline sweep runbook. Return a structured `ContentQualityReview` JSON object.

## Scoring rubric

Score each dimension 0-100. Be strict but fair.

### 1. usefulness_score
- 80-100: Answers a specific question with an original example, data point, first-hand insight, or concrete workflow.
- 60-79: Useful but generic; could be found in competing results with minor effort.
- 40-59: Thin or mostly boilerplate; no clear takeaway.
- 0-39: Does not answer the implied query or is largely filler.

### 2. image_score
- 80-100: Contains a relevant diagram, chart, screenshot, or custom visual that genuinely aids understanding.
- 60-79: Has a relevant image but it is decorative or stock.
- 40-59: Image is present but weakly related.
- 0-39: No image, or a misleading/generic placeholder.

### 3. seo_score
Check frontmatter and structure:
- Title tag present and under ~60 characters.
- Meta description present.
- H1 present and aligned with `target_keyword`.
- Clean `slug` aligned with keyword.
- `canonical` URL present.
- At least one internal link using `[anchor](/blog/slug)` format.

80-100: All critical SEO fields present and well-formed.
60-79: One minor issue (e.g., title slightly long, only one weak internal link).
40-59: Two or more missing/weak fields.
0-39: Major SEO fields missing.

### 4. cluster_fit_score
- 80-100: Clear pillar/cluster alignment; references related content on the site naturally.
- 60-79: Fits a cluster but lacks explicit related-content links.
- 40-59: Ambiguous cluster fit.
- 0-39: Off-strategy or isolated.

## overall_pass

Set `overall_pass` to `true` only if **all four scores are >= 60** and no critical SEO field (title, H1, slug, canonical) is missing.

## checks array

For every dimension that scores < 60, include a check:

```json
{
  "id": "usefulness" | "image" | "seo_basics" | "cluster_fit",
  "label": "Human-readable label",
  "pass": false,
  "detail": "Specific issue and what would fix it"
}
```

## Output format

Return only a valid JSON object matching the `ContentQualityReview` schema:

```json
{
  "overall_pass": true,
  "usefulness_score": 75,
  "image_score": 80,
  "seo_score": 85,
  "cluster_fit_score": 70,
  "checks": []
}
```

Do not include Markdown code fences or commentary outside the JSON.
