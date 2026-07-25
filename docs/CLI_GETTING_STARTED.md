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
./scripts/install-cli.sh              # try download first; cargo if needed
FROM_SOURCE=1 ./scripts/install-cli.sh  # force cargo build
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

**Minimum for GSC desk reads:**

- `GSC_SERVICE_ACCOUNT_PATH` and/or  
- `GSC_REPORT_OAUTH_CLIENT_SECRETS`

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
```

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

That gate runs Rust tests (`test:rust`), the task-store lifecycle check (`check:task-store`), and the CLI machine-contract smoke (`check:cli-contract` → `scripts/check-cli-contract.sh`). `pnpm test:all` is an alias of `test:cli` (no frontend/Vite/IPC gates).

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
| [weekly-seo skill](../.agents/skills/weekly-seo/SKILL.md) | Weekly operator policy |
| [CONTRACTS.md](../CONTRACTS.md) | Runtime / machine contracts |
| [issue #177](https://github.com/fstrauf/pageseeds-app/issues/177) | Setup wizard |
| [issue #156](https://github.com/fstrauf/pageseeds-app/issues/156) | License gate |
