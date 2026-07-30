# Machine-parseable `project.md` strategy (operator checklist)

Operator rollout checklist for **epic #273**. Ensures each customer site’s content strategy is machine-readable so hard gates and ACTIVE rank boosts actually fire.

> **Source of truth:** module docs + parse contract in `crates/pageseeds-core/src/strategy/mod.rs` (loader `load_project_strategy`; unit-test `FIXTURE` and `StrategyLoadStatus`). Do **not** invent new tokens or heading levels outside that contract.

This is **operator config + verification**, not a product feature change. Strategy load quality is already reported as `empty` | `partial` | `ok` (#276); gates still no-op on empty — operators must verify status, not expect a hard fail.

---

## Why it matters

Strategy hard gates and ACTIVE rank boosts only apply when `{repo}/.github/automation/project.md` matches the machine parse contract.

| If strategy is… | What the product does |
|-----------------|------------------------|
| **Empty** (missing file, wrong structure, no recognized sections) | All hard gates **no-op**. `apply_strategy_filter` early-returns when `is_empty()` — every candidate kept as `StrategyRank::Neutral`. `strategy_blocks_expansion` returns `false`. Load quality: `StrategyLoadStatus::Empty` / `content_strategy.status: "empty"`. |
| **Partial / ok** | `do_not_expand` + LEGACY hard-drop seeds/themes; ACTIVE/primary get `StrategyRank::ActiveBoost`; MAINTAIN deprioritizes new seeds. Path B injects Primary/ACTIVE bullets as shortlist fuel. Load quality: `partial` or `ok` on `content_strategy.status`. |

Parse is **best-effort and never fails research**. Gates remain no-op when strategy is empty. **#276** is the existing load-quality status signal (`StrategyLoadStatus` / `content_strategy.status` as `empty` | `partial` | `ok` on research-context) — **not** a hard-fail. Operators verify via `research-context` → `content_strategy.status` (and the free `strategy` command).

---

## File location

| Item | Value |
|------|--------|
| Path | `{repo}/.github/automation/project.md` |
| Resolution | `ProjectPaths` automation dir (`.github/automation/`) |
| Loader | `load_project_strategy` / `load_project_strategy_from_project_path` / `load_for_project` |

Other prose in `project.md` is fine; only recognized `##` / `###` sections feed the strategy types.

### gitignore

If `.github/automation/` (or the whole `.github/`) is gitignored, **force-include** the strategy file so it ships with the content repo.

Git cannot re-include a file under an ignored parent: a lone `!.github/automation/project.md` is **not** enough when a parent path is ignored. Un-ignore each ancestor first, then the file. Match the chain to **your** ignore pattern:

**Parent `.github/` ignored** (or broader ignore covering it):

```gitignore
!.github/
!.github/automation/
!.github/automation/project.md
```

**Only `.github/automation/` ignored** (`.github/` itself still tracked):

```gitignore
!.github/automation/
!.github/automation/project.md
```

Variants such as `!.github/automation/*` plus an explicit file rule also work — use whatever matches how the parent was ignored. Without a correct chain, local gates may work while CI/other clones see empty strategy.

---

## Required structure

**H2 must be `##`, not `###`.** Heading wording is flexible and **case-insensitive**, but must contain the substrings below.

### Search Keywords

- Top section: `## …` whose heading contains **`search keyword`** (e.g. `## Search Keywords`).
- Under it, `###` buckets:

| Heading contains (case-insensitive) | Maps to |
|-------------------------------------|---------|
| `primary` | `primary_keywords` |
| `problem` | `problem_keywords` |
| `audience` | `audience_keywords` |
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

---

## Status semantics (must match code)

| Status | Product effect | Operator use |
|--------|----------------|--------------|
| **ACTIVE** | Expand / seed / rank boost (`matches_active_or_primary` → `StrategyRank::ActiveBoost`). Bullets inject as shortlist fuel (`strategy_active`). | Pillars you still want to grow. |
| **MAINTAIN** | Keep inventory; deprioritize new seeds (`StrategyRank::Maintain`). Not a hard drop. | **Prefer for high-impression money pages** instead of LEGACY. |
| **LEGACY** | Hard-block expansion (`matches_legacy_cluster`). Match = listed bullet phrases (substring) **and/or** multi-token name overlap only. **Single-token names do not ban all substrings** (e.g. LEGACY cluster named `Services` alone does not block every keyword containing `"services"`). | True dead pillars / service lines you never want to seed. |
| **PLANNED** | Intentional new pillar only; no ACTIVE boost, no LEGACY hard block via status. | Future pillars not yet expanded. |
| **Unknown** / missing status | No boost, no hard block via status. Soft annotation only when matched. | Fix the heading parentheses. |

### Traffic rule (epic #273)

**Do not LEGACY high-impression pillars for “purity.”** Use **MAINTAIN** + CTR/desk fixes so inventory stays live and expansion is deprioritized without nuking research around money pages.

Hard blocks that *should* stay hard: put explicit short phrases under `do_not_expand` (and/or LEGACY clusters with multi-token names + seed bullets that match the ban surface you intend).

---

## Verify commands

From any machine with `pageseeds-cli` and the project set up:

```bash
# Parse project.md at repo root (or pass -p)
pageseeds-cli strategy -p /path/to/site

# Full research-context package (includes content_strategy summary)
pageseeds-cli research-context -i <project_id>
```

### What “non-empty useful” looks like

- ≥1 **primary** keyword and/or ≥1 **ACTIVE** cluster with seed-phrase bullets.
- `do_not_expand` bullets are short phrases — spot-check **length of each bullet** (reject multi-sentence policy dumps).
- Clusters you care about show status `active` / `maintain` / `legacy` / `planned` in JSON (not all `unknown`).
- `research-context` → `content_strategy.status` is `ok` (or at least `partial` with the sections you care about populated) — not `empty`.
- Empty / missing sections → empty strategy → gates no-op; status reports `empty` (fix structure + re-check `content_strategy.status`).

`strategy` and `research-context` are free / read-only; see [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md).

---

## Template (test fixture shape)

Canonical structure from strategy unit tests (`strategy/mod.rs` `FIXTURE` constant). Copy and edit names/phrases per site:

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

## Operator checklist (per site)

1. [ ] Confirm path: `.github/automation/project.md` in the **content** repo.
2. [ ] If `.github/` or `.github/automation/` is gitignored, add the **full un-ignore chain** (ancestors + file) — see [gitignore](#gitignore); a lone `!.github/automation/project.md` is not enough under an ignored parent.
3. [ ] `## Search Keywords` (H2) with `### Primary Keywords` bullets (seed phrases).
4. [ ] `do_not_expand` section with **short** ban phrases only.
5. [ ] `## Content Clusters…` (H2) with `### Cluster N: Name (STATUS)` and seed bullets.
6. [ ] Money / high-impr pillars → **MAINTAIN** (or ACTIVE if still growing), **not** LEGACY.
7. [ ] Dead lines → LEGACY with multi-token names and/or explicit bullets; optional overlap with `do_not_expand`.
8. [ ] Run `pageseeds-cli strategy -p .` — non-empty primary and/or ACTIVE clusters.
9. [ ] Spot-check `research-context` → `content_strategy.status` (`ok` / `partial` / not `empty`) and summary fields when researching.

---

## Related

| Doc / issue | Role |
|-------------|------|
| `crates/pageseeds-core/src/strategy/mod.rs` | Parse contract, match policy, filter ranks, `StrategyLoadStatus` / `FIXTURE` |
| [WORKFLOW_ENGINE.md](./WORKFLOW_ENGINE.md) — Topic Health | How strategy injects into shortlist / Path B |
| [CLI_GETTING_STARTED.md](./CLI_GETTING_STARTED.md) | Install + setup |
| [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md) | `strategy` / `research-context` free tools |
| Epic #273 | Parent operator rollout |
| #276 | Load-quality status `empty` \| `partial` \| `ok` on research-context (`content_strategy.status`); **not** a hard-fail — gates still no-op on empty |
