---
name: weekly-seo-continue
description: >-
  Drain open operator decisions from the latest weekly SEO outcome JSON for one
  PageSeeds project without starting a full weekly pass. Use when the user wants
  to continue weekly leftovers, resolve Needs-your-decision items, or
  /weekly-seo-continue. Operator only — never edit pageseeds-app source.
when-to-use: >-
  Triggers on "/weekly-seo-continue", "weekly SEO continue", "drain weekly
  decisions", "continue weekly SEO leftovers", "resolve open operator decisions",
  "weekly outcome follow-up".
argument-hint: "[project-name-or-id]"
user-invocable: true
metadata:
  short-description: "Drain open weekly outcome decisions (no full weekly)"
---

# Weekly SEO Continue — Drain Open Outcome Decisions

> Companion to [weekly-seo](../weekly-seo/SKILL.md). This skill **only drains**
> open operator decisions from the latest outcome JSON. It does **not** run a
> full weekly pass, invent mode work, or reset the 5-day spacing clock.

## Invocation

```
/weekly-seo-continue
/weekly-seo-continue coffee
/user:weekly-seo-continue
```

| Arg | Meaning |
|-----|---------|
| *(none)* | Use cwd / setup default project |
| `<id-or-name>` | Resolve one project (prefer registry **`id`**) |

Prefer the **customer project** (cwd outside `pageseeds-app`). Requires
`pageseeds-cli` on PATH when possible.

---

## Role

| Layer | Role |
|-------|------|
| **weekly-seo** | Produces report MD + outcome JSON (full desk → ≤5 actions) |
| **This skill** | Loads latest outcome → resolves open `operator_act` / `operator_confirm` → rewrites outcome |
| **weekly-seo-status** | Cross-project recency board only |
| **Product source** | **Out of scope** — never patch product crates mid-continue |

**Relationship:** weekly-seo **produces** the outcome; weekly-seo-continue
**drains** only. When leftovers remain after a weekly, prefer
`/weekly-seo-continue <id>` over re-running full `/weekly-seo`.

---

## Canonical identity (do not freestyle)

| Field | Source | Role |
|-------|--------|------|
| **`id`** | `pageseeds-cli list-projects` / `projects.id` | **Canonical** — invoke by id |
| **`name`** | same | Human label only |
| **`path`** | same | Content root for outcome files |

Do **not** match on folder name or domain alone when the registry has a clear id.

---

## Source of truth

### Happy path — CLI

```bash
pageseeds-cli weekly-outcome -p .
# or after setup:
pageseeds-cli weekly-outcome -i <id> -p <path>
# compact operator view:
pageseeds-cli weekly-outcome -p . --summary
```

Prefer full JSON for rewrites; `--summary` is fine for a first scan of open
operator counts.

### Fallback — file read

If CLI missing/fails:

```text
{project.path}/.github/automation/weekly_outcome_latest.json
```

Else newest `weekly_outcome_YYYYMMDD_HHMMSS.json` by filename (same resolve
order as product). **Do not invent** decisions, measures, or project ids.

Schema fixture:
[docs/examples/weekly_outcome_example.json](../../../docs/examples/weekly_outcome_example.json)
(and weekly-seo § Outcome sidecar).

---

## Hard rails

| # | Rule |
|---|------|
| 1 | **CLI only** for data/tasks (+ PostHog only if a decision **truly** needs it — default **no** full PostHog desk). No direct DB writes, no hand-editing MDX. |
| 2 | **No product source edits** under `pageseeds-app`. Skill/doc product work is a separate session. |
| 3 | Load SOT: `pageseeds-cli weekly-outcome -p .` (happy path) or `weekly-outcome --summary`; fallback file-read `weekly_outcome_latest.json` if CLI missing. |
| 4 | Work **only** open decisions: prefer `operator_act` then `operator_confirm` (human confirm still required for confirm kind). |
| 5 | **Do not** start a mode-executing weekly: no Phase 0 mode creates, no research week freestyle, no ≤5 growth plan unless a decision *is* that create. |
| 6 | Budget: small — ≤**5** exec-like ops, ≤**2** creates if a decision requires create; prefer `cancel` / `update-task-status` / `list-tasks` / `execute-task` on existing rows. |
| 7 | After each resolved decision: set `status` to `done` / `dropped` / `deferred` / `watching` with one-line note (use `pending` / `guidance` appropriately). |
| 8 | **Rewrite** `weekly_outcome_latest.json` (+ optional `weekly_outcome_{ts}_continue.json` archive) and refresh `followup_prompt` for remaining opens. |
| 9 | **Spacing / MD ban:** continue **never** writes any file matching `weekly_seo_*.md` (including `weekly_seo_continue_*` and labeled `weekly_seo_{ts}.md`). That glob is Phase 0 / weekly-seo-status SOT for spacing + Runs(7d); any match pollutes the clock and breaks `weekly_seo_YYYYMMDD_HHMMSS` parse. **Default and only artifacts:** outcome JSON rewrite (+ optional `_continue` archive). Prefer **no** human MD; if a log is ever needed, use a prefix **outside** that glob only — `weekly_continue_{ts}.md` — never `weekly_seo_*`. |
| 10 | If **zero** open operator decisions: say so; optionally recheck `waiting` for expiry; exit without inventing work. |
| 11 | `product_gap` / `optional_backlog`: **list only**; do not expand into eng implementation mid-continue. |

### Explicit bans

| Ban | Do instead |
|-----|------------|
| Full weekly desk / soft path A–F | Outcome load → open operator drain only |
| **Any** write matching `weekly_seo_*.md` (incl. `weekly_seo_continue_*`, labeled `weekly_seo_{ts}.md`) | Outcome rewrite only (`weekly_outcome_latest.json` + optional `weekly_outcome_{ts}_continue.json`); prefer no MD; human log only as `weekly_continue_{ts}.md` if ever needed |
| Inventing work when zero open `operator_act` / `operator_confirm` | Clean no-op message + optional waiting recheck |
| Implementing `product_gap` mid-continue | List under Product gaps; escalate outside this skill |
| Agent-executed noindex / bulk deindex | `operator_confirm` stays human; escalate confirm |
| Full PostHog desk by default | Skip unless a decision’s guidance truly needs it |
| Expanding `optional_backlog` into new growth creates | Leave deferred / list only |

---

## Decision kinds (schema v1)

| Kind | Continue behavior |
|------|-------------------|
| `operator_act` | **Primary drain** — mechanical / nearly-mechanical CLI work |
| `operator_confirm` | **Secondary** — present + wait for human yes; never auto-confirm |
| `waiting` | Optional expiry recheck only when zero operators or user asks; prefer leave `watching` |
| `product_gap` | List only — do not implement product |
| `optional_backlog` | List only — do not invent growth week |

**Statuses:** `open | watching | done | deferred | dropped`.

**Top-level `status`:** `needs_attention` **iff** any decision has
`status=open` and `kind` in `{operator_act, operator_confirm}`; else `ok`.

**`followup_prompt`:** rebuild from remaining **open** `operator_act` +
`operator_confirm` (commands + pending one-liners); optional note that full
JSON still has waiting/product rows.

---

## Procedure

```text
resolve project → weekly-outcome (JSON)
  → filter open operator_act / operator_confirm
  → present plan (interactive) or hands-off execute mechanical acts
  → for each: CLI evidence → act or escalate confirm
  → rewrite weekly_outcome_latest.json
  → final user message: resolved / still open / outcome path
```

### 1. Resolve project

```bash
pageseeds-cli setup --path . --yes   # once if needed
pageseeds-cli list-projects          # when arg is ambiguous
```

Use registry **`id`** and `path`. Abort clearly if no project / path missing.

### 2. Load latest outcome

```bash
pageseeds-cli weekly-outcome -p .
pageseeds-cli weekly-outcome -p . --summary
```

Fallback:

```bash
cat <path>/.github/automation/weekly_outcome_latest.json
```

If no outcome file: say so and stop — do **not** invent a weekly plan or fake
decisions. Suggest `/weekly-seo <id>` when a full pass is wanted.

### 3. Filter open operator work

Keep decisions where:

- `kind` ∈ `{operator_act, operator_confirm}`
- `status` ∈ `{open}` (treat missing status as open only if clearly unfinished)

**Order:** all open `operator_act` first, then open `operator_confirm`.

If **zero** open operator decisions:

1. Message: no open operator acts/confirms to drain.
2. Optionally recheck `waiting` rows: if `expires_at` past → note / set
   `dropped` or refresh `pending`; if still lagging leave `watching`.
3. Optionally refresh `followup_prompt` / top-level `status` if waiting-only.
4. **Exit** — do not invent desk work or growth creates.

### 4. Plan (interactive vs hands-off)

Present a short plan:

```markdown
## Continue plan — {project id / name}

Open operator decisions ({N}):

1. **operator_act** `{id}` — {title}
   - Pending: …
   - Proposed: list-tasks / execute / cancel / update-status …
2. **operator_confirm** `{id}` — {title}
   - Pending: human confirm required
   - Will not auto-act

Budget: ≤5 exec · ≤2 creates · no full weekly
```

- **Interactive:** wait for approval (or act on explicit “go” / “hands-off”).
- **Hands-off:** state plan, then execute mechanical `operator_act` only;
  still stop on every `operator_confirm`.

### 5. Resolve each open operator decision

For each decision, in order:

#### Evidence (cheap CLI first)

```bash
pageseeds-cli list-tasks -i <id> -p <path>   # filter by type / related_task_ids
pageseeds-cli get-task -I <task-id>
# when guidance/commands say so:
pageseeds-cli site-overview -i <id> -p <path>
```

Honor `commands[]` on the decision when present. Do **not** invent GSC/deploy
truth without tool output.

#### `operator_act` (mechanical)

Prefer in this order:

1. `list-tasks` / `get-task` — see if already done / noise
2. `execute-task -I <id>` on existing actionable rows (counts toward ≤5 exec)
3. `update-task-status -I <id> -s done|cancelled|…` when mechanical disposal
4. Cancel / close fan-out noise when guidance says so
5. Create **only** if the decision itself requires a create **and** budget ≤2
   creates remains — never freestyle a growth plan

After act: set decision `status` to:

| Result | status | Note in `pending` / `guidance` |
|--------|--------|--------------------------------|
| Fully resolved | `done` | One-line what was done + evidence |
| No longer relevant | `dropped` | Why |
| Park for later intentionally | `deferred` | Why + when to revisit |
| External lag after partial work | `watching` | What is lagging; set `expires_at` if useful |

#### `operator_confirm` (human required)

- Surface title, evidence, and exact human step (e.g. noindex in CMS, `-y` on
  high-traffic merge).
- **Do not** auto-confirm, auto-noindex, or pass `-y` without explicit user OK
  in this session.
- If user confirms: perform the allowed follow-up (or mark done if they already
  did the manual step), then set `status=done` with note.
- If user declines: `dropped` or `deferred` with reason.
- If user not present / no answer: leave `open`; refresh `guidance` if rechecked.

#### Out of scope mid-loop

- Do not implement `product_gap`.
- Do not expand `optional_backlog` into creates.
- Do not start research week / mode queue drain / Path B write freestyle
  unless that **is** the open decision’s explicit command.

### 6. Rewrite outcome JSON

Under `<project-path>/.github/automation/`:

| File | Action |
|------|--------|
| `weekly_outcome_latest.json` | **Overwrite** with updated decisions, statuses, notes |
| `weekly_outcome_{YYYYMMDD_HHMMSS}_continue.json` | **Optional** archive of this continue pass |

Keep `schema_version: 1`, `kind: weekly_seo_outcome`, project fields, prior
`measures[]` (do not invent new measures unless you actually shipped something
this continue — then append sparingly).

Update:

1. Each touched decision’s `status`, `pending`, `guidance` (and commands if
   still useful).
2. Top-level `status` → `needs_attention` or `ok` (rule above).
3. `generated_at` → now (ISO).
4. `summary` / `headline` — short continue TL;DR (optional but preferred).
5. `recommended_next` — remaining opens + waiting only.
6. **`followup_prompt`** — rebuild from remaining open operator kinds.

Do **not** delete historical `done` / `dropped` rows from the ledger unless the
user asks for a prune; prefer status updates in place (stable `id`).

### 7. Human log (prefer none)

**Default: no MD.** Artifacts are outcome JSON only (step 6).

Hard ban: **never** create or overwrite any path matching
`weekly_seo_*.md` under automation (including `weekly_seo_continue_*` and
any labeled `weekly_seo_{ts}.md`). Phase 0 / weekly-seo-status treat that
glob as the spacing clock and Runs(7d) source; continue must not pollute it.

If a human narrative is ever required (rare; user explicitly asks), use a
prefix **outside** that glob:

```text
{project}/.github/automation/weekly_continue_{YYYYMMDD_HHMMSS}.md
```

Keep it short (resolved / still open / outcome path). Prefer chat final
message over writing MD at all.

### 8. Final user message (no JSON dumps)

```markdown
## Weekly SEO continue — {project name} ({date})

**TL;DR:** drained {n} decisions · {m} still need you · outcome {ok|needs_attention}

**Resolved**
- `{id}` ({kind}) → {done|dropped|deferred|watching} — one line

**Still need you** (open operator_act + operator_confirm)
- `{id}` — … → `command` (if any)

**Waiting / other** (optional one-liners)
- …

**Not expanded** (product_gap / optional_backlog — listed only)
- …

**Outcome:** {path to weekly_outcome_latest.json}
**Archive:** {path or “none”}
**Human log:** none (outcome only) · or `weekly_continue_{ts}.md` if user asked

**Spacing:** continue wrote **no** `weekly_seo_*.md` and did **not** reset the 5-day clock.
```

Rules:

- No full JSON dumps of outcome / list-tasks.
- No inventing open work when the ledger is clean.
- Offer `/weekly-seo <id>` only if the user wants a **full** weekly — never as
  the default way to finish leftovers.

---

## User install (discovery symlinks)

Edit only the canonical skill under `.agents/`. Symlinks for discovery:

| Scope | Path | When it loads |
|-------|------|---------------|
| Repo | `pageseeds-app/.grok/skills/weekly-seo-continue/SKILL.md` → `../../../.agents/skills/weekly-seo-continue/SKILL.md` | Sessions inside pageseeds-app |
| User Grok | `~/.grok/skills/weekly-seo-continue/SKILL.md` → same canonical (or copy) | **All** customer repos |
| User Kimi | `~/.kimi-code/skills/weekly-seo-continue/SKILL.md` | **All** repos (optional) |

Example (user Grok, all projects):

```bash
mkdir -p ~/.grok/skills/weekly-seo-continue
ln -sf /path/to/pageseeds-app/.agents/skills/weekly-seo-continue/SKILL.md \
  ~/.grok/skills/weekly-seo-continue/SKILL.md
```

---

## Alignment with weekly-seo

| Concern | weekly-seo | this continue skill |
|---------|------------|---------------------|
| Desk / mode / ≤5 growth | Full soft path | **Forbidden** unless a single open decision is that create |
| Outcome JSON | Writes pair after successful report | Rewrites `latest` (+ optional `_continue` archive) — **only** default artifacts |
| Spacing clock / `weekly_seo_*.md` | Mode-executing MD resets 5d | **Hard ban** on all `weekly_seo_*.md` writes; clock never resets |
| Human MD | `weekly_seo_{ts}.md` narrative | Prefer none; rare log only as `weekly_continue_{ts}.md` |
| Open operator leftovers | Phase 0.5 recheck inside full weekly | **Drain lane** without full weekly |
| `product_gap` | Report only | List only |
| Zero opens | Still may run desk if spacing allows | **Clean no-op** |

---

## Common failure modes

| Symptom | Cause | Correct behavior |
|---------|--------|------------------|
| “I re-ran weekly to finish leftovers” | No continue skill | Use `/weekly-seo-continue <id>` |
| Spacing / status polluted after continue | Wrote any `weekly_seo_*.md` (incl. `weekly_seo_continue_*`) | Outcome JSON only; hard-ban that glob; rare human log = `weekly_continue_*` only |
| Invented CTR/research work | Zero operators but agent freestyled | No-op message; optional waiting recheck only |
| Auto-noindex | Treated confirm as act | Escalate human; leave `open` until confirm |
| Implemented bulk noindex CLI mid-run | Expanded `product_gap` | List gap; do not eng mid-continue |
| Matched wrong project | Folder ≠ registry id | Resolve via `list-projects` **id** |

---

## Guardrails (summary)

- Drain open **`operator_act` then `operator_confirm`** only.  
- Load outcome via **`pageseeds-cli weekly-outcome`** (file fallback).  
- Budget ≤**5** exec · ≤**2** creates · prefer cancel/status/list.  
- Rewrite **`weekly_outcome_latest.json`** (+ optional `_continue` archive) + refresh **`followup_prompt`**.  
- **Hard-ban** all `weekly_seo_*.md` writes; **no** spacing reset. Prefer no human MD.  
- Zero opens → clean no-op (optional waiting expiry).  
- `product_gap` / `optional_backlog` list-only.  
- No product crate edits; no full desk; no inventing work.
