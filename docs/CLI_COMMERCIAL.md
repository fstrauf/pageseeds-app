# CLI commercial boundary (free desk vs paid operator)

> **Source of truth for free vs paid tool names.** Marketing and the code gate must match this file.
> Code enforcement: static paid set in [`crates/pageseeds-core/src/license/mod.rs`](../crates/pageseeds-core/src/license/mod.rs) (`PAID_TOOLS`), gated from [`pageseeds-cli`](../crates/pageseeds-cli/src/main.rs) (#156).
>
> **This file is the SoT for free vs paid names.** Keep `PAID_TOOLS` and the lists below in sync. Do not invent a second free/paid list in website copy.

---

## Product one-liner

**Free: see what’s going on (desk + GSC). Paid: research, write, fix, merge — the weekly operator actions.**

- **Free** = explore your own site (BYO Google keys). No content mutation.
- **Paid** = change the site / run the weekly operator loop that creates value.

---

## Policy (locked)

Owner decision 2026-07-25. Do not re-open in docs-only or gate PRs without a product change.

1. **Free desk forever** — try-before-buy desk: Site State + GSC/health reads; no content mutation.
2. **Paid starts at** Path B write/fix/merge, research that pulls/starts the pipeline, and task lifecycle that does real work.
3. **No free Path B trial for v1** — package/submit tools are paid from day one.
4. **v1 non-limits:** unlimited local projects; BYO API keys (not a license dimension).

---

## Free (no license) — 27 tools (+ meta)

Meta help/version/license stay free. Setup/list/create are match-arm free tools (no paid gate, no `-i`/`-p` required up front).

### Meta

- `--help` / `-h` / bare `help` / no args
- `license activate|status|deactivate`
- `--version` / `-V`
- `list-projects` — JSON of registered projects (same DB as desktop; no sqlite recipe)
- `create-project` — register workspace project via shared create helper (`--path`, `--name`, optional `--site-url`)
- `setup` — idempotent onboarding: link/create, write config defaults, optional license (`--license` / `PAGESEEDS_LICENSE`), first-win desk read (`--skip-first-win`, `--status`, `--yes`, `--json`)

### Desk / article reads

| Tool | Notes |
|------|--------|
| `site-overview` | Site State overview (includes `outcomes` aggregates) |
| `ctr-outcomes` | CTR closed-loop: deploy-verify + classify + report rollup (#302). May hit the **network** for live title verification when classifying ready rows. |
| `articles` | Article catalog + filters |
| `article` | Per-slug package |
| `article-list` | Lightweight article list |
| `article-frontmatter` | Frontmatter inspect |
| `article-body-hash` | Body hashes |
| `article-title-scan` | Title scan |
| `article-link-graph` | Internal link graph |
| `framework-files` | Framework / skill files |
| `video-clip-context` | Article context JSON for the `video-script` skill (local reads only) |

### GSC / health reads

| Tool | Notes |
|------|--------|
| `gsc-performance` | GSC performance (BYO Google keys) |
| `gsc-queries` | GSC queries for a page |
| `gsc-movers` | GSC movers |
| `indexing-status` | Indexing health snapshot |
| `ctr-health` | CTR health snapshot |
| `content-audit-report` | **Read existing report only** |
| `cannibalization-clusters` | **Read only** |
| `research-shortlist` | **Read existing shortlist only** |
| `article-quality-reviews` | Quality review list |
| `research-context` | **Package/inspect only — no pull** |
| `strategy` | **Read only** — structured content strategy from `project.yaml` (via ensure; auto-migrates legacy MD) as JSON |
| `project-config-status` | **Read only** — `project.yaml` vs legacy MD readiness |
| `migrate-project-config` | Deterministic MD → `project.yaml` migrator (`--dry-run` plans only) |

### Inspect

| Tool | Notes |
|------|--------|
| `list-tasks` | List tasks |
| `get-task` | Get one task |
| `validate-article` | Validate article structure |

**Free tool names (31):**  
`list-projects`, `create-project`, `setup`, `sync-site-urls`, `site-overview`, `ctr-outcomes`, `articles`, `article`, `article-list`, `article-frontmatter`, `article-body-hash`, `article-title-scan`, `article-link-graph`, `framework-files`, `video-clip-context`, `gsc-performance`, `gsc-queries`, `gsc-movers`, `indexing-status`, `ctr-health`, `content-audit-report`, `cannibalization-clusters`, `research-shortlist`, `article-quality-reviews`, `research-context`, `strategy`, `project-config-status`, `migrate-project-config`, `list-tasks`, `get-task`, `validate-article`

---

## Paid (license required) — 25 tools

### Path B package / submit

| Tool | Notes |
|------|--------|
| `write-context` | Package write context |
| `write-submit` | Submit write (catalog `draft`; does not publish) |
| `publish-content` | Explicit second step: catalog draft/ready → published (`-S` slug(s)) |
| `fix-context` | Package fix context |
| `fix-submit` | Submit fix |
| `merge-context` | Package merge context |
| `merge-submit` | Submit merge |

### Research act

| Tool | Notes |
|------|--------|
| `research-pull` | Pull research / start pipeline (not inspect-only) |
| `create-articles-from-keywords` | Spawn articles from keywords |

### Task / lifecycle act

| Tool | Notes |
|------|--------|
| `create-task` | Create task |
| `execute-task` | Execute task |
| `cancel-tasks` | Cancel tasks |
| `update-task-status` | Update task status |
| `set-task-status` | Set task status |
| `select-keywords` | Keyword selection follow-up |
| `select-content-review` | Content-review selection |
| `select-cannibalization` | Cannibalization selection |
| `create-tasks-from-approved` | Create tasks from approved items |
| `set-review-status` | Set review status |
| `create-reddit-replies` | Create Reddit reply tasks |

### Heavy / soft act

| Tool | Notes |
|------|--------|
| `run-content-audit` | Run content audit (writes report) |
| `cannibalization-strategy` | Cannibalization strategy workflow |
| `score-zero-impression-articles` | Live score = paid (DataForSEO SERP via `serp_guard`: keyword cache 14d + 50 live/day/project). **`--from-cache` / `--list` = $0** (local `article_metadata` winnability only). Remediation via existing fix/merge/link tools = $0 (no DataForSEO). Local score TTL 60d, max 25 assessments/run. |

### Misc product

| Tool | Notes |
|------|--------|
| `write-feature-spec` | Generate feature spec |
| `compare-rendered` | Compare rendered output |

**Paid tool names (25):**  
`write-context`, `write-submit`, `publish-content`, `fix-context`, `fix-submit`, `merge-context`, `merge-submit`, `research-pull`, `create-articles-from-keywords`, `create-task`, `execute-task`, `cancel-tasks`, `update-task-status`, `set-task-status`, `select-keywords`, `select-content-review`, `select-cannibalization`, `create-tasks-from-approved`, `set-review-status`, `create-reddit-replies`, `run-content-audit`, `cannibalization-strategy`, `score-zero-impression-articles`, `write-feature-spec`, `compare-rendered`

---

## Operator tier (no license, dev-machine only) — 1 tool

A third category **outside the commercial free/paid boundary**. Operator-tier
tools are neither free-desk nor paid: they are **not sold to customers**, are
not part of the single-prebuilt-binary promise, and may spawn external
subprocesses (node, ffmpeg, python — AGENTS.md §5) with PATH detection and
clear install-hint errors. They assume a **source checkout of pageseeds-app**
on a developer machine. Code: `OPERATOR_TOOLS` in
[`crates/pageseeds-core/src/license/mod.rs`](../crates/pageseeds-core/src/license/mod.rs)
(disjoint from `PAID_TOOLS`; the CLI inventory test asserts
free ∪ paid ∪ operator = all tools).

| Tool | Notes |
|------|--------|
| `video-clip-render` | Renders one clip definition via `<pageseeds-app>/video-engine/generate-clip.sh`. Requires node + ffmpeg on PATH and `video.config.json` in the project ([video clip spec](./video_clip_spec.md)). The free `video-clip-context` + embedded `video-script` skill produce the clip definition; rendering stays operator-side. |

---

## Rule of thumb (new subcommands)

| If the command… | Then |
|-----------------|------|
| Only reads local DB / files / GSC | **Free** |
| Writes MDX, redirects, or mutates content | **Paid** |
| Spawns or advances tasks that do real work | **Paid** |
| Calls paid third-party research APIs (e.g. Ahrefs pull) | **Paid** |
| Is meta / license / help / version | **Free** |
| Spawns external toolchains (node/ffmpeg/python) on a dev machine; not part of the commercial promise | **Operator tier** |

When in doubt: free = observe; paid = act or mutate.

---

## Maintenance

When adding a CLI match arm in [`crates/pageseeds-cli/src/main.rs`](../crates/pageseeds-cli/src/main.rs):

1. Update **this file** (free, paid, or operator list + counts).
2. Update `PAID_TOOLS` (paid) or `OPERATOR_TOOLS` (operator tier) in `crates/pageseeds-core/src/license/mod.rs` in the same PR.
3. Keep help `TOOLS` / `print_help` in sync with match arms.

Inventory check for implementers:

- free ∪ paid ∪ operator = all match-arm tools
- free ∩ paid = empty; operator ∩ (free ∪ paid) = empty
- Current lock: **27 free + 25 paid = 52** commercial tools (+ meta) and **1 operator-tier** tool (`video-clip-render`, outside the commercial boundary) = 53 total (verified against match arms; ignore status enum matches like `done` / `cancelled`)

---

## Cross-links

| Topic | Link |
|-------|------|
| License gate (code) | [#156](https://github.com/fstrauf/pageseeds-app/issues/156) |
| Customer getting started | [#158](https://github.com/fstrauf/pageseeds-app/issues/158) |
| Commercial CLI epic | [#154](https://github.com/fstrauf/pageseeds-app/issues/154) |
| Website offer / pricing | [fstrauf/pageseeds](https://github.com/fstrauf/pageseeds) — do **not** claim free write/fix/merge |
| Weekly operator skill | [`.agents/skills/weekly-seo/SKILL.md`](../.agents/skills/weekly-seo/SKILL.md) — desk explore free; Path B / task act paid |
| CLI binary | [`crates/pageseeds-cli/src/main.rs`](../crates/pageseeds-cli/src/main.rs) |
| Docs index | [README.md](./README.md) |

---

## Explicit contract for #156

- **SoT for free vs paid names:** this document.
- **#156** implements a static paid set that **must match** the 25 paid tools listed above (plus any later tools classified paid via the rule of thumb and updated here in the same PR).
- Free tools and meta (`--help`, `license` / `version`) must remain ungated.
- No free Path B trial in v1: `write-context`, `write-submit`, `publish-content`, `fix-context`, `fix-submit`, `merge-context`, `merge-submit` stay paid.

---

## License commands

| Command | Behavior |
|---------|----------|
| `pageseeds-cli license activate <key>` | Verify JWT (RS256 signature + claims) **before** write; persist raw token to license store |
| `pageseeds-cli license status` | JSON: `missing` / `valid` (plan, exp) / `expired` / `invalid` |
| `pageseeds-cli license deactivate` | **Local file delete only** — no phone-home |

**Store path:** `$PAGESEEDS_LICENSE_PATH` if set, else `{dirs::config_dir}/pageseeds/license.jwt` (typically `~/.config/pageseeds/license.jwt` on Linux, `~/Library/Application Support/pageseeds/license.jwt` on macOS).

**Runtime gate:** for each paid tool name, the CLI re-reads the file and re-verifies signature + `exp` + `plan` offline. No network. Expiry is the JWT `exp` claim only.

**Deny message shape (stderr, exit 1):**

```
ERROR: Paid command '<tool>' requires a valid PageSeeds license.
Activate: pageseeds-cli license activate <key>
Buy: https://pageseeds.com
```

Code: [`crates/pageseeds-core/src/license/mod.rs`](../crates/pageseeds-core/src/license/mod.rs), gate in [`pageseeds-cli`](../crates/pageseeds-cli/src/main.rs).

---

## JWT claim contract (website / mint — pageseeds#4)

| Field | Required | Notes |
|-------|----------|--------|
| Algorithm | **RS256** only | CLI rejects other algs |
| `exp` | **Required** | NumericDate (seconds). After this, license is expired |
| `plan` | **Required** | Must be exactly `"cli"` |
| `iat` | Recommended | Issued-at NumericDate |
| `sub` | Optional | Customer id / email hash / etc. |

- CLI embeds **public** PEM only: [`crates/pageseeds-core/src/license/public_key.pem`](../crates/pageseeds-core/src/license/public_key.pem).
- **Private key must never be committed** to pageseeds-app. The matching private key is held by the website license mint ([fstrauf/pageseeds](https://github.com/fstrauf/pageseeds) issue #4 / commercial backend).
- Unit tests use a **separate** RSA pair under `crates/pageseeds-core/src/license/testdata/` (`#[cfg(test)]` only).
- No phone-home, no seat checks, no desktop gate in this epic.
