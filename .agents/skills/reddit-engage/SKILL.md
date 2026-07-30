---
name: reddit-engage
description: >-
  Standalone Reddit audience engagement pass for one PageSeeds project via
  pageseeds-cli: config check → reddit_opportunity_search → pick ≤5 posts →
  create-reddit-replies → execute reddit_reply (posts) → report. Use when the
  user wants Reddit engagement, Reddit opportunity search, community replies,
  or /reddit-engage. Operator only — never edit pageseeds-app source.
when-to-use: >-
  Triggers on "/reddit-engage", "reddit engage", "Reddit opportunity search",
  "find Reddit posts to reply to", "Reddit engagement pass", "post Reddit
  replies", "community engagement on Reddit".
argument-hint: "[project-name-or-id]"
user-invocable: true
metadata:
  short-description: "Reddit engage via pageseeds-cli (search → pick ≤5 → post)"
---

# Reddit Engage — CLI Operator Bible

> **Purpose:** Find high-fit Reddit conversations for one PageSeeds project,
> pick the best opportunities, and **post value-first replies** (≤5 per run).
>
> Extracted from the thin Reddit branch of `weekly-seo` into a dedicated pass so
> weekly SEO stays desk/GSC-focused.

## Invocation

```
/reddit-engage
/reddit-engage coffee
/reddit-engage expense
```

Prefer the **customer project** (cwd outside `pageseeds-app`). Requires
`pageseeds-cli` on PATH. Prefer the prebuilt install:

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
```

You are the Reddit engagement operator for **one** project. Do not edit product
source or hand-edit MDX for this skill.

| Layer | Role |
|-------|------|
| **Capability** | `pageseeds-cli` JSON tools |
| **Policy** | This skill — budgets, quality rails, report, isolation |
| **Agent** | You — pick posts, gate drafts, execute posts |
| **Product source** | **Out of scope** — never patch `pageseeds-app` |

---

## Separation of concerns (mandatory)

| Role | Workspace | May write |
|------|-----------|----------|
| **This skill** | Customer project / neutral cwd | Only the engage report under project automation |
| **pageseeds-cli** | N/A (binary on PATH) | Tasks/DB/replies **via tools only** |
| **Product engineer** | `pageseeds-app` (separate session) | App source / PRs |

If the session is inside the product repo (`pageseeds-app` + editing Rust/TS),
**stop** and re-run with only the customer project open.

---

## Inputs

Prefer **setup defaults** so you do not pass `-i`/`-p` on every call:

```bash
pageseeds-cli setup --path . --yes
pageseeds-cli list-projects
```

- `-i <project-id>` — optional after setup
- `-p <project-path>` — optional after setup

```bash
pageseeds-cli <tool> [args...]
# or with explicit override:
pageseeds-cli <tool> -i <project-id> -p <project-path> [args...]
```

All tools print **JSON**. Never invent opportunity counts, scores, or post IDs.

### Required project files

Under `<project-path>/.github/automation/`:

| File | Role |
|------|------|
| **`project.yaml`** | **Structured SOT** — `product_name`, strategy keywords/clusters, Reddit knobs (`mention_stance`, `seed_subreddits`, `excluded_subreddits`, `trigger_topics`, `query_keywords`) |
| `project.md` | Brand / product **prose** for enrichment (not search keyword SOT) |
| `reddit/_reply_guardrails.md` | Safety + tone constraints for drafts |

| Legacy (migrate only) | Role |
|-----------------------|------|
| `reddit_config.md` | Old Reddit knobs — **not** live SOT |
| Strategy sections in `project.md` | Old Search Keywords / Content Clusters — **not** live SOT |

**Preflight (CLI first):**

```bash
pageseeds-cli project-config-status -p <path>
# optional: structured strategy view
pageseeds-cli strategy -p <path>
```

| Status | Action |
|--------|--------|
| Valid `project.yaml` with non-empty `reddit.trigger_topics` and/or `reddit.query_keywords` | Proceed |
| `needs_migration: true` (legacy MD only) | Run dry-run then migrate (or rely on auto-migrate on first ensure). Prefer explicit: `pageseeds-cli migrate-project-config -p <path> --dry-run` then without `--dry-run`. Review YAML after. |
| Valid YAML but **empty** `trigger_topics` **and** `query_keywords` | **Stop**. Fill `project.yaml` reddit block (do **not** invent keywords). Setup writes empty defaults only. |
| No YAML and no legacy sources | **Stop**. `pageseeds-cli setup -p <path> --yes` then fill YAML. |

Do **not** require `reddit_config.md`. Do **not** invent subreddits or query keywords mid-run.

If guardrails are missing → **degrade**: still run search, but apply base
validation only and note the gap in the report.

After auto-migrate or explicit migrate: note in the report that YAML was written
from legacy MD (values are preserved as-is — fix product intent in YAML if
topics are wrong, e.g. brewing vs roasting).

### Auth / credentials

Posting requires Reddit OAuth credentials available to PageSeeds (typically
via env / secrets the desktop app and CLI already use). If
`execute-task` on `reddit_reply` fails with auth/rate-limit errors → stop
further posts, report failures, do not retry forever.

---

## Hard rails (always)

Breaking these fails the run.

| # | Rule |
|---|------|
| 1 | **CLI only** for tasks/data (+ the report file). No direct DB writes, no hand-posting outside CLI. |
| 2 | **No product source edits** under `pageseeds-app`. |
| 3 | **Missing capability → escalate**, don’t implement. |
| 4 | **Budgets:** ≤**1** new `reddit_opportunity_search` · ≤**5** `create-reddit-replies` post picks · ≤**5** `reddit_reply` executions (posts) · ≤**8** total `execute-task` calls. |
| 5 | **May-create list only:** `reddit_opportunity_search`. Reply children come **only** from `create-reddit-replies` (never bare `create-task reddit_reply`). |
| 6 | **Evidence:** every pick cites artifact fields (post_id, subreddit, score/severity, why_relevant). |
| 7 | **Draft quality gate before post** (below) — skip posts that fail; never “hope” Reddit accepts spam. |
| 8 | **Report only file write:** `reddit_engage_{YYYYMMDD_HHMMSS}.md` under `<project-path>/.github/automation/`. |
| 9 | **Missing Reddit config / API / auth → degrade and say so**; never fake opportunities or mark posts as sent. |
| 10 | **No URLs in replies.** Product validation forbids `http(s)://` and markdown links. Prefer genuine advice; mention product only per mention stance. |
| 11 | **Structured knobs only from `project.yaml`** (or CLI ensure/migrate). Never re-author search topics from free-form brand prose alone. |

### Draft quality gate (must pass before `execute-task` on `reddit_reply`)

From `get-task` description / opportunity `reply_text`:

| Check | Fail if |
|-------|---------|
| Length | &lt; ~30 words or &gt; ~250 words |
| Sentences | &lt; 3 or &gt; 5 sentences (approx) |
| Links | Contains `http://`, `https://`, or `[text](url)` |
| Value | Pure pitch / no answer to the OP’s question |
| Duplicate | Same post_id already has a reply task / posted history |
| Stance | Mention stance **REQUIRED** but product name absent |
| Guardrails | Violates project `reddit/_reply_guardrails.md` |

Failing drafts: **skip** that post (do not execute). Note in report under
**Skipped**. Do not invent a rewrite path unless the CLI later exposes
draft-edit tools — default is skip.

### Pick quality preferences

Prefer opportunities that are:

1. **High relevance / severity** (`CRITICAL` / `HIGH`, high `final_score` /
   `relevance_score` when present)
2. **Fresh and answerable** — OP asks a real question the brand can help with
3. **Draft already good** — enrichment produced a non-empty `reply_text` that
   passes the quality gate
4. **On-seed subreddits** — not excluded communities
5. **Not promotional bait** — avoid “recommend a tool” threads that only reward
   spam if the draft is thin

Reject / deprioritize:

- Empty or placeholder drafts
- Posts already replied to (check open/done `reddit_reply` tasks + history)
- Subreddits in excluded list
- Threads where only a link would help (you cannot post links)

---

## Soft guidance (default path)

```text
resolve project → recency/capacity → config preflight (project.yaml)
  → create+execute reddit_opportunity_search
  → get-task + parse reddit_results_stage
  → pick ≤5 post_ids (quality + impact)
  → create-reddit-replies
  → execute each reddit_reply one-by-one (post)
  → report
```

### A. Resolve project

```bash
pageseeds-cli list-projects
# or rely on setup defaults in cwd
```

If the user named a project (`coffee`, `expense`, …), match by `id` or `name`
and pass `-i` / `-p` explicitly.

### B. Recency / capacity

```bash
pageseeds-cli list-tasks -i <id> -p <path> -t reddit_opportunity_search
pageseeds-cli list-tasks -i <id> -p <path> -t reddit_reply
```

| Condition | Action |
|-----------|--------|
| Search in `review` with fresh opportunities and user did not force a new search | **Reuse** that task — go to pick (do not create another search) |
| Open `reddit_reply` tasks (`todo`/`queued`/`in_progress`) ≥ 5 | Prefer finishing those first; skip new search unless user forces |
| Last engage report **&lt; 2 days** ago and user did not force | Skip run; state why |
| User says “run anyway” / “force” | Continue |

### C. Config preflight

```bash
pageseeds-cli project-config-status -p <path>
```

Confirm:

1. **Format** is `yaml` (or migrate first if `legacy_md` / `needs_migration`)
2. **Reddit fuel:** non-empty `reddit.trigger_topics` and/or `reddit.query_keywords` in `project.yaml` (status `counts` or read file). Empty both → stop and ask operator to fill YAML.
3. Optional: `seed_subreddits` present (preferred; empty may still search depending on product behavior — prefer explicit seeds)
4. `reddit/_reply_guardrails.md` present if possible
5. Brand prose via `project.md` (enrichment)

Missing / empty structured Reddit knobs → stop with:

```text
Fill .github/automation/project.yaml reddit block (trigger_topics / query_keywords,
seed_subreddits, mention_stance). See docs/PROJECT_MD_STRATEGY.md.
Legacy only: pageseeds-cli migrate-project-config -p . --dry-run
```

Do **not** invent keywords or subreddits.

### D. Search

```bash
pageseeds-cli create-task -i <id> -p <path> \
  -t reddit_opportunity_search \
  -T "Reddit opportunity search" \
  -r "Standalone reddit-engage pass — find high-fit threads to reply to"

pageseeds-cli execute-task -I <search-task-id>
```

Expect status **`review`** with `RedditPicker` / `reddit_results_stage` artifact.

On failure (empty config, API, license): stop that branch; report; no fake picks.
Config parse fails loud when both query keywords and trigger topics are empty —
that is expected; fix YAML, do not invent mid-run.

### E. Review + pick

```bash
pageseeds-cli get-task -I <search-task-id>
```

Parse `reddit_results_stage` (and any opportunity fields on the task). Build a
shortlist table in your head:

| post_id | subreddit | severity/score | why_relevant | draft OK? |

Pick **≤5** post_ids that pass the quality gate. Fewer is better (1–3 often
enough). Zero good posts is a valid outcome — report and stop without posting.

```bash
pageseeds-cli create-reddit-replies -I <search-task-id> -P post1,post2,...
```

- Parent search is marked **done** by the CLI on success.
- Spawns `reddit_reply` children (CLI help may say `draft_reddit_reply` —
  actual task type is **`reddit_reply`**).

### F. Post (execute replies)

```bash
pageseeds-cli get-task -I <reply-task-id>   # optional: re-check draft
pageseeds-cli execute-task -I <reply-task-id>
```

Rules:

1. Execute **one at a time** so rate limits surface early.
2. On success → record comment/permalink if present in task result.
3. On rate limit → wait once if the tool already backs off; if still failing,
   stop remaining posts and report.
4. On validation / auth / other hard fail → skip remaining risky posts; report.
5. Never re-execute a reply task that already posted (check status / history).
6. Stay within ≤5 posts and ≤8 total executions for the whole run.

### G. Report

`<project-path>/.github/automation/reddit_engage_{YYYYMMDD_HHMMSS}.md`

```markdown
# Reddit Engage — {project name}

**Date:** {ISO timestamp}

## Summary
2–3 sentences: what was found, how many posted, any blocks.

## Config
- project.yaml present / format: yaml | legacy_md | missing
- needs_migration / auto-migrated: yes/no
- product_name / mention_stance (from YAML)
- trigger_topics / query_keywords counts (non-empty?)
- seed_subreddits / excluded counts
- guardrails present: yes/no
- project.md prose present: yes/no

## Search
| Task | Status | Notes |
| Opportunities considered | n |
| Picked | n (post_ids) |

## Posts
| post_id | subreddit | URL | Task | Outcome |

## Skipped (and why)
- …

## Needs your decision
- …

## Product / CLI gaps (if any)
- …

## Recommended next actions
- …
```

### Final user message (no JSON dumps)

```
## Reddit Engage — {project name} ({date})

**TL;DR:** …

**Config:** project.yaml … (topics/seeds OK or blocked)

**Search:** … opportunities → picked n

**Posted**
- r/… — … → outcome

**Skipped**
- …

**Needs your decision**
- …

**Report:** {path}
```

---

## Explicit bans

| Ban | Do instead |
|-----|------------|
| Bare `create-task reddit_reply` | `create-reddit-replies` from search parent |
| Posting without quality gate | Skip + report |
| URLs / markdown links in replies | Value-only prose; no links |
| Mass-replying same thread | One reply per post_id (idempotency key) |
| New search every time while review open | Reuse review-status search |
| Editing `pageseeds-app` for missing tools | Report gap |
| Inventing opportunities | Only artifact / list-tasks data |
| Full weekly SEO mid-run | Point to `/weekly-seo` |
| Requiring or editing `reddit_config.md` as live SOT | Use `project.yaml`; migrate legacy once |
| Inventing `trigger_topics` / `query_keywords` / subreddits from brand prose | Stop; operator fills YAML |

---

## Relationship to weekly-seo

| Pass | Owns |
|------|------|
| `/weekly-seo` | GSC desk, content fixes, research, indexing, interlinking |
| `/reddit-engage` | Reddit search, pick, post |

Weekly SEO may still *mention* Reddit as optional capacity; this skill is the
**dedicated** engagement loop. Prefer running them separately.

---

## Guardrails (summary)

- CLI-only operator; no product source edits.
- Preflight **`project.yaml`** via `project-config-status`; migrate if needed;
  stop if Reddit topics/keywords empty.
- Do **not** require `reddit_config.md`.
- Reuse open search in `review` when possible.
- ≤1 search create, ≤5 picks, ≤5 posts, ≤8 executions.
- Quality gate before every post; skip &gt; force.
- No URLs in replies; honor mention stance + guardrails.
- Rate-limit / auth fail → stop further posts.
- Only write the engage report file.
- Evidence required; never fake posts.

---

## Design note

Product flow (`pageseeds-app` BUSINESS_PROCESSES / project config epic #289):

```text
reddit_opportunity_search
  → reddit_config_parse (deterministic: ensure_project_config → project.yaml)
  → reddit_search → reddit_enrich → reddit_results
  → review (RedditPicker)
  → create-reddit-replies → reddit_reply → reddit_post_reply (API)
```

Structured knobs: `project.yaml`. Prose: `project.md`. Safety:
`reddit/_reply_guardrails.md`. Legacy MD is migrate-only.

This skill is the **CLI operator policy** around that pipeline: when to search,
how to pick, when to refuse a draft, and how to report — not a reimplementation
of Reddit search or OAuth posting.

Operator checklist: [docs/PROJECT_MD_STRATEGY.md](../../../docs/PROJECT_MD_STRATEGY.md)
(in pageseeds-app checkout) or the same doc on main.
