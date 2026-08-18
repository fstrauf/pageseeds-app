# CLI Getting Started (commercial)

Short path for operators who use **`pageseeds-cli`** on a customer project. Tools print **JSON on stdout**. Prefer the installed binary from any directory — do **not** `cargo run` from the product repo for day-to-day work.

> **Free vs paid (one line):** Free = see what’s going on (desk + GSC). Paid = research, write, fix, merge.  
> Details: [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md).

---

## Happy path (two commands)

```bash
# 1. Install (macOS Apple Silicon prebuilt; other platforms: FROM_SOURCE=1)
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
# Ensure ~/.local/bin is on PATH

# 2. Setup once in the customer project repo
cd /path/to/customer-site
pageseeds-cli setup --path . --yes
# optional: --license <key>  or  PAGESEEDS_LICENSE=…  (paid tools)
# optional: --site-url sc-domain:example.com
# optional: --skip-first-win

# 3. Desk tools — no -i/-p needed after setup
pageseeds-cli site-overview
pageseeds-cli articles -m 100 -l 20
```

`setup` is **idempotent**: re-run links the same project, refreshes defaults, does not create duplicates.

Check readiness without changing anything:

```bash
pageseeds-cli setup --status
```

### Agent-driven onboarding

An agent can drive the same path (install → setup → connect → first desk value)
using the operator **onboarding** skill (slash runbook, same layer as
`/weekly-seo`). Prefer that over re-inventing setup steps mid-session.

| Host | Typical invocation |
|------|--------------------|
| Grok / Kimi / Claude-style | `/onboarding` or `/onboarding .` (when skill is on the agent host path) |
| Free-form | “onboard this project”, “setup and connect”, “first value desk” |

**Canonical skill:** [`.agents/skills/onboarding/SKILL.md`](../.agents/skills/onboarding/SKILL.md)  
Host discovery (this monorepo): `.grok/skills/onboarding` → that directory. This
is **not** embedded in the CLI binary — it is an operator agent skill, not a
task/workflow skill.

The skill sequences existing CLI only (`setup`, `setup --status --json`,
`connect …`, `site-overview`, `gsc-performance`). It does not invent a second
free/paid tool list — see [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md).

---

## Install details

### Preferred (no cargo, no checkout)

macOS **Apple Silicon** prebuilt only (Darwin/arm64) today:

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
# → ~/.local/bin/pageseeds-cli
# Optional pin: VERSION=0.1.0 curl -fsSL ... | bash
```

### Contributor / fallback (checkout + cargo)

```bash
./scripts/cli/install-cli.sh          # monorepo: try download first; cargo if needed
FROM_SOURCE=1 ./scripts/cli/install-cli.sh  # monorepo force cargo build
```

---

## What setup does

1. **Optional license** — if `--license` or `PAGESEEDS_LICENSE` is set, activates via the existing license store. Free desk path still completes if license is omitted or fails.
2. **Link or create** a workspace project in the operator SQLite DB (shared helper; no hand-rolled SQL).
3. **Write defaults**
   - Global: `~/.config/pageseeds/config.toml` (`default_project_id`, `default_project_path`)
   - Local: `.pageseeds.yaml` in the project (`project_id`)
4. **First-win desk read** — runs free `site-overview` unless `--skip-first-win`.

Related free meta tools:

```bash
pageseeds-cli list-projects
pageseeds-cli create-project --path . --name "My Site"
```

---

## Project context resolution

After setup, data tools resolve project id/path in this order (first wins):

1. Flags: `-i` / `--project-id`, `-p` / `--project-path` (**always override**)
2. Env: `PAGESEEDS_PROJECT_ID`, `PAGESEEDS_PROJECT_PATH`
3. Local: `.pageseeds.yaml` in **cwd** (`project_id`)
4. Global: `config.toml` defaults
5. Registry fill: missing path looked up by id; missing id looked up by path

If nothing resolves:

```text
ERROR: No project context resolved. Run `pageseeds-cli setup` …
```

You never need to open SQLite by hand for the happy path.

---

## License (commercial path)

```bash
pageseeds-cli license activate <key>
pageseeds-cli license status
# or during setup:
pageseeds-cli setup --path . --yes --license <key>
```

Paid tools (write/fix/merge, research-pull, task act, audits that write) require a valid license. Free desk/GSC/inspect/setup tools work without one. See [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md).

| Link | Status |
|------|--------|
| [https://pageseeds.com](https://pageseeds.com) | Buy / product |
| [https://pageseeds.com/manage](https://pageseeds.com/manage) | Manage — mark **not live** until the portal is public |

---

## Secrets (BYO keys)

Precedence (first match wins):

1. `~/.config/automation/secrets.env`
2. `{repo}/.env.local`
3. `{repo}/.env`
4. Shell environment

**Front door:** `pageseeds-cli connect <provider>` (and `setup --status` → `providers` matrix).

| Provider | Connect | What to put in secrets.env |
|----------|---------|----------------------------|
| `gsc` | `pageseeds-cli connect gsc` (or `gsc-connect`) | `GSC_OAUTH_CLIENT_ID` then browser OAuth → writes `GSC_OAUTH_REFRESH_TOKEN` |
| `dataforseo` | `connect dataforseo --login L --password P` (verifies, then writes secrets) | `DATAFORSEO_LOGIN`, `DATAFORSEO_PASSWORD` |
| `reddit` | `connect reddit [--client-id … --client-secret …]` (browser OAuth); optional `--configure --topics a,b --subreddits x,y` | `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_REFRESH_TOKEN` (all three) |
| `llm` | `connect llm` (detection matrix); `connect llm --provider kimi\|claude\|openai\|grok\|ollama` (sets global `agent_provider`) | kimi/grok CLI on PATH, `ANTHROPIC_API_KEY` / `OPENAI_API_KEY`, or Ollama on `:11434` |
| `posthog` | `connect posthog` (stub + fix) | `POSTHOG_API_KEY` |
| `clarity` | `connect clarity` (stub + fix) | `CLARITY_API_TOKEN` |

Check readiness without mutating anything:

```bash
pageseeds-cli setup --status --json
# → providers.gsc.state, providers.reddit.fix, …
```

**Minimum for GSC desk reads** — browser connect (recommended):

1. Create a **Desktop** OAuth client in Google Cloud Console with redirect URI
   `http://127.0.0.1:8085`.
2. Put the client id in secrets:

```bash
# ~/.config/automation/secrets.env (or project .env)
GSC_OAUTH_CLIENT_ID=your-desktop-client-id.apps.googleusercontent.com
```

3. Connect:

```bash
pageseeds-cli connect gsc
# equivalent legacy alias:
pageseeds-cli gsc-connect
```

This opens Google consent (PKCE, loopback `127.0.0.1:8085`) and writes
`GSC_OAUTH_REFRESH_TOKEN` to `~/.config/automation/secrets.env`. The CLI refuses
to start OAuth if `GSC_OAUTH_CLIENT_ID` is missing.

**Advanced (CI / service account):**

- `GSC_SERVICE_ACCOUNT_PATH` or `GOOGLE_APPLICATION_CREDENTIALS` (JSON key path)
  — no OAuth client id needed for service-account auth

---

## Weekly operator path

**Operator bible:** [`.agents/skills/weekly-seo/SKILL.md`](../.agents/skills/weekly-seo/SKILL.md)

1. **Desk-first** — `site-overview` → `articles` / `article` / `gsc-queries`
2. **≤5 actions** — highest-impact only
3. Do **not** nest `content_review` as the weekly strategy brain

Path B write (paid):

```bash
pageseeds-cli write-context -I <research-task-id> -K "<keyword>"
pageseeds-cli write-submit -f <mdx-path>
# Catalog stays draft until explicit publish:
pageseeds-cli publish-content -S <slug>
```

### Operator runs (schedule + status)

Configure skill cadence and read last-run status (paid). SQLite SoT is core `operator_runs`; CLI is thin JSON I/O only.

```bash
pageseeds-cli operator-runs enable -i coffee --skill weekly_seo --every 5d
pageseeds-cli operator-runs enable -i coffee --skill reddit_engage --every 2d
pageseeds-cli operator-runs enable -i coffee --skill workspace_cmd:coffees-research --every 1d \
  --cmd 'npm run coffees:research -- --apply --limit=40'
pageseeds-cli operator-runs status -i coffee
pageseeds-cli operator-runs run -i coffee --skill weekly_seo
pageseeds-cli operator-runs tick --dry-run   # preview due skills
pageseeds-cli operator-runs install-helper   # macOS: hourly LaunchAgent for tick
```

Skills: `weekly_seo` (default 5d), `reddit_engage` (2d), `video_clip` (3d), plus named `workspace_cmd:<name>` jobs (default 1d; `--cmd` required). Cadence is `last_finished_at + interval_days` (optional `--hour` is stored but **not** used by due-eval yet). `status` is the JSON read API; `tick` + `install-helper` run unattended due schedules on macOS.

Agent skills run in a **git worktree** and ship a content PR on exit (`artifacts.pr_url`). Interactive isolate/ship: `pageseeds-cli worktree begin|ship -i <id>`. The live checkout stays clean.

**Full guide** (all commands, LaunchAgent contract, status field list, migration from agent_jobs, cutover): [OPERATOR_RUNS.md](./OPERATOR_RUNS.md).

---

## Optional: native menu bar companion

macOS only. A small **MenuBarExtra** app that **projects**  
`pageseeds-cli operator-runs status` JSON into the menu bar (poll every 60s).  
It does **not** replace the CLI: no SQLite, no second status store, no skill logic.  
The CLI alone remains complete for all operator workflows.

**Status surface:** `operator-runs status` (issue #59) is the JSON source the menu bar polls. Run / continue menu actions shell out to `operator-runs run` / `continue` and fail cleanly if those subcommands are not installed yet.

### Build & install

```bash
cd apps/operator-menubar
./install.sh
# → ~/Applications/PageSeeds Operator Menubar.app
#    + Open at Login (LaunchAgent); launches now
```

Dev loop without installing:

```bash
cd apps/operator-menubar
swift test
swift run          # menu bar while the process runs
```

### Uninstall

```bash
launchctl bootout "gui/$(id -u)/com.pageseeds.operator-menubar" 2>/dev/null || true
rm -f ~/Library/LaunchAgents/com.pageseeds.operator-menubar.plist
rm -rf ~/Applications/PageSeeds\ Operator\ Menubar.app
```

Details: [apps/operator-menubar/README.md](../apps/operator-menubar/README.md).

---

## Escape hatch (advanced)

Defaults and `list-projects` cover normal use. The app DB is shared with the operator:

| OS | Default DB path |
|----|-----------------|
| macOS | `~/Library/Application Support/com.pageseeds.app/pageseeds.db` |
| Linux | `~/.local/share/com.pageseeds.app/pageseeds.db` |
| Windows | `%APPDATA%\com.pageseeds.app\pageseeds.db` |

Override: **`PAGESEEDS_DB_PATH`**. Config override: **`PAGESEEDS_CONFIG_DIR`** / **`PAGESEEDS_CONFIG_PATH`**.

Prefer `pageseeds-cli list-projects` over raw `sqlite3` queries. Do **not** hand-roll `INSERT INTO projects`.

---

## Developer / contributor verification

Before opening operator or CLI PRs, run:

```bash
pnpm test:cli
```

That gate runs Rust tests (`test:rust` / pageseeds-core), `cargo test -p pageseeds-cli`, the task-store lifecycle check (`check:task-store`), and the CLI machine-contract smoke (`check:cli-contract` → `scripts/check-cli-contract.sh`). `pnpm test:all` is an alias of `test:cli` (no frontend/Vite/IPC gates).

Machine contract details (stdout/stderr/exit codes): [CONTRACTS.md](../CONTRACTS.md) §14.

---

## Troubleshooting

| Symptom | What to check |
|---------|----------------|
| `command not found: pageseeds-cli` | Install script ran? Is `~/.local/bin` on `PATH`? |
| Prebuilt install fails on Linux / Intel Mac / Windows | Prebuilt is **Darwin/arm64 only**; use `FROM_SOURCE=1` |
| No project context / “run setup” | `pageseeds-cli setup --path . --yes` then retry |
| Wrong project after multi-repo work | Pass `-i`/`-p`, or re-run setup in the target repo |
| GSC empty / auth errors | Secrets chain + `site_url` on the project |
| Paid command requires license | `license activate` or `setup --license <key>` |

---

## See also

| Doc / link | Role |
|------------|------|
| [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md) | Free vs paid tool names |
| [OPERATOR_RUNS.md](./OPERATOR_RUNS.md) | Operator skill cadence, status SoT, dual-status cutover |
| [CLI_RELEASE.md](./CLI_RELEASE.md) | Version bump, `cli-v*` tags, GitHub release |
| [PROJECT_MD_STRATEGY.md](./PROJECT_MD_STRATEGY.md) | Strategy in `project.yaml` (+ legacy MD migrate); `strategy` / `project-config-status` / `migrate-project-config` |
| [weekly-seo skill](../.agents/skills/weekly-seo/SKILL.md) | Weekly operator policy |
| [CONTRACTS.md](../CONTRACTS.md) | Runtime / machine contracts |
| [issue #177](https://github.com/fstrauf/pageseeds-app/issues/177) | Setup wizard |
| [issue #156](https://github.com/fstrauf/pageseeds-app/issues/156) | License gate |
