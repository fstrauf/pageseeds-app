# CLI Getting Started (commercial)

Short path for operators who use **`pageseeds-cli`** on a customer project. Tools print **JSON on stdout**. Prefer the installed binary from any directory — do **not** `cargo run` from the product repo for day-to-day work.

> **Free vs paid (one line):** Free = see what’s going on (desk + GSC). Paid = research, write, fix, merge.  
> Details: [issue #155](https://github.com/fstrauf/pageseeds-app/issues/155) (commercial matrix; `docs/CLI_COMMERCIAL.md` when published).

---

## 1. Install

### Preferred (no cargo, no checkout)

macOS **Apple Silicon** prebuilt only (Darwin/arm64) today:

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
# → ~/.local/bin/pageseeds-cli
# Optional pin: VERSION=0.1.0 curl -fsSL ... | bash
```

Ensure `~/.local/bin` is on your `PATH`, then verify:

```bash
pageseeds-cli --help
```

Other platforms: prebuilt is **not** published yet. Use the contributor path below on a `pageseeds-app` checkout with Rust/cargo.

### Contributor / fallback (checkout + cargo)

```bash
# From a pageseeds-app checkout:
./scripts/install-cli.sh              # try download first; cargo if needed
FROM_SOURCE=1 ./scripts/install-cli.sh  # force cargo build
```

---

## 2. License (commercial path)

Activate a key once per machine (offline JWT store; no phone-home):

```bash
pageseeds-cli license activate <key>
pageseeds-cli license status
pageseeds-cli license deactivate
```

Paid tools (write/fix/merge, research-pull, task act, audits that write) require a valid license. Free desk/GSC/inspect tools work without one. See [CLI_COMMERCIAL.md](./CLI_COMMERCIAL.md).

Paid deny on stderr:

```text
ERROR: Paid command '<tool>' requires a valid PageSeeds license.
Activate: pageseeds-cli license activate <key>
Buy: https://pageseeds.com
```

| Link | Status |
|------|--------|
| [https://pageseeds.com](https://pageseeds.com) | Buy / product |
| [https://pageseeds.com/manage](https://pageseeds.com/manage) | Manage — mark **not live** until the portal is public |

---

## 3. Project ID + path

Every data tool needs both:

| Flag | Meaning |
|------|---------|
| `-i` / `--project-id` | Project UUID in the PageSeeds SQLite DB |
| `-p` / `--project-path` | **Absolute** path to the customer repo |

### Project registration gap (honest)

The CLI has **no** `create-project` or `list-projects`. Projects live in the **same SQLite** as the desktop app. You need a project that already exists (created via desktop / prior setup). This is **not** pure greenfield CLI onboarding.

**Discover existing projects** (macOS example):

```bash
sqlite3 ~/Library/Application\ Support/com.pageseeds.app/pageseeds.db \
  "SELECT id, name, path FROM projects"
```

Default DB location (platform data dir + `com.pageseeds.app/pageseeds.db`):

| OS | Default path |
|----|----------------|
| macOS | `~/Library/Application Support/com.pageseeds.app/pageseeds.db` |
| Linux | `~/.local/share/com.pageseeds.app/pageseeds.db` |
| Windows | `%APPDATA%\com.pageseeds.app\pageseeds.db` (via OS data dir) |

Override: set **`PAGESEEDS_DB_PATH`** to a full path to the DB file.

Do **not** hand-roll `INSERT INTO projects` recipes — use the desktop app (or an existing install) to register the project, then pass `-i` / `-p` to the CLI.

---

## 4. Secrets (BYO keys)

Precedence (first match wins):

1. `~/.config/automation/secrets.env`
2. `{repo}/.env.local`
3. `{repo}/.env`
4. Shell environment

**Minimum for GSC desk reads:**

- `GSC_SERVICE_ACCOUNT_PATH` and/or  
- `GSC_REPORT_OAUTH_CLIENT_SECRETS`

**Research / paid tooling** (when you run those paths): CAPSOLVER / DataForSEO (and related) keys as applicable to your setup.

---

## 5. First desk read (free tools)

Replace placeholders with values from step 3:

```bash
pageseeds-cli site-overview -i <project-id> -p <project-path>
pageseeds-cli articles -i <project-id> -p <project-path> -m 100 -l 20
pageseeds-cli article -i <project-id> -p <project-path> -S <slug>
pageseeds-cli gsc-queries -i <project-id> -p <project-path>
```

JSON on stdout. For the machine-oriented stdout / error contract, see [CONTRACTS.md](../CONTRACTS.md) and [issue #159](https://github.com/fstrauf/pageseeds-app/issues/159) — this guide does not re-specify the full contract.

---

## 6. Weekly operator path

**Operator bible (only source of truth for weekly policy):**  
[`.agents/skills/weekly-seo/SKILL.md`](../.agents/skills/weekly-seo/SKILL.md)

Sketch:

1. **Desk-first** — `site-overview` → `articles` / `article` / `gsc-queries` (and related GSC tools as needed).
2. **≤5 actions** — highest-impact only; evidence from tool output.
3. **Do not** nest `content_review` as the weekly strategy brain.
4. **Do not** `cargo run` from the product repo for the customer path.

### Path B write (paid)

After keyword selection has a research task + chosen keyword:

```bash
pageseeds-cli write-context -i <id> -p <path> -I <research-task-id> -K "<keyword>"
# Write full MDX to package target_file in the session
pageseeds-cli write-submit -i <id> -p <path> -f <mdx-path>
```

Full budgets, bans, Path B fix/merge, and report format: weekly-seo skill only.

---

## Troubleshooting

| Symptom | What to check |
|---------|----------------|
| `command not found: pageseeds-cli` | Install script ran? Is `~/.local/bin` on `PATH`? |
| Prebuilt install fails on Linux / Intel Mac / Windows | Prebuilt is **Darwin/arm64 only**; use checkout + `FROM_SOURCE=1` or wait for more platforms |
| `ERROR: --project-path required` (or missing project id) | Pass **both** `-i` and `-p` (absolute path) on every data tool |
| Project not found / empty desk | Wrong `-i` or DB; list projects via `sqlite3` on the default (or `PAGESEEDS_DB_PATH`) DB; confirm desktop has registered the project |
| GSC empty / auth errors | Secrets: `GSC_SERVICE_ACCOUNT_PATH` or `GSC_REPORT_OAUTH_CLIENT_SECRETS` in the secrets chain; site property configured on the project |
| Paid command requires license | `pageseeds-cli license activate <key>` then retry; buy at https://pageseeds.com |

---

## See also

| Doc / link | Role |
|------------|------|
| [weekly-seo skill](../.agents/skills/weekly-seo/SKILL.md) | Weekly operator policy (desk-first + Path B) |
| [CONTRACTS.md](../CONTRACTS.md) | Runtime / machine contracts |
| [issue #155](https://github.com/fstrauf/pageseeds-app/issues/155) | Free vs paid matrix |
| [issue #156](https://github.com/fstrauf/pageseeds-app/issues/156) | License gate (not shipped) |
| [issue #159](https://github.com/fstrauf/pageseeds-app/issues/159) | CLI machine contract follow-up |
| [Tool catalog](./TOOL_CATALOG.md) | Task types (desktop/queue oriented) |
