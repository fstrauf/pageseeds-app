# SEO Baseline Sweep Runbook

A manual, project-by-project review that applies the "head terms + tangential content + structure + quality" SEO strategy before any new content is produced.

Use this runbook to establish a baseline for a project and identify quick wins. Several sections of this runbook are now partially automated by the workflow implemented in [GitHub issue #7](https://github.com/fstrauf/pageseeds-app/issues/7):

- **Section 4 (Content quality audit)** → automated by the `review_article_quality` task that runs after every `write_article`.
- **Section 5 (Topic health assessment)** → automated by the topic-health reducer that runs after every `content_review` / `content_audit` and marks `research_shortlist` entries as `promising`, `unproven`, or `depleted`.

Run this manual sweep once per project to catch strategic gaps the automated workflow cannot see; then let the ongoing workflow keep the quality gate and topic pool up to date.

---

## 1. Pillar audit

Goal: confirm the project has 3–5 clear head terms (pillars) and that the homepage/key pages own them.

### Steps
1. Open the project's homepage / landing page copy.
2. Open `.github/automation/manifest.json` (or equivalent project brief).
3. Read the top-level `README.md` / `index.md` / `page.mdx` if they exist.
4. Extract the apparent pillars:
   - What is the product/service?
   - What are the 3–5 high-level concepts a stranger should associate with the site?
5. Score each pillar:
   - Is it named explicitly on the homepage?
   - Is it in the H1 / title / meta description?
   - Is there a dedicated pillar page or section?

### Output
- List of proposed or confirmed pillars.
- Any pillar that is missing, buried, or unclear.

---

## 2. Content inventory & cluster mapping

Goal: map every published article to a pillar and identify orphaned or off-strategy content.

### Steps
1. List all published MDX files in the content directory.
2. For each article, read:
   - Title
   - Target keyword / H1
   - Internal links (where does it link to?)
   - Rough topic / sub-theme
3. Map each article to the closest pillar.
4. Flag articles that don't map cleanly to any pillar.

### Output
- Table: article → pillar → target keyword → status (mapped / orphaned / off-strategy).
- Clusters grouped by pillar.

---

## 3. Site structure / internal linking audit

Goal: verify that related content is linked together and that the structure isn't flat/random.

### Steps
1. Build a simple link graph: for each article, list outbound internal links.
2. Identify:
   - Orphaned pages (0 inbound internal links).
   - Pages with only cross-category links (no related-content links).
   - Dense clusters (good) vs. flat, unconnected pages (bad).
3. Check navigation / category pages if they exist.

### Output
- Number of orphaned pages.
- List of cluster gaps (related articles that should link to each other but don't).
- Quick-win link suggestions.

---

## 4. Content quality audit

Goal: flag articles that fail the "useful + visual + SEO basics" bar, regardless of traffic.

### Steps
For each published article (sample the top 10–20 by traffic + 10–20 random long-tail pieces), check:

1. **Usefulness / originality**
   - Does it answer a specific question?
   - Does it include an original example, data, take, or first-hand insight?
   - Would a reader learn something they couldn't get from the top 3 Google results?
2. **Visual**
   - Does it have at least one relevant image, diagram, chart, or screenshot?
   - Is the image genuinely useful, or generic stock?
3. **SEO basics**
   - Title tag present and under ~60 chars.
   - Meta description present.
   - H1 present and aligned with target keyword.
   - URL slug clean and keyword-aligned.
   - Internal links present.

### Output
- List of articles failing 2+ criteria.
- Common failure patterns across the site.

---

## 5. Topic health assessment

Goal: identify which keyword clusters are worth doubling down on and which are depleted.

### Steps
1. Pull GSC / analytics data if available:
   - impressions, clicks, CTR, average position per page/query.
   - approximate dwell time / engagement if available (Clarity/PostHog).
2. For each cluster/pillar, aggregate signals:
   - **Promising:** clicks + impressions growing, good engagement, ranking 5–20.
   - **Steady:** consistent but flat traffic, healthy engagement.
   - **Depleted:** low/no clicks, declining impressions, poor engagement, or ranking beyond 30 with no trend.
3. Cross-reference with content quality audit: a depleted cluster with weak content is a content problem; a depleted cluster with strong content is a market/saturated problem.

### Output
- Promising clusters to produce more content for.
- Depleted clusters to deprioritize or improve existing content in.
- Underexplored tangential topics adjacent to promising clusters.

---

## 6. Synthesis & action list

Goal: turn the audit into a short, prioritized plan.

### Output
1. **Pillar fixes** — homepage copy, navigation, or pillar pages to create/update.
2. **Structure fixes** — internal links to add, orphans to rescue, clusters to form.
3. **Content fixes** — specific articles to improve or remove.
4. **New content opportunities** — tangential topics under promising clusters.
5. **Workflow learnings** — what patterns should be tuned in the automated quality gate or topic-health thresholds (e.g., common quality failures, depleted thresholds).

---

## How to use this runbook for a new project

1. Replace the project name and content directory in the checklist below.
2. Work through sections 1–5, taking notes in a scratch doc or issue comment.
3. Produce the synthesis in section 6.
4. If the findings repeat across projects, they become requirements for the automated workflow in issue #7.

### Project checklist

- [ ] Project name:
- [ ] Content directory:
- [ ] Homepage / landing URL:
- [ ] Pillar audit complete
- [ ] Content inventory & cluster map complete
- [ ] Internal linking audit complete
- [ ] Content quality audit sample complete (automated `review_article_quality` tasks also reviewed)
- [ ] Topic health assessment complete (automated `research_shortlist.health_status` also reviewed)
- [ ] Synthesis & action list written
