# Content Review Revisit Policy

## Goal

Keep content review moving through pages that have never been reviewed, while allowing previously reviewed pages to re-enter the queue when they are stale or clearly regressing.

## Selection Policy

`content_review_recommend` should select articles in two passes:

1. Backlog-first: include eligible articles that have never been reviewed.
2. Revisit-backfill: if fewer than `max_items` backlog articles are available, fill remaining slots with previously reviewed articles that are eligible for a revisit.

### Exclusions

- Exclude `draft` articles.
- Exclude articles already marked `in_review`.
- Exclude articles without a content file.

### Backlog Articles

Backlog articles are articles with no `last_reviewed_at` and no `review_status == reviewed`.

These stay eligible even if their computed score is low, because the product goal is to drain the unreviewed backlog over time.

### Revisit Eligibility

Reviewed articles can re-enter only when one of these is true:

- Stale revisit: `last_reviewed_at` is at least 45 days old.
- Regression revisit: `last_reviewed_at` is at least 14 days old and the article currently shows a strong content-review opportunity.

Regression signals use the same deterministic inputs already available to `select_priority_articles`:

- CTR opportunity: average position 5-20, impressions > 200, CTR < 3%
- Poor health audit result
- Low audit score / multiple failed checks

## Ordering

- Score all eligible backlog articles with the existing formula and take the highest-scoring backlog items first.
- Only after backlog slots are exhausted should revisit-eligible reviewed articles be considered.
- Revisit candidates are still ordered by score descending.

## UI

Surface review progress directly in the article table:

- Add a compact review column showing `unreviewed`, `in review`, or `reviewed`.
- Show `last_reviewed_at` for reviewed articles.
- Show `review_count` when non-zero.
- Add a small header summary so users can see backlog totals without opening task details.

## Validation

- Rust unit tests for:
  - backlog-first behavior when reviewed articles score higher
  - stale reviewed articles becoming eligible again
  - recently reviewed articles staying excluded despite strong signals
- TypeScript check for the article-table UI update