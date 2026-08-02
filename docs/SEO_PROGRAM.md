# SEO program ops (`seo_program.yaml`)

Operator guide for the **90-day SEO operating layer**: mode rotation, Primary /
harvest / tools / prune queues, and conversion-oriented metrics.

> **Not** a second content-strategy SOT. Theme gates stay in
> [project.yaml](./PROJECT_MD_STRATEGY.md) (`ProjectConfig` / `pageseeds-cli strategy`).
> This file sequences **what to do over weeks** against those gates.

| File | Role |
|------|------|
| `{repo}/.github/automation/project.yaml` | Primary / clusters / `do_not_expand` / Reddit — expansion gates |
| `{repo}/.github/automation/seo_program.yaml` | Goal, metrics, `current_mode`, queues — ops SOT |
| `{repo}/.github/automation/project.md` | Brand / prose prompts only |
| `weekly_seo_*.md` | Weekly run logs — **human narrative** + spacing clock (snapshots, not SOT) |
| `weekly_outcome_*.json` / `weekly_outcome_latest.json` | Weekly **machine handoff** — typed decisions, measures, `followup_prompt` (schema v1; epic #326) |
| `seo_program_review_*.md` | Monthly rebalance reports |

**Weekly dual write:** MD is for operators (readable story + Phase 0 spacing
filename clock). Outcome JSON is for follow-up agents / status tooling
(decisions ledger, open operator work, waiting rows). Same timestamp when
possible; `weekly_outcome_latest.json` is overwritten each successful report.
See [weekly-seo skill](../.agents/skills/weekly-seo/SKILL.md) § F. Report.

---

## Why YAML (not markdown)

Same lesson as epic #273: anything weekly / skill / CLI must **obey or score**
needs a typed shape. Freeform north-star MD re-creates silent drift
(agent invents mode, ignores Primary order, “interprets” queues).

| Layer | Format | Consumer |
|-------|--------|----------|
| Theme gates | `project.yaml` | CLI hard filters + research seed order |
| Program ops | `seo_program.yaml` | Skills: `/weekly-seo`, `/seo-program-review` |
| Narrative | weekly / review MD reports | Humans |
| Machine handoff | `weekly_outcome_*.json` | Follow-up agents / status tooling |

No multi-quarter task type in SQLite. Skills remain the writers/SOT for
`seo_program.yaml`. Rust **reads** `current_mode` (and a small task-type → mode
family map) warn-only on `create-task` when the type is outside the current
mode — never hard-refuses. There is no full program manager or mode enforcer
in core.

---

## Schema v1

Path: `.github/automation/seo_program.yaml`

```yaml
schema_version: 1                    # required; only 1 supported
goal: "one-line conversion north star"
current_mode: attract                # attract | harvest | tools | measure
mode_mix_this_month:
  attract: 2
  harvest: 2
  tools: 1
metrics:
  - id: blog_to_signup_started
    source: posthog                  # posthog | gsc | shortlist_catalog | other
    direction: up                    # up | down | flat_or_down_share
    note: optional
product_paths:                       # optional product URL map for harvest CTAs
  signup: /sign-up
  pricing: /pricing
  tools_hub: /tools
  examples: []
primary_backlog:
  - keyword: "…"                     # must align with project.yaml primary
    status: open                     # open | in_progress | shipped | measuring | done | dropped
    target_slug: null                # catalog slug when known
    notes: optional
harvest_queue:
  - slug: some-edu-slug              # or null until desk-filled
    goal: product_cta_bridge         # free string; common: product_cta_bridge | intro_bridge | internal_links
    status: open
    notes: optional
tools_queue:
  - path_or_slug: /covered-call-scanner
    status: open
    notes: optional
prune_queue:                         # optional; ≤15 open rows; monthly producer → weekly drain
  - slug: thin-drift-post            # or url: /blog/...
    cluster: tax                     # LEGACY / do_not_expand cluster name when known
    action: merge_into:keeper-slug   # merge_into:<keeper-slug> | noindex
    evidence: "104k imp/90d, 0 clicks"  # GSC (or desk) window evidence — required
    status: open                     # open | in_progress | done | dropped  (no measuring for noindex; merge may use measuring after submit)
    confirm: required                # MANDATORY on every noindex row; optional/omit for merge_into
    notes: optional
cluster_policy_hints:                # advisory for monthly review; project.yaml is gate SOT
  - name: "Cluster name matching project.yaml"
    preferred_status: maintain       # active | maintain | legacy | planned
    notes: optional
last_reviewed_at: "YYYY-MM-DD"       # ISO date; set by /seo-program-review
review_cadence_days: 30
notes: optional freeform (short)
```

### Mode → weekly action

| `current_mode` | Weekly preference | Cap (within ≤5 creates) |
|----------------|-------------------|-------------------------|
| **attract** | Primary backlog → `research-pull -K` / Path B write / publish / cluster | ≤2–3 new articles |
| **harvest** | Desk + harvest_queue → Path B fix (CTA / intro / links to product_paths) | ≤5 fixes |
| **tools** | tools_queue commercial / calculator / screener pages | ≤2–3 |
| **measure** | Due `content_outcome_review` + GSC movers + PostHog blog→signup (engine conversion tape `posthog_page_daily` when `collect_posthog` has run; weekly skill MCP desk remains for live ranking) | ≤1–2 reviews; light creates only if critical |

**Every week** still runs measure as a **mandatory side-pass when due** (≤1–2
due outcome reviews, post-`not_before` filter) even when `current_mode` is
attract/harvest/tools. **Non-compliant only if** ≥1 due and zero executed;
when due >2, remainder is **deferred under cap** (still compliant). “None due”
after filter is fine.

**Prune drain (side-pass, any mode):** When `prune_queue` has open rows, weekly
may drain them as a side-pass within ≤5 creates / ≤15 exec budgets — not a new
`current_mode` value. Prefer `merge_into` via existing Path B merge; never
auto-execute `noindex` (see below).

### Queue statuses

| Status | Meaning |
|--------|---------|
| `open` | Ready to pick |
| `in_progress` | Claimed this or prior week |
| `shipped` | Published / fix submitted this cycle |
| `measuring` | Waiting +30d `content_outcome_review` (or PostHog check); weekly skill reads back and flips to `done` with classification note when the review executes |
| `done` | Closed-loop complete (after measurement); leave or archive in monthly review |
| `dropped` | Explicitly abandoned (note why) |

**Do not** invent a second Primary keyword list. `primary_backlog[].keyword`
must match (or be a deliberate subset of) `project.yaml` →
`search_keywords.primary`. Expansion bans still come from `do_not_expand` +
LEGACY clusters.

### `prune_queue` (optional, schema v1)

- **Purpose:** turn LEGACY / `do_not_expand` inventory into concrete per-URL
  actions (shrink blocked territory). Theme gates still live in `project.yaml`;
  this queue is ops only.
- **Cap:** ≤15 open rows (thin board, same spirit as other queues).
- **Row fields:**
  - `slug` (preferred) or `url`
  - `cluster` (optional name matching `project.yaml`)
  - `action` = `merge_into:<keeper-slug>` **or** `noindex`
  - `evidence` (required string with GSC/desk numbers)
  - `status` = `open | in_progress | done | dropped` (and for **merge rows
    only**, `measuring` when +30d `content_outcome_review` is scheduled after
    merge-submit — same pattern as other queues)
  - `confirm: required` — **mandatory for every noindex row**; optional/omit
    for `merge_into`
  - short `notes` optional
- **Hard rule:** noindex rows are **never** auto-executed by skills or CLI.
  They are operator-manual only. `confirm: required` is the schema signal;
  weekly-seo surfaces them under Needs your decision / manual checklist.
- **`merge_into` rows:** weekly drains via existing Path B `merge-context` →
  session MDX → `merge-submit` (human confirm thresholds already in merge
  package: `MIN_KEEPER_WORDS`, `HUMAN_CONFIRM_CLICKS` / `IMPRESSIONS`).
  Successful merge-submit already spawns `content_outcome_review` (+30d) —
  no new task type.
- Do **not** invent keep-with-harvest-CTA here — that is `harvest_queue`.
- **Producer:** `/seo-program-review` (Prune scan). **Consumer:** `/weekly-seo`.

---

## Operator skills

| Skill | Cadence | Writes |
|-------|---------|--------|
| [weekly-seo](../.agents/skills/weekly-seo/SKILL.md) | Weekly | Report MD + outcome sidecar JSON pair + **narrow** queue status updates on `seo_program.yaml` |
| [seo-program-review](../.agents/skills/seo-program-review/SKILL.md) | ~monthly / product shift | Full rewrite of program file + proposed `project.yaml` cluster/keyword edits + review report |

Missing `seo_program.yaml`: weekly degrades to desk-default modes and **notes**
the gap (same honesty pattern as empty strategy). Prefer creating the file via
monthly review or a minimal seed rather than inventing modes in prose.

### gitignore

If automation is gitignored, force-include like strategy:

```gitignore
!.github/
!.github/automation/
!.github/automation/project.yaml
!.github/automation/seo_program.yaml
!.github/automation/project.md
```

---

## CLI surface (current)

No dedicated `pageseeds-cli seo-program` yet. Skills read/write the YAML on disk;
desk context still comes from existing tools (`strategy`, `research-context`,
`site-overview`, …). Promote to typed CLI when mode/queue validation needs to
be shared with non-skill callers.

### Intentional Primary write (Mode A)

When Ahrefs expansion is empty/covered but Primary gaps remain:

```bash
pageseeds-cli write-context -i <id> -p <path> -K "<primary or problem keyword>"
# auth_source: strategy_primary_or_problem — no -I / selection membership required
# still blocked by do_not_expand / LEGACY
pageseeds-cli write-submit … → publish-content -S <slug>
```

Classic path after research pick remains:
`write-context -I <research-task-id> -K <selectable keyword>`.

---

## Related

- [PROJECT_MD_STRATEGY.md](./PROJECT_MD_STRATEGY.md) — theme gates SOT  
- [weekly-seo skill](../.agents/skills/weekly-seo/SKILL.md) — weekly consumer  
- [seo-program-review skill](../.agents/skills/seo-program-review/SKILL.md) — monthly producer  
