---
name: competitor-review
description: >-
  Operator runbook: research one competitor/tool into competitors.yaml matrix
  facts, then Path B write (new) or fix (existing slug) → draft MDX via submit.
  Also used to refresh one stale matrix row. Report only
  through draft — human publish-content. Not a new task type; not auto-spawn.
when-to-use: >-
  Triggers on "/competitor-review", "competitor review", "tool review matrix",
  "research competitor for alternatives page", "fill competitors.yaml",
  "draft competitor comparison MDX".
argument-hint: "[project-name-or-id] [seed-id or primary_keyword]"
user-invocable: true
metadata:
  short-description: "Seed → matrix facts → Path B competitor review draft"
---

# Competitor Review — Operator Bible

> **Purpose:** Turn **one** competitor seed into matrix facts + a **draft** MDX
> review (or refresh an existing review). Matrix SoT is
> `.github/automation/competitors.yaml`. Path B **write** when no page yet;
> Path B **fix** when a live slug exists. No new task type; no auto-publish.

**Canonical path:** `.agents/skills/competitor-review/SKILL.md`

Edit only the canonical copy; agent-specific skill directories should link to it or copy it.

## Invocation

```
/competitor-review
/competitor-review <project>
/competitor-review <project> surfer-seo
/competitor-review <project> "surfer seo alternative for small business"
```

| Layer | Role |
|-------|------|
| **Matrix SoT** | `{repo}/.github/automation/competitors.yaml` |
| **Path B new** | `write-context -K "<seed primary_keyword>"` → session MDX → `write-submit` |
| **Path B refresh** | Existing live slug + facts changed → `fix-context` / `fix-submit` `-k content` (or `refresh` only if GSC decay also applies) |
| **Skill body** | Embedded `competitor-review-write` (package includes row + CTA) on write path |
| **Policy** | This skill + the stale / as_of policy below |
| **Not this skill** | Catalog **hub** pillar (use `/competitor-catalog-hub` after ≥3 spokes), bulk multi-CLI research, inventing a refresh queue |

---

## Hard rails

| # | Rule |
|---|------|
| 1 | **One competitor per run** — one seed → one matrix row update → optional one draft/fix. |
| 2 | **No publish** without human `publish-content -S <slug>`. Stop at draft. |
| 3 | **No invented pricing** or limits — empty fact stays empty; cite `source_url` + `as_of`. |
| 4 | **No auto-spawn** of tasks — run this runbook on request (≤1–2 stale rows at a time); never invent a competitor-refresh task type or queue. |
| 5 | Honor strategy `do_not_expand` / LEGACY (write-context will hard-block). |
| 6 | Prefer public pricing / docs pages over third-party recaps when possible. |
| 7 | Do not create new task types; writing is Path B only. |
| 8 | **Never** use `write-context` to overwrite an existing article path — use Path B **fix** when a live review slug already exists. |

---

## Soft path

```text
resolve project path / id
  → ensure/read competitors.yaml (seeds + product_cta)
  → pick ONE seed (arg, stale-row shortlist, or ask)
  → optional: pageseeds-cli page-fetch -U "<seed or pricing HTTPS URL>"
     for a bounded main-text excerpt (report only; no matrix write)
  → research public sources (page-fetch excerpt, session web tools, or pasted URLs)
  → update competitors[] row: facts with value + source_url + as_of;
     leave unknown keys omitted; update row as_of only when sourced this run
  → branch:
       if live review slug exists AND facts changed:
         fix-context -S <slug> -k content → edit MDX → fix-submit
         (use -k refresh only when GSC decay also applies)
       else if no page yet:
         write-context -K "<seed primary_keyword>" → craft MDX → write-submit
       else:
         matrix-only → report "no MDX change"
  → stop at draft (or matrix-only); human publish-content if MDX shipped
  → worktree ship of this-run tracked files (matrix + MDX); Path B auto-begins
  → report: matrix path, slug if any, missing facts, publish reminder, PR url
```

### Refresh mode (stale row)

Same one-competitor runbook with **stale policy selection**:

| Input | Pick |
|-------|------|
| Explicit `seed-id` / `primary_keyword` arg | That seed |
| Owner-picked stale matrix row | That row (≤1–2 per run) |
| No arg | Ask, or pick oldest `as_of` / missing critical facts first |

Stale definition and caps:
(default 90d row/`pricing`/`free_tier` hygiene; never invent; soft prior only).
Still **one competitor per run**; no publish without human.

### Optional page-fetch assist

When the seed has an official homepage / pricing / docs URL, prefer a
deterministic fetch before inventing or scraping ad hoc:

```bash
pageseeds-cli page-fetch -U "https://example.com/pricing"
```

- **Report only** — stdout JSON (`text_excerpt`, `title`, `fetched_at`, …).
  Does **not** mutate `competitors.yaml` or auto-fill prices.
- You still write matrix facts yourself: each fact needs `value` + `source_url`
  + `as_of` (use the page URL and `fetched_at` date).
- Fallback: session web tools or operator-pasted page text when fetch fails
  (non-HTML, paywall, bot block, timeout).

### Matrix edit rules

- File: `.github/automation/competitors.yaml` (create via **Bootstrap seeds** below if missing).
- Match `competitors[].id` to `seeds[].id`.
- Each fact: `{ value, source_url, as_of }` — omit the whole key if unknown.
- Common fact keys: `pricing`, `free_tier`, `platform`, `writes_to_repo`, `api`, `best_for`.
- Row-level `as_of` + `sources[]` for provenance.

### Path B — new page (no live slug yet)

```bash
pageseeds-cli write-context -i <project-id> -p <project-path> \
  -K "<exact seed primary_keyword>"
# Session writes MDX to package.target_file using skill_content
pageseeds-cli write-submit -i <project-id> -p <project-path> \
  --file <target_file> -K "<same keyword>"
```

Authorization: seed `primary_keyword` (normalized) → `auth_source: competitor_catalog`
without `-I`. Skill name must be `competitor-review-write`.

`write-submit` returns a dry-run `source_gate` verdict (#486). Report it. Name a declared source only if its payload is used in an artefact; never add `sources` to `project.yaml` to clear it.

### Path B — existing live slug (refresh)

When the review article already exists and matrix fact values changed:

```bash
pageseeds-cli fix-context -i <project-id> -p <project-path> \
  -S <slug> -k content
# session edits full MDX for fact/CTA truth
pageseeds-cli fix-submit -i <project-id> -p <project-path> \
  -S <slug> -k content [--file mdx]
```

Use `-k refresh` **only** when GSC decay also justifies a content refresh — do
not overload refresh for pure fact repair. **Never** call `write-context` to
overwrite an existing article path.

### Final user message

```markdown
## Competitor review — {project} / {seed id or name}

**TL;DR:** Matrix updated; {draft MDX at slug | Path B fix submitted | matrix-only}; **not published** (unless human already published).

**Mode:** new write | refresh fix | matrix-only
**Matrix:** `.github/automation/competitors.yaml` (row as_of: {old} → {new})
**Keyword:** `{primary_keyword}`
**Draft / slug:** `{slug}` / `{path}` (or n/a if matrix-only)
**Missing facts:** {list empty/omitted keys worth a second pass}
**Publish:** human only — `pageseeds-cli publish-content -i … -S {slug}`

### Sources used
- {url} ({as_of})
```

---

## Relationship

| Surface | Owns |
|---------|------|
| `/competitor-review` | One-competitor matrix research + Path B write **or** fix |
| `/competitor-catalog-hub` | Hub pillar after ≥3 seed-matched review spokes |
| `competitors.yaml` | Seeds + matrix facts + optional `hub:` (customer repo); **inventory for stale refresh** via `as_of` |
| `page-fetch` | Optional HTTPS excerpt for fact research (no matrix write) |
| `write-context` / `write-submit` | New review page only (no existing path) |
| `fix-context` / `fix-submit` | Existing live review slug when facts changed |
| `/keyword-explore` | Demand/SERP scout — optional upstream of seed choice |

---

## Design

```text
competitors.yaml seeds
  → session researches → fill matrix row (sourced facts only)
  → if no live slug:
       write-context -K seed primary_keyword
         → content_brief + competitor_row + product_cta + competitor-review-write
       → session MDX → write-submit → human publish-content
  → if live slug + facts changed:
       fix-context -k content → session edit → fix-submit → human publish
  → else matrix-only
```

## Bootstrap seeds

For a project with no usable `competitors.yaml`. Seeds only; no matrix rows or facts.

1. Run `pageseeds-cli competitor-catalog-status -p <project>`. If `gap_ready` is true, stop.
2. Gather candidates from: the product itself (README / landing copy: who it replaces);
   ranking domains in the latest `keyword_explore_latest.json` deep results for the
   project's Primary keywords (run `keyword-explore --serp-top 8` first if no report);
   and the operator.
3. Exclude marketplaces, forums, video sites and encyclopedias (the report's page types
   mark them), the project's own domain, and anything matching `do_not_expand`.
4. Propose 5-10 seeds to the operator with evidence for each (which keywords it ranks
   for, at what position). Write only what the operator confirms.
5. Write `seeds[]` from this template:

   ```yaml
   schema_version: 1
   seeds:
     - id: example-co            # kebab-case, unique
       name: Example Co
       url: https://example.com  # homepage, HTTPS
       primary_keyword: example co alternative   # default "<name> alternative"
   ```

6. Re-run `competitor-catalog-status`; done when `valid` and `gap_ready` are true.
   Changes go through a worktree PR like every other skill write.
