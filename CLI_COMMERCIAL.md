# CLI commercial boundary (free desk vs paid operator)

> **Source of truth for free vs paid tool names.** Marketing and the code gate must match this file.
> Code enforcement: static paid set in [`crates/pageseeds-core/src/license/mod.rs`](../crates/pageseeds-core/src/license/mod.rs) (`PAID_TOOLS`), gated from [`pageseeds-cli`](../crates/pageseeds-cli/src/main.rs) (#156).
>
> **This file is the SoT for free vs paid names.** Keep `PAID_TOOLS` and the lists below in sync. Do not invent a second free/paid list in website copy.

---

## Product one-liner

**Free: see what’s going on (desk + GSC). Paid: research, write, fix, merge — the SEO run and the other operator actions.**

- **Free** = explore your own site (BYO Google keys). No content mutation.
- **Paid** = change the site / run the SEO loop (`pageseeds run`) that creates value.

---

## Policy (locked)

Owner decision 2026-07-25. Do not re-open in docs-only or gate PRs without a product change.

1. **Free desk forever** — try-before-buy desk: Site State + GSC/health reads; no content mutation.
2. **Paid starts at** Path B write/fix/merge, research that pulls/starts the pipeline, and task lifecycle that does real work.
3. **One free weekly pass per machine** (owner decision 2026-10-09, #399; supersedes "no free Path B trial"). A machine without a valid license may run the paid tools from its first paid command until the first pull request PageSeeds opens, or 7 days, whichever comes first. After that every tool in the paid list requires a license exactly as before. The paid list itself does not change, and the 14-day money-back guarantee stays.
4. **v1 non-limits:** unlimited local projects; BYO API keys (not a license dimension).

---

## Free (no license)

Meta help/version/license stay free. Setup/list/create/delete/gsc-connect/connect/sync-site-urls are match-arm free tools (no paid gate, no `-i`/`-p` required up front).

### Meta

- `--help` / `-h` / bare `help` / no args
- `license activate|status|deactivate`
- `--version` / `-V`
- `list-projects` — JSON of registered projects (same DB as desktop; no sqlite recipe)
- `create-project` — register workspace project via shared create helper (`--path`, `--name`, optional `--site-url`)
- `delete-project` — delete a project and all of its DB rows; requires `--yes`; files on disk are not touched (`-i <project-id>`)
- `setup` — idempotent onboarding: link/create, write config defaults, optional license (`--license` / `PAGESEEDS_LICENSE`), first-win desk read (`--skip-first-win`, `--status`, `--yes`, `--json`, optional `--connect-gsc`)
- `gsc-connect` — browser PKCE OAuth for Search Console; saves `GSC_OAUTH_REFRESH_TOKEN` to `~/.config/automation/secrets.env` (no GCP service-account chore)
- `connect` — unified provider connect front door (`gsc` / `reddit` browser OAuth; `dataforseo --login/--password` verify+write secrets; `llm` detection matrix / `--provider` sets global `agent_provider`; `posthog` / `clarity` / `bing` structured stubs with fix hints). Also powers `setup --status` `providers` matrix
- `sync-site-urls` — one-off site_url consolidation into the projects table

### Desk / article reads

| Tool | Notes |
|------|--------|
| `site-overview` | Site State overview |
| `articles` | Article catalog + filters |
| `article` | Per-slug package |
| `article-list` | Lightweight article list |
| `article-frontmatter` | Frontmatter inspect |
| `article-body-hash` | Body hashes |
| `article-title-scan` | Title scan |
| `article-link-graph` | Internal link graph |
| `framework-files` | Framework / skill files |
| `video-clip-context` | Article context JSON for the `video-script` skill (local reads only) |
| `youtube-analytics` | Read stored YouTube performance snapshots (the operator tick auto-refreshes clips whose newest snapshot is stale > 7 days); use `--refresh` for an explicit Analytics API fetch |
| `video-clip-list` | Clip definition ledger (SQLite; hydrates leftover `video/clips/*.json`) |

### GSC / health reads

| Tool | Notes |
|------|--------|
| `gsc-performance` | GSC performance (BYO Google keys) |
| `gsc-queries` | GSC queries for a page |
| `gsc-movers` | GSC movers |
| `gsc-daily` | Daily GSC site totals + site-drop evaluation (`--refresh`, `--through`, `--ack`) |
| `bing-daily` | Weekly Google + Bing clicks/impressions with WoW deltas from the stored daily tapes (`--refresh`, `-w`; BYO `BING_WEBMASTER_API_KEY`) |
| `traffic-channels` | Weekly PostHog session split (google / bing_family / ai / direct / other) with engaged counts, excluded bot sessions and suspect clusters from the stored tape (`--refresh`, `-w`; BYO `POSTHOG_API_KEY` + `posthog_project_id`) |
| `bing-performance` | Bing page performance, last 90d (BYO `BING_WEBMASTER_API_KEY`) |
| `bing-queries` | Bing queries, site-wide or for a page |
| `bing-url-info` | Bing crawl/index facts + clicks for one URL |
| `bing-crawl-issues` | URLs Bing flagged during crawl |
| `indexing-status` | Indexing health snapshot |
| `source-gate` | **Read only** — `source-gate report [-i <id>]`: dry-run source gate (#486) per project: writes / pass / would_refuse, reason counts, runs with nothing to publish, each declared source with the articles that used it (`used_by`) (SQLite `source_gate_log`). The paid `write-context` takes `--source <id>` to return a declared source's payload. |
| `ai-visibility-audit` | **Report only** — robots.txt AI agent matrix, raw-HTML main-content / client-shell sample, llms.txt presence, IndexNow key-file check; writes `.github/automation/ai_visibility.json` |
| `site-health-audit` | **Report only** — host variants, sitemap URL responses (redirect / non-200 / noindex / non-self canonical), rendered title length, in-content inbound links, URL-inspection freshness; writes `.github/automation/site_health.json` |
| `article-quality-reviews` | Quality review list |
| `strategy` | **Read only** — structured content strategy from `project.yaml` (via ensure; auto-migrates legacy MD) as JSON |
| `project-config-status` | **Read only** — `project.yaml` vs legacy MD readiness |
| `competitor-catalog-status` | **Read only** — `competitors.yaml` present / valid / `gap_ready` (no network) |
| `migrate-project-config` | Deterministic MD → `project.yaml` migrator (`--dry-run` plans only) |

### Search engine hygiene

Owner decision 2026-09-23: notifying Bing about the user's own URLs/sitemap is basic hygiene (same class as `connect`), not a content mutation — **free**.

| Tool | Notes |
|------|--------|
| `bing-submit-urls` | Submit URLs to Bing (`SubmitUrlBatch`; default = sitemap URLs with `lastmod` in the last 7 days; `--dry-run`). Checks daily quota first. |
| `bing-submit-sitemap` | Register the project sitemap with Bing (`SubmitFeed`) |
| `indexnow-submit` | Notify IndexNow engines (Yandex, Naver, Seznam, Yep) of the same URL set (one POST to `api.indexnow.org`; `--dry-run`). Needs public `indexnow_key` in `project.yaml` and `/<key>.txt` served; refuses to POST unless the key file checks out. |

### Inspect

| Tool | Notes |
|------|--------|
| `list-tasks` | List tasks |
| `get-task` | Get one task |
| `validate-article` | Validate article structure |

**Free tool names (45):**  
`list-projects`, `create-project`, `delete-project`, `setup`, `gsc-connect`, `connect`, `sync-site-urls`, `site-overview`, `articles`, `article`, `article-list`, `article-frontmatter`, `article-body-hash`, `article-title-scan`, `article-link-graph`, `framework-files`, `video-clip-context`, `youtube-analytics`, `video-clip-list`, `gsc-performance`, `gsc-queries`, `gsc-movers`, `gsc-daily`, `bing-daily`, `traffic-channels`, `bing-performance`, `bing-queries`, `bing-url-info`, `bing-crawl-issues`, `bing-submit-urls`, `bing-submit-sitemap`, `indexnow-submit`, `indexing-status`, `source-gate`, `ai-visibility-audit`, `site-health-audit`, `article-quality-reviews`, `strategy`, `project-config-status`, `competitor-catalog-status`, `migrate-project-config`, `list-tasks`, `get-task`, `validate-article`, `verify-redirects`

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
| `merge-submit` | Submit merge (refused unless the `merge_evidence` GSC-query gate passes; #541) |

### Research act

| Tool | Notes |
|------|--------|
| `keyword-explore` | Exploratory opportunity scout: seed discovery + optional live SERP deep pass; **report only** (no task spawn). Spends DataForSEO credits; `--gap` adds up to 6 Labs ranked-keywords requests (5 competitors + own domain). Same paid tool, no new `PAID_TOOLS` entry. |
| `page-fetch` | Research assist: explicit HTTPS URL → bounded HTML main-text excerpt + fetch metadata as JSON (**report only**; no matrix write, no competitors.yaml mutation). Third-party page fetch for matrix facts (`source_url` + `as_of`). |
| `reddit-search` | Read-only Reddit discovery (`q` + optional subreddit/sort/time/limit) → JSON posts. Report only; no task spawn, no `reddit_opportunities` write. Third-party research, same class as `page-fetch`. **Not** the reply/engage loop (`create-reddit-replies` / `reddit_opportunity_search`). |
| `reddit-posted-brief` | Posted-reply brief over local `reddit_opportunities` (`reply_status='posted'`): named vs silent split, deterministic promise-phrase counts vs `project.yaml` strategy, stance-drift flag. Local reads only — no Reddit call, no LLM; writes `reddit_posted_brief_*.json` sidecars (#92). Report only. |
| `reddit-listen` | Ranked-questions listen report over a subreddit: multi-query corpus (≤6 seed queries, ≤50 unique posts via desk search) + read-only top-level comments on the ≤25 highest-signal posts → per-post comprehension extract + one cluster pass → ranked `questions[]` with `sample_post_ids` evidence. Report only; no task spawn, no `reddit_opportunities` write, no replies. Third-party research, same class as `reddit-search`. **Not** the reply/engage loop. |

### Task / lifecycle act

| Tool | Notes |
|------|--------|
| `create-task` | Create task |
| `execute-task` | Execute task |
| `cancel-tasks` | Cancel tasks |
| `update-task-status` | Update task status |
| `set-task-status` | Set task status |
| `create-reddit-replies` | Create Reddit reply tasks |

### Misc product

| Tool | Notes |
|------|--------|
| `write-feature-spec` | Generate feature spec |
| `compare-rendered` | Compare rendered output |
| `reindex-article-evidence` | Fill missing `article_evidence` embedding vectors (semantic recall layer for write context + cannibalization; batch-embeds via local Ollama, **no-op `embedded: 0` when Ollama is down**). Mutates derived DB state. |
| `operator-runs` | Operator skill schedule, status, skill runner, and unattended tick (`list-skills` / `list` / `enable` / `disable` / `status` / `run` / `continue` / `tick` / `install-helper` / `uninstall-helper`). Also `workspace_cmd:<name>` project commands (not a new tool). |
| `worktree` | Isolate agent writes in a git worktree and ship a content PR (`begin` / `ship` / `abort` / `status`). Scheduled `operator-runs` begins/ships automatically. |
| `run` | v2 SEO loop, read-only for now: `run --dry-run -i <id> [--format json\|md]` (refresh, stock-take, cached research, quota worklist, scoreboard; hard error on a failed, stale or gappy GSC tape) and `run migrate-config` (builds `.github/automation/run.yaml`). Orchestrates paid capabilities like `operator-runs`. See `docs/RUN.md`. |
| `program` | Program ledger: record bets, measures, per-run entries and metric snapshots (`plan open`, `plan close`, `bet open`, `bet grade` / `entry add` / `snapshot add` / `show`; JSON payload via `--file` or `--json`; read views `brief -i <id> [--as-of]`, `status [--as-of]` across all projects, `export -i <id>` → `.github/automation/program_ledger.md`; `timeline -i <id> --slug <s>` read-only per-page actions, verdicts, linked bets and weekly metrics). SQLite SoT, see `CONTRACTS.md` §12.2. |

**Paid tool names (25):**  
`write-context`, `write-submit`, `publish-content`, `fix-context`, `fix-submit`, `merge-context`, `merge-submit`, `keyword-explore`, `page-fetch`, `reddit-search`, `reddit-posted-brief`, `reddit-listen`, `create-task`, `execute-task`, `cancel-tasks`, `update-task-status`, `set-task-status`, `create-reddit-replies`, `write-feature-spec`, `compare-rendered`, `reindex-article-evidence`, `operator-runs`, `worktree`, `program`, `run`

---

## Operator tier (no license, dev-machine only) — 3 tools

A third category **outside the commercial free/paid boundary**. Operator-tier
tools are neither free-desk nor paid: they are **not sold to customers**, are
not part of the single-prebuilt-binary promise, and may spawn external
subprocesses (node, ffmpeg, python — AGENTS.md §5) with PATH detection and
clear install-hint errors. They assume a **source checkout of this monorepo**
on a developer machine. Code: `OPERATOR_TOOLS` in
[`crates/pageseeds-core/src/license/mod.rs`](../crates/pageseeds-core/src/license/mod.rs)
(disjoint from `PAID_TOOLS`; the CLI inventory test asserts
free ∪ paid ∪ operator = all tools).

| Tool | Notes |
|------|--------|
| `video-clip-render` | Renders one clip definition via `<monorepo>/video-engine/generate-clip.sh`. Requires node + ffmpeg on PATH and `video.config.json` in the project ([video clip spec](./video_clip_spec.md)). Prefer `--slug` (loads SQLite + working file); `--clip <path>` still works. |
| `video-clip-save` | Upsert schema-v1 clip JSON into app SQLite and materialize `~/01_code/video-clip-backup/<project_id>/clips/<slug>.json`. `--json` to create; `-S` to re-ingest `publish.py` write-back. |
| `video-clip-ship-embed` | Worktree PR of **one** content MDX embed (`git commit --no-verify`, no `pnpm test`). Cleans the worktree and restores the live file. |

---

## Rule of thumb (new subcommands)

| If the command… | Then |
|-----------------|------|
| Only reads local DB / files / GSC / Bing | **Free** |
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
- Current lock: **45 free + 25 paid + 3 operator** = **73** match-arm tools (`TOOLS.len()` in `pageseeds-cli`; free includes meta `list-projects` / `create-project` / `delete-project` / `setup` / `gsc-connect` / `connect` / `sync-site-urls`)

---

## Cross-links

| Topic | Link |
|-------|------|
| License gate (code) | [#156](https://github.com/fstrauf/pageseeds-app/issues/156) |
| Customer getting started | [#158](https://github.com/fstrauf/pageseeds-app/issues/158) |
| Commercial CLI epic | [#154](https://github.com/fstrauf/pageseeds-app/issues/154) |
| Website offer / pricing | [fstrauf/pageseeds](https://github.com/fstrauf/pageseeds) — do **not** claim free write/fix/merge |
| Run skill | [`.agents/skills/pageseeds-run/SKILL.md`](../.agents/skills/pageseeds-run/SKILL.md) — desk explore free; Path B / task act paid |
| CLI binary | [`crates/pageseeds-cli/src/main.rs`](../crates/pageseeds-cli/src/main.rs) |
| Docs index | [README.md](./README.md) |

---

## Explicit contract for #156

- **SoT for free vs paid names:** this document.
- **#156** implements a static paid set that **must match** the 26 paid tools listed above (plus any later tools classified paid via the rule of thumb and updated here in the same PR).
- Free tools and meta (`--help`, `license` / `version`) must remain ungated.
- The Path B tools (`write-context`, `write-submit`, `publish-content`, `fix-context`, `fix-submit`, `merge-context`, `merge-submit`) stay in the paid set. The one free weekly pass (policy item 3) is a runtime allowance in the gate, not a free tool list.

---

## License commands

| Command | Behavior |
|---------|----------|
| `pageseeds-cli license activate <key>` | Verify JWT (RS256 signature + claims) **before** write; persist raw token to license store |
| `pageseeds-cli license status` | JSON: `missing` / `valid` (plan, exp) / `expired` / `invalid`, plus `free_pass`: `{ state: unused / active / used / expired, started_at?, expires_at?, pr_url?, project_id? }` |
| `pageseeds-cli license deactivate` | **Local file delete only** — no phone-home |

**Store path:** `$PAGESEEDS_LICENSE_PATH` if set, else `{dirs::config_dir}/pageseeds/license.jwt` (typically `~/.config/pageseeds/license.jwt` on Linux, `~/Library/Application Support/pageseeds/license.jwt` on macOS).

**Runtime gate:** for each paid tool name, the CLI calls `license::authorize` in core, which re-reads the file and re-verifies signature + `exp` + `plan` offline. No network. Expiry is the JWT `exp` claim only. A valid license is allowed immediately and never reads or writes the free-pass file.

**Free first-run pass (#400):** with no valid license (missing, expired or invalid), `authorize` falls back to a per-machine pass stored in `free_pass.json` in the same directory as the license file (the parent of `$PAGESEEDS_LICENSE_PATH`). The first paid command creates it (`started_at`); every paid tool is then allowed, with no output on success. The pass ends when `git_worktree::ship_session` returns a pull request URL (interactive `worktree ship` and `operator-runs` both go through it), recording `ended_at`, `pr_url` and `project_id`, or when 7 days (`FREE_PASS_WINDOW_DAYS`) pass since `started_at`. A skipped ship, a failed push or a missing `gh` do not end it either; only an opened pull request uses it up. An unreadable or uncreatable pass file is treated as used.

**What the pass does and does not prevent:** the state is a plain file on the user's own disk. Deleting `free_pass.json`, pointing `PAGESEEDS_LICENSE_PATH` elsewhere, using another OS user account or changing the clock gives a new pass, and nothing detects it: verification stays offline, and the 14-day guarantee covers the rest. Shipping by hand instead of `worktree ship` keeps the pass open, but only until day 7. After the pass is used, an installed `operator-runs` scheduler tick fails with the deny message like any unlicensed machine.

**Issuance (monthly and yearly):** a key is valid to the paid period end + 7 days. The first key is shown on `/success` and emailed; every later paid invoice (each month or year) emails a fresh key, which the customer activates with `license activate`. Claims are identical for both intervals.

**Deny message shape (stderr, exit 1):**

```
ERROR: Paid command '<tool>' requires a valid PageSeeds license.
[license expired | license invalid (<reason>)]
Your free weekly pass on this machine is used: it opened <pr_url> for project '<project_id>' on <YYYY-MM-DD>.
Activate: pageseeds-cli license activate <key>
Buy: https://pageseeds.com
```

The pass line is, depending on state: "is used: it opened …", "started on <date> and ended after 7 days without opening a pull request", or a note naming the unreadable/uncreatable `free_pass.json` path. The optional reason line appears only when a stored license has lapsed.

Code: [`crates/pageseeds-core/src/license/`](../crates/pageseeds-core/src/license/) (`authorize`, `free_pass.rs`); the CLI gate in [`pageseeds-cli`](../crates/pageseeds-cli/src/main.rs) only prints the returned text. End-to-end proof: `cargo test -p pageseeds-cli --test free_pass -- --nocapture`.

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
- **Private key must never be committed** anywhere in this monorepo. Host secrets only (`PAGESEEDS_LICENSE_PRIVATE_KEY_PEM` on Vercel). Public PEM is embedded in the CLI crate for offline verify.
- Unit tests use a **separate** RSA pair under `crates/pageseeds-core/src/license/testdata/` (`#[cfg(test)]` only).
- No phone-home, no seat checks, no desktop gate in this epic.
