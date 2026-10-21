---
name: keyword-explore
description: >-
  Exploratory keyword opportunity research for one PageSeeds project via
  pageseeds-cli keyword-explore: seeds → DataForSEO discovery → shortlist →
  optional live SERP deep pass (page types, soft score, effort, win path).
  Report only — does not spawn research/write tasks. Use for niche scouting,
  “what would it take to win?”, and pre-strategy territory checks.
when-to-use: >-
  Triggers on "/keyword-explore", "keyword explore", "explore keywords",
  "underutilized keywords", "keyword opportunity scout", "what would it take
  to rank for", "SERP deep dive on seeds", "niche keyword research".
argument-hint: "[project-name-or-id] [seed phrases…]"
user-invocable: true
metadata:
  short-description: "Seed → discover → SERP deep pass → win-path report"
---

# Keyword Explore — CLI Operator Bible

> **Purpose:** Structured exploratory keyword research: find demand, score
> openness of the SERP, state what it would take to win — **before** spawning
> write tasks. Policy here; capability is `pageseeds-cli`.

**Canonical path:** `.agents/skills/keyword-explore/SKILL.md` (edit only this
copy; agent-specific skill directories should link to it or copy it).

## Invocation

```
/keyword-explore
/keyword-explore <project>
/keyword-explore <project> standing desk, ergonomic chair
```

Requires `pageseeds-cli` + DataForSEO connected + paid license.

| Layer | Role |
|-------|------|
| **CLI** | `pageseeds-cli keyword-explore` |
| **Policy** | This skill |
| **Not this skill** | Path B write (optional handoff only) |

---

## Hard rails

| # | Rule |
|---|------|
| 1 | CLI only for discovery/SERP — no ad-hoc API scripts. |
| 2 | Report-only — no write/create-task without user go-ahead. |
| 3 | Cap cost: ≤12 seeds; default SERP top 15 (`serp_guard` daily live cap applies). |
| 4 | Never invent metrics; surface CLI errors. |
| 5 | Honor strategy `do_not_expand`. |
| 6 | **One scoring story:** shortlist = `opportunity_score`; deep win = `deep.soft_serp_score` + page types + effort. There is **no** product-winnability field. |
| 7 | One project per run. |

---

## CLI

```bash
pageseeds-cli keyword-explore -i <project-id> -p <path> \
  -K "seed1,seed2,seed3" \
  [--country us] \
  [--shortlist 40] \
  [--serp-top 15] \
  [--discover-only] \
  [--modifiers calculator,generator,checker,template]

# Competitor gap (replaces -K; see below)
pageseeds-cli keyword-explore -i <project-id> -p <path> --gap \
  [--competitors a.com,b.com] [--max-kd 30] [--discover-only]
```

| Flag | Default | Notes |
|------|---------|--------|
| `-K` | strategy Primary + ACTIVE/PLANNED | Max 12 seeds |
| `--country` | `project.yaml` `search_country`, else `us` | Unsupported codes (currently us, uk/gb, ca, au, de, fr) fail before any paid call |
| `--shortlist` | `40` | Discovery shortlist size |
| `--serp-top` | `15` | Live SERP via serp_guard (max 30). `0` = discovery only |
| `--discover-only` | off | Sets serp_top=0 (cheap scan) |
| `--modifiers` | none | Comma list; keep only candidates with a whole-token (or plural) match, applied before the shortlist cut |
| `--gap` | off | Competitor gap discovery instead of seeds; rejected together with `-K` |
| `--competitors` | `competitors.yaml` seed URLs | Comma list of domains; implies `--gap`; max 5 used |
| `--max-kd` | `30` | Gap only: max keyword difficulty for competitor rows |

**Success JSON** (`kind: "keyword_explore"`), exit 0.  
Sidecar (when path known): `.github/automation/keyword_explore_*.json` + `_latest.json`.

### Free-tool scan

To find which free tools (calculators, generators, checkers, templates) are worth building, compose seeds as `-K "<niche term> calculator,<niche term> template,…"` and pass `--modifiers calculator,generator,checker,template`. The report gains `modifiers` and a `Modifier filter: kept X of Y` note. Read `deep.tool_in_top5` before recommending anything: at 3+ the SERP is tool pages, effort is `high`, `assets_needed` includes `interactive tool`, and an article will not win. Report only — nothing is built or queued.

### Competitor gap

`--gap` answers "which keywords do several of my competitors rank for that I do not, and that are easy enough?". Candidates are keywords where at least `min(2, competitors)` sampled competitors rank in the top 20 and the project does not (a lower own rank is kept and shown as `own_position`). Brand terms (competitor names/domains, the project's own) and `do_not_expand` terms are dropped. Shortlist and the SERP pass are unchanged.

- Competitors: `--competitors a.com,b.com`, else hosts of `competitors.yaml` seed `url`s in file order; max 5. The project's own domain comes from `site_url`.
- Report: `seeds_source: "gap"`, `competitors` (domains used), per-candidate `competitor_ranks[]` (`domain`, `position`) and optional `own_position`. Read `notes[]`: a failed competitor fetch degrades the run; a domain with 1000 rows is truncated; own-domain failure or a non-DataForSEO provider fails the run.
- Theme fit is your judgement: check candidates against `project.yaml` clusters and `do_not_expand` before recommending. Report only; spot-check a few by searching them.

### Cost

| Phase | Band | Control |
|-------|------|---------|
| Discovery | cents / seed (2 Labs calls) | ≤5–8 seeds first |
| Deep SERP | ~$0.002 live (cache free) | default 15; budget breaks soft |
| Full first pass | often **~$0.30–0.50** | preferred shape |

Prefer `--discover-only`, then a second run with tighter seeds + deep.

**Gap runs:** at most 6 Labs `ranked_keywords` requests (up to 5 competitors + the own domain), each returning up to 1000 rows. Estimated from the DataForSEO Labs pricing page ("All other endpoints", Live: $0.012 per request + $0.00012 per returned item): about $0.01-$0.13 per request, so **under ~$0.80 worst case** per run, typically far less because competitor rows are filtered by rank and KD server-side. TODO(owner): per-request price for Labs ranked_keywords not confirmed; the pricing page does not list this endpoint by name.

---

## Soft path

```text
resolve project → dataforseo gate
  → seeds (user > strategy > ask)
  → keyword-explore
  → interpret candidates (+ deep when present)
  → report wedges
  → optional Path B write (user agrees only)
```

### Interpret report (no invention)

| Field | Meaning |
|-------|---------|
| `candidates[]` | Ranked list (after deep: soft SERP first, else opportunity_score) |
| `candidates[].opportunity_score` / `opportunity_tier` | Shortlist multi-factor score (volume, KD, …) |
| `candidates[].deep` | Present if SERP ran: page types, soft_serp_score, effort, win_path, blockers, assets_needed, organic |
| `candidates[].deep.soft_serp_score` | 0–10 openness (higher = easier) |
| `candidates[].deep.effort` | `low` \| `medium` \| `high` from SERP shape |
| `candidates[].variants` / `variant_count` | Near-duplicate keywords folded into this topic (word-order / plural variants); volume and KD are the best member's, not summed; `discovered_count` is pre-grouping. Omitted when none |
| `candidates[].covered_by` | `{slug, matched_on}`: an existing article (draft or published) whose `target_keyword` or slug matches the topic or one of its variants (same token-similarity rule as variant grouping). Covered rows are never sent to the SERP pass. Gap mode + no `own_position` = a page exists and does not rank: a **harvest lead**, not a new article. Seed mode = **check the page before writing**. Omitted when none. Note `Coverage: K of M …` |
| `notes[]` | Failures, budget exhaustion, write errors |

**How to read win:**

| Signal | Meaning |
|--------|---------|
| High volume + low KD alone | Insufficient — need `deep` |
| `soft_serp_score` ≥ 6 + medium/low effort | Interesting wedge |
| `brand_in_top5` ≥ 3 | Need product/brand |
| AIO + informational intent | Funnel-only |
| High CPC + commerce page types | Product/affiliate prize |

Prefer mid-tail commercial wedges. Filter brand noise yourself (the tool has no vertical-specific cluster tables).

### Final user message

```markdown
## Keyword explore — {project}

**TL;DR:** {best wedge + effort}

**Seeds:** … ({explicit|strategy})
**Discovered:** N · **SERP deep:** M · **Report:** path

### Top candidates
| Rank | Keyword | Opp score | Soft | Effort | Vol | CPC |
|…|

### What it would take (top 1–3 with deep)
- Win path / blockers / assets
- Who ranks (domains + page_type)

### Not recommended
- Brand walls, AIO-only how-tos without a product

### Next (optional)
- `write-context -K "…" --worklist` (when the keyword is a run create row) → Path B write
```

### Handoff (only if user agrees)

| Goal | Tool |
|------|------|
| Content pipeline | `pageseeds run` (the run reads this skill's cached shortlist) |
| Skip | Report only — valid |

---

## Relationship

| Surface | Owns |
|---------|------|
| `/keyword-explore` | Exploratory demand + SERP-shape win path |
| `/pageseeds-run` | Existing-site scheduled SEO run; reads this skill's cached shortlist |

---

## Design (canonical scoring)

```text
seeds → parallel discovery
  → shortlist by score_opportunity
  → sequential SERP via serp_guard (cache + daily cap)
  → soft_serp_score + page-type win_advice (generic)
  → candidates[] with optional deep
```

No vertical-specific cluster tables. No product-winnability dual score.
