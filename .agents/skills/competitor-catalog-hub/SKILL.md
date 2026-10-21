---
name: competitor-catalog-hub
description: >-
  Operator runbook: package a competitor-catalog hub pillar page via Path B
  write-context on hub.target_keyword after ≥3 seed-matched review spokes exist.
  Uses embedded hub-write skill + spokes + product_cta. Draft only — human publish.
  Not a new task type; not auto-spawn.
when-to-use: >-
  Triggers on "/competitor-catalog-hub", "competitor catalog hub", "hub for tool
  reviews", "SEO tools hub page", "write competitor hub after reviews".
argument-hint: "[project-name-or-id]"
user-invocable: true
metadata:
  short-description: "Catalog hub Path B draft after ≥3 review spokes"
---

# Competitor Catalog Hub — Operator Bible

> **Purpose:** After you have **≥3** competitor review articles (draft or
> published) whose `target_keyword` matches seed `primary_keyword` values, write
> one **hub / pillar** page via Path B. Matrix + hub SoT is
> `.github/automation/competitors.yaml`. No new task type; no auto-publish.

**Canonical path:** `.agents/skills/competitor-catalog-hub/SKILL.md`

Edit only the canonical copy; agent-specific skill directories should link to it or copy it.

## Invocation

```
/competitor-catalog-hub
/competitor-catalog-hub <project>
```

| Layer | Role |
|-------|------|
| **Matrix + hub SoT** | `{repo}/.github/automation/competitors.yaml` (`hub:` + seeds) |
| **Path B** | `write-context -K "<hub.target_keyword>"` → session MDX → `write-submit` |
| **Skill body** | Embedded `hub-write` (package includes `hub_spokes`, `product_cta`, `suggested_url`) |
| **Policy** | This skill |
| **Not this skill** | Single competitor review (`/competitor-review`), the `pageseeds run` loop, inventing hub slug |

---

## Hard rails

| # | Rule |
|---|------|
| 1 | **One hub per run** — package only `hub.target_keyword`; do not invent a second pillar. |
| 2 | **No publish** without human `publish-content -S <slug>`. Stop at draft. |
| 3 | **≥3 spokes required** — write-context fails closed if fewer seed→article matches. |
| 4 | **No invented competitor facts** on the hub — summarize spoke reviews; do not invent pricing. |
| 5 | **Do not invent `hub.slug`** — must be configured in `competitors.yaml` (`hub-{topic}` single segment). |
| 6 | Honor strategy `do_not_expand` / LEGACY (write-context will hard-block). |
| 7 | Do not create new task types; writing is Path B only. |

---

## Soft path

```text
resolve project path / id
  → load competitors.yaml (hub + seeds + product_cta)
  → confirm hub: { slug, title, target_keyword } present
  → confirm ≥3 articles with target_keyword matching seed primary_keyword
     (draft or published); if fewer → stop and run /competitor-review first
  → pageseeds-cli write-context -i <id> -p <path> -K "<hub.target_keyword>"
  → craft MDX using package skill_content (hub-write) + hub_spokes +
     product_cta + suggested_url + content_brief → write to target_file
  → pageseeds-cli write-submit … → stop at draft
  → report: hub slug, spoke count, draft path, publish reminder
```

### Path B

```bash
pageseeds-cli write-context -i <project-id> -p <project-path> \
  -K "<exact hub.target_keyword>"
# Session writes MDX to package.target_file using skill_content (hub-write)
pageseeds-cli write-submit -i <project-id> -p <project-path> \
  --file <target_file> -K "<same keyword>"
```

Authorization: `hub.target_keyword` (normalized) → `auth_source: competitor_catalog_hub`
without `-I`. Skill name must be `hub-write`. Package includes:
- `suggested_url` = hub.slug (single segment)
- `hub_spokes` (≥3, ≤8): title + url_slug
- `product_cta` from catalog
- `target_words` guidance 1500 (min still 800)

`write-submit` returns a dry-run `source_gate` verdict (#486). Report it. Name a declared source only if its payload is used in an artefact; never add `sources` to `project.yaml` to clear it.

### Final user message

```markdown
## Competitor catalog hub — {project}

**TL;DR:** Hub draft MDX at {slug or path}; **not published**.

**Hub keyword:** `{hub.target_keyword}`
**Suggested URL / slug:** `{hub.slug}`
**Spokes packaged:** {n} (seed-matched reviews)
**Draft:** `{slug}` / `{path}`
**Publish:** human only — `pageseeds-cli publish-content -i … -S {slug}`
```

---

## Relationship

| Surface | Owns |
|---------|------|
| `/competitor-catalog-hub` | One hub pillar Path B draft after ≥3 spokes |
| `/competitor-review` | One seed → matrix + review spoke draft |
| `competitors.yaml` `hub:` | slug + title + target_keyword (do not invent) |
| `write-context` / `write-submit` | Package + validate + catalog draft |

---

## Design

```text
competitors.yaml hub + seeds
  → ≥3 review articles (target_keyword = seed primary_keyword)
  → write-context -K hub.target_keyword
     → hub-write + hub_spokes + product_cta + suggested_url
  → session MDX → write-submit → human publish-content
```
