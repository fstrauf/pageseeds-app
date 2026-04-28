# CTR FAQ Preservation Policy Spec

Status: Proposed  
Created: 2026-04-29  
Scope: CTR article fixes, FAQ generation, rendered FAQ schema detection, inline JSON-LD cleanup, and audit issue classification.

## Summary

The current CTR fix workflow is not safe enough to keep running at scale because its content-generation policy is too aggressive around FAQ fixes.

The workflow can now avoid raw JSON-LD injection and write FAQ data into frontmatter, which is the right architectural direction. But the latest run showed a new failure mode: when an article already has rich, useful FAQ content, the agent may replace it with shorter, generic FAQ answers. This is a quality regression for readers and SEO.

The root fix belongs in PageSeeds. PageSeeds must preserve existing high-quality FAQ content, only generate FAQ content when structured FAQ is genuinely missing or unusable, and separate "article source FAQ is missing" from "target repo fails to render FAQ schema."

## Problem

CTR checks currently treat FAQ health too broadly. A page can be flagged as having an FAQ/schema problem for several different reasons:

- The article has no FAQ content at all.
- The article has visible FAQ content but no structured `faq` frontmatter.
- The article has rich `faq` frontmatter, but the rendered page does not emit FAQPage JSON-LD.
- The article has both `faq` frontmatter and an old inline JSON-LD script, creating duplicate schema sources.
- The target repo parser drops nested frontmatter fields.
- The target repo renderer does not pass parsed FAQ data into its SEO/head component.

These are not the same problem. Treating them as one `missing_faq_schema` issue causes bad fixes.

The latest workflow run demonstrated this clearly:

- Raw JSON-LD insertion was mostly stopped.
- FAQ data was written to frontmatter, which is good.
- But existing rich FAQ answers were replaced with generic answers.
- Some legacy inline JSON-LD blocks still remained from earlier runs.
- At least one modified article had both frontmatter FAQ and old inline FAQPage JSON-LD.

PageSeeds needs a stricter policy before allowing automated CTR FAQ fixes to continue.

## Goals

- Preserve existing high-quality FAQ content by default.
- Only generate new FAQ content when source FAQ is missing, malformed, empty, or clearly unusable.
- Separate source-level FAQ issues from rendered-schema issues.
- Stop using article-level FAQ rewrites to fix target-repo rendering problems.
- Treat inline JSON-LD as cleanup debt, not as the canonical future format.
- Avoid duplicate FAQ sources in a single article.
- Make FAQ fix decisions deterministic wherever possible.
- Require agentic FAQ generation to be grounded in existing article content and GSC/query context.

## Non-Goals

- Rewriting every historical FAQ in the content library.
- Guaranteeing Google will display FAQ rich results.
- Making PageSeeds responsible for target repo framework rendering.
- Building a full target-repo integration scanner in this feature.
- Automatically deleting all old inline JSON-LD blocks without validation.

## Core Principle

FAQ fixes must be conservative.

When PageSeeds encounters existing FAQ content, it should prefer preservation, normalization, or migration over regeneration.

The default rule is:

```text
Existing useful FAQ beats newly generated FAQ.
```

## Issue Taxonomy

Replace the broad `missing_faq_schema` classification with more precise issue types.

### Source-Level Issues

These are article content problems and may be fixed by `fix_ctr_article`.

| Issue | Meaning | Article Fix Allowed? |
|---|---|---|
| `missing_source_faq` | No frontmatter FAQ, no visible FAQ section, no inline FAQPage schema. | Yes |
| `empty_source_faq` | `faq` exists but has no valid question/answer pairs. | Yes |
| `malformed_source_faq` | FAQ exists but cannot be parsed safely. | Yes, with review if risky |
| `thin_source_faq` | FAQ exists but is obviously too thin, generic, or unrelated. | Review first |
| `unstructured_visible_faq` | Body has visible FAQ section but no structured frontmatter FAQ. | Prefer migration, not generation |

### Render-Level Issues

These are target repo rendering/template problems and should not trigger article FAQ regeneration.

| Issue | Meaning | Article Fix Allowed? |
|---|---|---|
| `missing_rendered_faq_schema` | Source FAQ exists, rendered page has no FAQPage JSON-LD. | No |
| `dropped_faq_frontmatter` | Source FAQ exists, target parser appears not to expose it. | No |
| `schema_renderer_missing` | Target repo has no central FAQ schema renderer. | No |
| `duplicate_rendered_faq_schema` | Rendered output contains multiple FAQPage blocks. | No, route to cleanup/render task |

### Cleanup Issues

These are migration debt from previous runs.

| Issue | Meaning | Fix Path |
|---|---|---|
| `inline_json_ld_faq_debt` | MDX body contains FAQPage JSON-LD script. | Cleanup task |
| `duplicate_source_faq_sources` | Frontmatter FAQ and inline FAQPage both exist. | Preserve frontmatter, remove/ignore inline script after validation |
| `body_metadata_block_debt` | Metadata-like YAML appears in article body after frontmatter closes. | Cleanup task or review |

## Desired Behavior

### Case 1: Article Has Good Frontmatter FAQ

If an article already has valid `faq` frontmatter with useful answers:

- Do not regenerate FAQ.
- Do not replace FAQ questions.
- Do not shorten answers.
- If rendered FAQPage is missing, classify as `missing_rendered_faq_schema`.
- Create a repo-rendering task or warning, not an article FAQ patch.

### Case 2: Article Has Visible FAQ But No Frontmatter FAQ

If the body contains a `Frequently Asked Questions` section but no frontmatter `faq`:

- Extract existing visible FAQ into structured frontmatter if possible.
- Preserve answer detail.
- Do not ask the agent to invent replacement FAQ unless extraction fails.
- If extraction is uncertain, mark for review.

### Case 3: Article Has Inline JSON-LD FAQ But No Frontmatter FAQ

If an old inline FAQPage script exists and frontmatter FAQ is missing:

- Parse the inline JSON-LD.
- Migrate its questions/answers into frontmatter `faq`.
- Remove the inline script only after successful migration and validation.
- If parsing fails, mark for review.

### Case 4: Article Has Both Frontmatter FAQ And Inline JSON-LD

If both exist:

- Treat frontmatter `faq` as canonical.
- Compare inline JSON-LD to frontmatter.
- If equivalent or less detailed, remove inline JSON-LD as cleanup.
- If inline JSON-LD contains unique useful content, merge carefully or mark for review.
- Do not regenerate FAQ from scratch.

### Case 5: Article Has No FAQ Content

If no FAQ source exists:

- Generate 3-5 FAQ items from the article content and query context.
- Require specific, useful answers grounded in the page.
- Write the result to frontmatter `faq`.
- Do not insert raw JSON-LD.

### Case 6: Target Repo Does Not Render FAQ Schema

If source FAQ exists but rendered output has no FAQPage schema:

- Do not alter article FAQ content.
- Create or recommend a target-repo renderer/schema task.
- The task should explain that the repo must parse `faq` frontmatter and render FAQPage JSON-LD centrally.

## FAQ Quality Policy

FAQ answers should not be generic filler.

When generating or judging FAQ content, PageSeeds should prefer answers that include:

- Specific facts from the article.
- Prices, ranges, dates, quantities, countries, tools, methods, or constraints when present.
- Clear caveats and tradeoffs.
- Direct answers in the first sentence.
- Local context when the article is region-specific.
- Enough detail to be useful without reading the full article.

Avoid answers that only restate obvious category-level information.

### Minimum Quality Rules

Generated FAQ should meet these defaults:

- 3-5 questions.
- Each answer should usually be 35-120 words.
- At least two answers should include specific article-derived details if available.
- Answers must not contradict article body content.
- Questions should match real search intent, not just generic headings.
- Do not remove richer existing FAQ unless it fails validation or is unrelated.

### FAQ Preservation Heuristic

Before generating FAQ, PageSeeds should score existing FAQ content.

Suggested deterministic signals:

- Number of valid Q/A pairs.
- Average answer word count.
- Presence of numbers, prices, dates, named entities, or article-specific terms.
- Overlap with target keyword or GSC queries.
- Whether answers are unique rather than repeated boilerplate.
- Whether questions are meaningful search-style questions.

If existing FAQ passes a conservative threshold, preserve it.

If score is borderline, route to review rather than overwrite.

## Agent Prompt Requirements

The CTR FAQ generation prompt must change from "create FAQ" to "preserve, migrate, or create only if missing."

Agent input should include:

- Current frontmatter FAQ if present.
- Visible FAQ section if present.
- Inline JSON-LD FAQ if present.
- Article excerpt and headings.
- Target keyword.
- Query-level GSC context when available.
- Existing FAQ quality assessment from deterministic code.
- The specific issue type being fixed.

Agent output must choose one action:

```json
{
  "action": "preserve_existing" | "migrate_existing" | "generate_missing" | "merge_existing" | "needs_review",
  "reason": "Short explanation",
  "faq": [
    { "question": "...", "answer": "..." }
  ],
  "cleanup": {
    "remove_inline_json_ld": true,
    "remove_body_metadata_block": false
  }
}
```

Rules:

- `generate_missing` is allowed only for `missing_source_faq`, `empty_source_faq`, or confirmed unusable FAQ.
- `preserve_existing` should be the default when valid FAQ already exists.
- `migrate_existing` should preserve wording as much as possible.
- `merge_existing` requires explanation of what is being merged and why.
- `needs_review` should be used when there are multiple conflicting FAQ sources.

## Deterministic Patch Requirements

Rust should apply FAQ changes deterministically.

Patch types should distinguish:

- `frontmatter_faq_set`
- `frontmatter_faq_preserve`
- `frontmatter_faq_migrate_from_body`
- `frontmatter_faq_migrate_from_json_ld`
- `inline_json_ld_remove`
- `body_metadata_block_remove`
- `rendered_schema_task_required`

The apply step should not receive vague `faq_questions` without knowing why FAQ is being changed.

Acceptance criteria:

- Existing FAQ is not overwritten unless the patch explicitly says replacement is allowed and includes the issue reason.
- Inline JSON-LD removal only happens after a canonical FAQ source is present.
- The patch result reports exactly what was preserved, migrated, generated, or removed.

## Workflow Changes

### CTR Audit

`ctr_build_context` should record separate source and rendered FAQ states:

```json
{
  "source_faq": {
    "has_frontmatter_faq": true,
    "frontmatter_faq_count": 6,
    "has_visible_faq_section": true,
    "has_inline_json_ld_faq": false,
    "quality": "rich"
  },
  "rendered_faq": {
    "checked": true,
    "has_rendered_faq_page": false,
    "rendered_faq_question_count": 0
  },
  "issues": ["missing_rendered_faq_schema"]
}
```

### Task Spawning

Spawn article FAQ fix tasks only for source-level issues.

Spawn renderer/schema tasks for render-level issues.

Spawn cleanup tasks for inline JSON-LD debt.

Do not spawn `fix_ctr_article` for `missing_rendered_faq_schema` when source FAQ is already good.

### Fix CTR Article

The per-article fix task should:

1. Analyze current FAQ sources.
2. Decide preserve/migrate/generate/review.
3. Apply deterministic patch.
4. Verify source FAQ state.
5. Optionally verify rendered state if URL/dev server is available.

### Verification

Verification should check:

- Valid frontmatter YAML.
- `faq` is parseable if present.
- No duplicate inline FAQPage remains when cleanup was requested.
- Existing FAQ was not replaced without an allowed action.
- Rendered FAQPage exists only when rendered verification is part of the task.

## Data Model Impact

If issue tracking stores CTR issue rows, add distinct issue kinds:

- `missing_source_faq`
- `empty_source_faq`
- `malformed_source_faq`
- `thin_source_faq`
- `unstructured_visible_faq`
- `missing_rendered_faq_schema`
- `inline_json_ld_faq_debt`
- `duplicate_source_faq_sources`
- `body_metadata_block_debt`

Existing `missing_faq_schema` should be treated as legacy and mapped to one of the new issue kinds during the next audit.

## Rollout Plan

### Phase 1: Stop Unsafe FAQ Replacement

- Disable FAQ replacement when valid frontmatter FAQ already exists.
- Treat existing valid FAQ as passing source FAQ health.
- Stop generating FAQ for `missing_rendered_faq_schema`.
- Keep title/meta/snippet fixes running.

### Phase 2: Add Precise FAQ Issue Classification

- Add source/render/cleanup FAQ issue types.
- Update context output and task spawning.
- Add tests for each classification.

### Phase 3: Add Migration/Cleanup Tasks

- Migrate inline JSON-LD FAQ to frontmatter when needed.
- Remove duplicate inline JSON-LD when canonical frontmatter exists.
- Detect body metadata blocks outside frontmatter.

### Phase 4: Improve Agent Prompt And Patch Contract

- Add preserve/migrate/generate action choice.
- Add deterministic quality assessment to agent input.
- Require grounded, specific FAQ output only when generation is allowed.

### Phase 5: Rendered Verification Integration

- Verify target pages emit FAQPage from frontmatter FAQ.
- Route renderer failures to target-repo tasks.
- Report source-vs-render status in UI.

## Tests

### Unit Tests

- Existing rich frontmatter FAQ is preserved.
- Existing rich FAQ is not replaced by generated FAQ.
- Missing frontmatter FAQ with no other FAQ source allows generation.
- Visible FAQ section is migrated instead of regenerated.
- Inline JSON-LD FAQ is migrated when frontmatter FAQ is missing.
- Frontmatter FAQ plus inline JSON-LD triggers cleanup, not regeneration.
- Source FAQ exists but rendered FAQPage missing classifies as render-level issue.
- Malformed FAQ routes to review or safe repair.

### Integration Tests

- A fixture article with rich FAQ and missing rendered schema creates a renderer task, not an article FAQ rewrite.
- A fixture article with no FAQ creates a frontmatter FAQ patch.
- A fixture article with old inline JSON-LD and no frontmatter FAQ migrates to frontmatter.
- A fixture article with frontmatter FAQ and old inline JSON-LD removes duplicate script after validation.
- Re-running audit after a preserved FAQ does not requeue the same FAQ issue.

### Regression Tests

- The Australia coffee buying article keeps specific FAQ answers with prices and roaster examples.
- The beginner subscription article keeps detailed onboarding FAQ answers.
- The decaf article does not end with both frontmatter FAQ and inline FAQPage JSON-LD.

## Acceptance Criteria

- Running CTR fixes cannot replace existing high-quality FAQ with generic FAQ.
- PageSeeds only generates FAQ when source FAQ is genuinely missing or unusable.
- Source FAQ issues and rendered FAQ schema issues are distinct in audit output.
- Missing rendered FAQ schema routes to a target-repo renderer/schema task or warning.
- Inline JSON-LD FAQ is no longer inserted by default.
- Existing inline JSON-LD debt is detected and cleaned up only when safe.
- The task summary clearly says whether FAQ was preserved, migrated, generated, cleaned, or sent to review.
- Repeated workflow runs do not keep reprocessing preserved FAQ content.

## Open Questions

- What exact threshold should define `thin_source_faq` versus acceptable concise FAQ?
- Should FAQ quality scoring be deterministic only, or should an agent review borderline cases?
- Should inline JSON-LD cleanup live in CTR fixes, content health, or a separate cleanup workflow?
- Should rendered FAQ verification require a configured dev server URL, or can it use production URLs by default?
- Should PageSeeds expose a manual review queue specifically for FAQ preservation conflicts?
