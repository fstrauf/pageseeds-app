# Project strategy config (operator checklist)

Operator rollout checklist for **epic #273** + **#290–#294**. Ensures each customer site’s content strategy is machine-readable so hard gates and ACTIVE rank boosts actually fire.

> **Live source of truth:** `.github/automation/project.yaml` (`ProjectConfig` schema v1).  
> Load path: `ensure_project_config` → `load_project_strategy` / `pageseeds-cli strategy`.  
> See `crates/pageseeds-core/src/project_config/` and `strategy/mod.rs`.

`project.md` is **prose / brand context** for prompts — not the structured strategy SOT after YAML cutover. The MD heading contract below is **legacy import-only** (migrator input).

This is **operator config + verification**, not a product feature change. Strategy load quality is reported as `empty` | `partial` | `ok` (#276); gates still no-op on empty — operators must verify status, not expect a hard fail.

---

## Why it matters

Strategy hard gates and ACTIVE rank boosts only apply when structured strategy is non-empty in `project.yaml` (or successfully migrated from legacy MD).

| If strategy is… | What the product does |
|-----------------|------------------------|
| **Empty** (missing/empty YAML fields, wrong structure, no recognized sections after migrate) | All hard gates **no-op**. `apply_strategy_filter` early-returns when `is_empty()` — every candidate kept as `StrategyRank::Neutral`. `strategy_blocks_expansion` returns `false`. Load quality: `StrategyLoadStatus::Empty` / `content_strategy.status: "empty"`. |
| **Partial / ok** | `do_not_expand` + LEGACY hard-drop seeds/themes; ACTIVE/primary get `StrategyRank::ActiveBoost`; MAINTAIN deprioritizes new seeds. Path B injects Primary/ACTIVE bullets as shortlist fuel. Load quality: `partial` or `ok` on `content_strategy.status`. |

Parse/load is **best-effort and never fails research**. Gates remain no-op when strategy is empty. **#276** is the existing load-quality status signal (`StrategyLoadStatus` / `content_strategy.status` as `empty` | `partial` | `ok` on research-context) — **not** a hard-fail. Operators verify via `research-context` → `content_strategy.status` (and the free `strategy` command).

---

## Live files

| Item | Path | Role |
|------|------|------|
| **Structured SOT** | `{repo}/.github/automation/project.yaml` | `ProjectConfig` v1: search keywords, clusters, Reddit knobs |
| **Prose / brand** | `{repo}/.github/automation/project.md` | Prompt context (summary, brand voice) — not strategy field SOT |
| **Reply safety** | `{repo}/.github/automation/reddit/_reply_guardrails.md` | Reddit reply constraints |
| **Legacy (migrate only)** | `reddit_config.md` (+ MD strategy sections in `project.md`) | Import source for `migrate-project-config` |

### Field map (`project.yaml` / `ProjectConfig`)

| YAML path | Maps to strategy / Reddit |
|-----------|---------------------------|
| `schema_version` | Must be `1` |
| `product_name` | Product name (Reddit + prompts) |
| `search_keywords.primary` | `primary_keywords` |
| `search_keywords.problem` | `problem_keywords` |
| `search_keywords.audience` | `audience_keywords` |
| `search_keywords.do_not_expand` | `do_not_expand` |
| `clusters[]` | `name`, `status` (`ACTIVE` / `MAINTAIN` / `LEGACY` / `PLANNED`), `keywords[]` |
| `reddit.mention_stance` | `OPTIONAL` / `RECOMMENDED` / `REQUIRED` / `OMIT` |
| `reddit.seed_subreddits` | Bare names, no `r/` prefix |
| `reddit.excluded_subreddits` | Bare names |
| `reddit.trigger_topics` | Search topics |
| `reddit.query_keywords` | Compact search queries |

Init (`pageseeds-cli setup` / `initialize_project_workspace`) writes schema-v1 **defaults** (empty keyword/cluster lists; Reddit `mention_stance: Optional`). Fill strategy fields in YAML (or migrate from legacy MD).

### Commands

```bash
# Structured strategy as JSON (from project.yaml via ensure; auto-migrates legacy MD)
pageseeds-cli strategy -p .

# Readiness: YAML vs legacy MD
pageseeds-cli project-config-status -p .

# Deterministic MD → YAML migrator (no LLM). Dry-run first:
pageseeds-cli migrate-project-config -p . --dry-run
pageseeds-cli migrate-project-config -p .

# Full research-context package (includes content_strategy summary)
pageseeds-cli research-context -i <project_id>
```

### gitignore

If `.github/automation/` (or the whole `.github/`) is gitignored, **force-include** the strategy files so they ship with the content repo.

Git cannot re-include a file under an ignored parent: a lone `!.github/automation/project.yaml` is **not** enough when a parent path is ignored. Un-ignore each ancestor first, then the file:

```gitignore
!.github/
!.github/automation/
!.github/automation/project.yaml
!.github/automation/project.md
```

---

## Status semantics (must match code)

| Status | Product effect | Operator use |
|--------|----------------|--------------|
| **ACTIVE** | Expand / seed / rank boost (`matches_active_or_primary` → `StrategyRank::ActiveBoost`). Bullets inject as shortlist fuel (`strategy_active`). | Pillars you still want to grow. |
| **MAINTAIN** | Keep inventory; deprioritize new seeds (`StrategyRank::Maintain`). Not a hard drop. | **Prefer for high-impression money pages** instead of LEGACY. |
| **LEGACY** | Hard-block expansion (`matches_legacy_cluster`). Match = listed bullet phrases (substring) **and/or** multi-token name overlap only. **Single-token names do not ban all substrings** (e.g. LEGACY cluster named `Services` alone does not block every keyword containing `"services"`). | True dead pillars / service lines you never want to seed. |
| **PLANNED** | Intentional new pillar only; no ACTIVE boost, no LEGACY hard block via status. | Future pillars not yet expanded. |
| **Unknown** / missing status | No boost, no hard block via status. Soft annotation only when matched. | Fix the status token. |

### Traffic rule (epic #273)

**Do not LEGACY high-impression pillars for “purity.”** Use **MAINTAIN** + CTR/desk fixes so inventory stays live and expansion is deprioritized without nuking research around money pages.

Hard blocks that *should* stay hard: put explicit short phrases under `search_keywords.do_not_expand` (and/or LEGACY clusters with multi-token names + seed keywords that match the ban surface you intend).

---

## Verify

From any machine with `pageseeds-cli` and the project set up:

```bash
pageseeds-cli strategy -p /path/to/site
pageseeds-cli project-config-status -p /path/to/site
pageseeds-cli research-context -i <project_id>
```

### What “non-empty useful” looks like

- ≥1 **primary** keyword and/or ≥1 **ACTIVE** cluster with seed-phrase keywords.
- `do_not_expand` entries are short phrases — spot-check **length of each** (reject multi-sentence policy dumps).
- Clusters you care about show status `active` / `maintain` / `legacy` / `planned` in JSON (not all `unknown`).
- `research-context` → `content_strategy.status` is `ok` (or at least `partial` with the sections you care about populated) — not `empty`.
- Empty / missing fields → empty strategy → gates no-op; status reports `empty`.

`strategy`, `project-config-status`, and `research-context` are free / read-only; see [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md).

---

## Operator checklist (per site)

1. [ ] Confirm path: `.github/automation/project.yaml` in the **content** repo (setup creates defaults).
2. [ ] Fill `search_keywords` + `clusters` (or migrate from legacy MD).
3. [ ] If you still have legacy `project.md` strategy headings / `reddit_config.md`, run `migrate-project-config --dry-run` then migrate.
4. [ ] Money / high-impr pillars → **MAINTAIN** (or ACTIVE if still growing), **not** LEGACY.
5. [ ] Dead lines → LEGACY with multi-token names and/or explicit keywords; optional overlap with `do_not_expand`.
6. [ ] Run `pageseeds-cli strategy -p .` — non-empty primary and/or ACTIVE clusters.
7. [ ] Spot-check `research-context` → `content_strategy.status` (`ok` / `partial` / not `empty`).

---

## Legacy MD heading contract (import-only)

> **Not live SOT.** Used by the deterministic migrator (`migrate_project_config`) and residual MD parsers. Prefer editing `project.yaml` for day-to-day strategy.

**H2 must be `##`, not `###`.** Heading wording is flexible and **case-insensitive**, but must contain the substrings below.

### Search Keywords

- Top section: `## …` whose heading contains **`search keyword`** (e.g. `## Search Keywords`).
- Under it, `###` buckets:

| Heading contains (case-insensitive) | Maps to |
|-------------------------------------|---------|
| `primary` | `primary_keywords` → `search_keywords.primary` |
| `problem` | `problem_keywords` → `search_keywords.problem` |
| `audience` | `audience_keywords` → `search_keywords.audience` |
| `do not expand` / `do-not-expand` / `never expand`, or `legacy` + (`keyword` \| `service`) | `do_not_expand` |

Example bucket titles that parse: `### Primary Keywords`, `### Legacy Service Keywords (do not expand)`.

### Content Clusters

- Top section: `## …` whose heading contains **`content cluster`** (e.g. `## Content Clusters`, `## Content Clusters And Priorities`).
- Under it, one cluster per `###` heading:

```text
### Cluster N: Name (ACTIVE|MAINTAIN|LEGACY|PLANNED)
- seed phrase
- another seed phrase
```

- Status token is the first recognized word **inside parentheses** (case-insensitive). Unknown / missing → `ClusterStatus::Unknown` (no boost, no hard block **via status**).
- `Cluster N:` prefix is stripped from the stored name; bare `### Name (ACTIVE)` also works.

### Bullet form

| Accepted | Notes |
|----------|--------|
| `- phrase` | Preferred |
| `* phrase` | Also accepted |
| `+ phrase` | Also accepted |

- Short **seed phrases**, not full article titles.
- Do **not** wrap entire policy paragraphs as one `do_not_expand` bullet — matching is case-insensitive **substring**, so a long bullet is a huge ban surface and hard to audit.
- Empty lines and HTML comments (`<!-- … -->`) are ignored.

### Template (legacy MD shape → migrate)

Canonical structure from strategy unit tests (`strategy/mod.rs` `FIXTURE` constant). Useful for migration input; after migrate, edit `project.yaml`:

```markdown
# Example Project

## Search Keywords

### Primary Keywords
- seo tools
- keyword research

### Problem Keywords
- thin content

### Audience Keywords
- content marketers

### Legacy Service Keywords (do not expand)
- custom web design
- wordpress agency

## Content Clusters And Priorities

### Cluster 1: SEO Fundamentals (ACTIVE)
- on-page seo
- technical seo

### Cluster 2: Alternatives (MAINTAIN)
- competitor alternatives

### Cluster 3: Services (LEGACY)
- web design packages

### Cluster 4: New Pillar (PLANNED)
- ai content ops
```

---

## Related

| Doc / issue | Role |
|-------------|------|
| `crates/pageseeds-core/src/project_config/` | `ProjectConfig`, load/save, migrate, ensure |
| `crates/pageseeds-core/src/strategy/mod.rs` | Match policy, filter ranks, `StrategyLoadStatus` |
| [WORKFLOW_ENGINE.md](./WORKFLOW_ENGINE.md) — Topic Health | How strategy injects into shortlist / Path B |
| [CLI_GETTING_STARTED.md](./CLI_GETTING_STARTED.md) | Install + setup |
| [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md) | `strategy` / `research-context` free tools |
| Epic #273 | Parent operator rollout |
| #276 | Load-quality status `empty` \| `partial` \| `ok` |
| #290–#294 | YAML schema, migrator, ensure, runtime cutover, YAML-first init |
