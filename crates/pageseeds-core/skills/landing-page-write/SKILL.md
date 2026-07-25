# Landing Page Writer

<!-- skill-version: 1 -->

Write a conversion-focused MDX landing page that ranks for a commercial or
transactional keyword and turns that traffic into signups, trials, or demos.
A landing page is not a blog post: every section must move the reader toward
the call to action. Generic feature lists that duplicate the SERP will not
win — differentiation is mandatory.

## Input

- **Target keyword** (and KD / Volume / CPC when available) in the task description.
- **Structured brief** (`content_brief` task artifact, JSON): search intent,
  `page_type` (comparison / use_case / feature / category), proposed title,
  why the keyword was selected, research themes, and
  `internal_link_candidates` — a list of valid internal-link targets
  (`slug` + `title`) from this site. Use it when present.
- **Project context**: repo path, site URL.

## Core Principle: Match Intent, Earn the Conversion

A searcher for a commercial keyword is evaluating options, not learning a
concept. Before writing, reason about what they already see in the SERP and
what would make THIS page the one they act on:

- **Proof over promises** — concrete numbers, screenshots described in words,
  real workflows, real outcomes. "We processed 40k trades last month" beats
  "powerful and easy to use."
- **Specificity over adjectives** — name the exact feature, plan, limit, or
  integration that answers the query. Cut "best-in-class," "cutting-edge,"
  "seamless."
- **Honest positioning** — say who the product is NOT for when that helps the
  right reader self-select. Balanced pages convert and rank better than pure
  promotion.

## Structure by Page Type

Follow the `page_type` from the brief (default: category):

- **comparison** — Hero addressing the comparison query → quick side-by-side
  comparison table → detailed pros/cons per option → "Choose X if…" guidance → CTA.
- **use_case** — Hero: the problem + how the product solves it → step-by-step
  walkthrough of the workflow → concrete outcome/benefit evidence → social
  proof → CTA.
- **feature** — Hero: feature name + one-line benefit → problem/solution →
  how it works → integrations/compatibility → CTA.
- **category** — Hero: category value prop → 3-5 key capabilities → who it's
  for → social proof → CTA.

## CTA Placement

- One primary CTA (trial, signup, demo — pick what fits the site) visible in
  the hero and repeated after the final section.
- CTAs are action phrases tied to the reader's goal ("Start tracking your
  trades"), not generic buttons ("Submit", "Learn more").
- Do not invent URLs for CTAs — link to the site's home page or a slug from
  `internal_link_candidates`, or leave the CTA as an unlinked directive.

## E-E-A-T (Experience, Expertise, Authoritativeness, Trust)

For finance, health, legal, and other YMYL topics this is non-negotiable:

- State credentials or basis of experience where natural.
- Cite sources for any specific claim, statistic, regulation, or number.
- Distinguish opinion from fact; distinguish historical results from guarantees.
- Include risk, caveat, or "when this doesn't apply" context.

## SEO Requirements

- **One H1** containing the target keyword (natural phrasing, not keyword stuffing).
- **Lead paragraph** answers the searcher's core question within the first 100
  words — make it quotable and complete.
- **Comparison/decision content** as real markdown tables, not prose lists.
- **FAQ section** (3-5 questions) addressing the natural follow-ups — feeds FAQ
  schema and People-Also-Ask capture.
- **Internal links** to genuinely related articles on this site with descriptive
  anchor text, chosen from the brief's `internal_link_candidates`. Only link
  where it helps the reader.
- Target **900-1,400 words**. Landing pages are tighter than blog posts —
  every sentence either ranks or converts.

## Output

A complete MDX document: YAML frontmatter (title, date, description, target_keyword)
followed by the page body. Return ONLY the MDX content — no markdown wrappers,
no commentary, no explanations outside the document.

## Constraints

- Write as `.mdx`. Preserve valid frontmatter and MDX syntax.
- Internal links use `[anchor text](/blog/slug)` format.
- **Never invent internal-link slugs.** Link only to slugs listed in the brief's
  `internal_link_candidates`. If none fit, write the page without internal links
  rather than guessing — invented slugs fail link verification.
- The frontmatter `date` must be exactly the publish date provided in the task —
  do not invent one.
- Do not fabricate statistics, testimonials, customer names, or results. If you
  lack real data, omit the claim rather than inventing it — fake social proof is
  worse than none.
