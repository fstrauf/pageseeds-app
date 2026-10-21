---
name: onboarding
description: >-
  Drive pageseeds-cli setup + connect to first desk value.
  Use when onboarding, first setup, connect providers, /onboarding.
when-to-use: >-
  "/onboarding", "onboard this project", "setup and connect",
  "first value desk", "connect GSC and LLM".
argument-hint: "[project-path]"
user-invocable: true
metadata:
  short-description: "Setup + connect providers to first desk value"
---

# Onboarding — setup + connect to first desk value

Drive a clean machine and customer project from **install → setup → connect → first
desk value** using `pageseeds-cli` only. Use when onboarding, first setup, connect
providers, first value desk, or `/onboarding`.

## Invocation

```
/onboarding
/onboarding .
/onboarding /path/to/customer-site
```

Prefer the **customer project** as cwd (not the product monorepo). Requires
`pageseeds-cli` on PATH. Prefer the prebuilt install (never `cargo run` from the
product repo for customer work):

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
# Ensure ~/.local/bin is on PATH
```

| Layer | Role |
|-------|------|
| **Capability** | `pageseeds-cli` (setup, connect, desk JSON tools) |
| **Policy** | This skill — sequence, budgets, stop rails, report |
| **Agent** | You — run commands, prompt user for consents/keys only |
| **Product source** | **Out of scope** — never patch `pageseeds-app` / monorepo |

This skill **is** the orchestrator. There is no Rust wizard mode and no interactive
CLI TUI. Do not invent a second commercial free/paid list — free desk/GSC path
works without a license; paid tools need a key (product docs / `CLI_COMMERCIAL`).

---

## Hard rails (always)

Breaking these fails the run.

| # | Rule |
|---|------|
| 1 | **Installed CLI only** — prefer `pageseeds-cli` on PATH. Never `cargo run` from the product repo for customer onboarding. |
| 2 | **Max one connect attempt per provider per pass.** Re-check status once after each connect. |
| 3 | **Auth failure → report and stop** that provider branch (and the whole pass if the failure is hard/unrecoverable). No retry loops. |
| 4 | **Never ask for the license key twice.** If the user pastes a key once, use it; if they decline, continue free path. |
| 5 | **Never invent metrics** from desk tools. Summarize only numbers present in JSON. Empty/stale/missing → say so. |
| 6 | **Never write `project.yaml` content** beyond reddit `connect reddit --configure` flags the user supplied. Do not invent keywords, subreddits, or strategy. |
| 7 | **Provider order (this skill):** `gsc` → `llm` → `dataforseo` → `reddit`. Skip `posthog` / `clarity` (stubs / optional later). |
| 8 | **Out of scope this pass:** video/YouTube/Instagram, PostHog/Clarity deep setup, the SEO run (`/pageseeds-run`), content write/fix. |
| 9 | **No product source edits.** Missing capability → report gap; do not implement mid-run. |

---

## Pass steps

### 1. Ensure CLI on PATH

```bash
command -v pageseeds-cli || which pageseeds-cli
pageseeds-cli --help >/dev/null 2>&1
```

If missing, install (customer):

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
# Ensure ~/.local/bin is on PATH, then re-check
```

Prefer the installed binary. Stop with a clear install note if the binary still
cannot run.

### 2. Setup project

Work in the customer project path (argument or `.`).

```bash
cd <project-path>   # if needed
pageseeds-cli setup --path . --yes
```

**License:** pass `--license <key>` **only if** the user already pasted a key (or
`PAGESEEDS_LICENSE` is set). Do **not** prompt twice. Free desk path completes
without a license.

Optional (only if user provided them):

- `--site-url sc-domain:example.com` (or property URL)
- `--skip-first-win` if you will run desk yourself in step 5

`setup` is idempotent: re-run links the same project and refreshes defaults.

### 3. Read readiness matrix

```bash
pageseeds-cli setup --status --json
```

Read `providers` (object keyed by provider name). Each entry has at least:

| Field | Meaning |
|-------|---------|
| `state` | `connected` \| `missing` \| `unverified` |
| `fix` | Command string the agent can run |
| `missing_keys` | Optional list of secret names |
| `detail` / `method` | Optional human/machine hints |

Also note project link / desk readiness fields if present. Do not invent status.

### 4. Connect non-connected providers (order fixed)

For each of **gsc → llm → dataforseo → reddit**:

1. If `state` is `connected` → **skip**.
2. If `missing` or `unverified` → tell the user the **one** action needed (consent
   click / paste key / choose LLM provider / DataForSEO login), then run **one**
   matching connect command (prefer matrix `fix` when it is a full command;
   otherwise the table below).
3. Re-run `pageseeds-cli setup --status --json` **once** for that provider path.
4. On auth / hard failure → **report and stop** further connects (no retries).
   Optional providers the user declined can be marked skipped instead of failing
   the whole pass.

#### Connect command reference

| Provider | Command | User action |
|----------|---------|-------------|
| **gsc** | `pageseeds-cli connect gsc` | Needs `GSC_OAUTH_CLIENT_ID` in secrets first (`~/.config/automation/secrets.env` or project `.env` / `.env.local`). Then browser OAuth consent. |
| **llm** | `pageseeds-cli connect llm` then `pageseeds-cli connect llm --provider kimi\|claude\|openai\|grok\|ollama` | Matrix shows what is available; user chooses provider. |
| **dataforseo** | `pageseeds-cli connect dataforseo --login L --password P` | User pastes login + password once. |
| **reddit** | `pageseeds-cli connect reddit` | Browser OAuth (script app). Optional configure only with user answers (step 6). |

**GSC minimum (when connecting):**

1. Google Cloud **Desktop** OAuth client with redirect `http://127.0.0.1:8085`.
2. Put client id in secrets:

```bash
# ~/.config/automation/secrets.env (or project .env)
GSC_OAUTH_CLIENT_ID=your-desktop-client-id.apps.googleusercontent.com
```

3. `pageseeds-cli connect gsc` — writes `GSC_OAUTH_REFRESH_TOKEN` on success.

**LLM:** run `connect llm` for the detection matrix, show the user what is available,
then set one provider they choose. Do not pick a paid/cloud provider without user
intent if a free local option is what they want.

**DataForSEO / Reddit:** skip cleanly if the user has no credentials yet — note
under Skipped. Do not invent credentials.

**PostHog / Clarity:** out of scope for this skill. If matrix shows them
missing, one line under Later optional is enough.

### 5. First value (desk)

After setup (and GSC connect when possible), run free desk tools and summarize
in plain language. **Never invent numbers.**

```bash
pageseeds-cli site-overview
pageseeds-cli gsc-performance
```

Summarize only fields present in JSON, for example:

- Catalog / site readiness (project linked, article counts when present)
- GSC window totals (clicks, impressions, CTR) when GSC is connected
- Top pages or movers when present
- Freshness / degraded reasons when present

If GSC is not connected: still report catalog/setup outcome; state that GSC desk
is unavailable and what one step remains (`connect gsc` + client id).

### 6. Optional Reddit configure

Only if Reddit is connected **and** the user gave short answers for topics and/or
subreddits:

```bash
pageseeds-cli connect reddit --configure --topics a,b --subreddits x,y
```

**Never invent keywords or subreddits.** If the user did not provide them, skip
configure and note that `/reddit-engage` later needs `project.yaml` reddit knobs
filled by the operator.

### 7. Close with a written summary

End every pass with a short report (chat is enough; no required file write):

```markdown
## Onboarding — {project name or path}

**CLI:** installed / missing
**Setup:** linked project id/path (or error)
**License:** activated | skipped (free path) | failed (once; not re-asked)

### Providers
| Provider | State | Action this pass |
| gsc | connected \| missing \| skipped | … |
| llm | … | … |
| dataforseo | … | … |
| reddit | … | … |
| posthog / clarity | (not this skill) | later optional |

### First value (desk)
- site-overview: … (only real numbers)
- gsc-performance: … or “GSC not connected”

### Skipped / later
- …
### Needs from you
- … (one concrete next action per open item)
```

---

## Soft guidance (default path)

```text
ensure CLI on PATH
  → setup --path . --yes [--license only if pasted]
  → setup --status --json
  → connect gsc if needed (once)
  → connect llm matrix + --provider if needed (once)
  → connect dataforseo if user has credentials (once)
  → connect reddit if user wants it (once)
  → optional reddit --configure from user answers only
  → site-overview + gsc-performance
  → written summary
```

Degrade gracefully: user can stop after setup + GSC + desk and still get first
value. Paid research/write is not required for a successful onboarding pass.

---

## Explicit bans

| Ban | Do instead |
|-----|------------|
| `cargo run` / monorepo binary for customer work | Installed `pageseeds-cli` / public install script |
| Retry loops on OAuth/auth failure | Report + stop; one attempt per provider |
| Asking for license key twice | Use pasted key once or free path |
| Inventing desk metrics | Quote tool JSON only |
| Inventing reddit topics/subreddits | Skip configure; ask user later |
| Hand-editing `project.yaml` strategy | Only reddit `--configure` flags from user |
| Connecting posthog/clarity deep setup | Mention as later optional |
| SEO run / write / fix mid onboarding | Point to `/pageseeds-run` after first value |
| Building a Rust onboarding orchestrator | This skill sequences existing CLI |

---

## Guardrails (summary)

- Install → setup → status matrix → connect in order **gsc → llm → dataforseo → reddit**.
- One connect attempt per provider; re-status once; auth fail stops retries.
- License optional; never re-prompt.
- First value = `site-overview` + `gsc-performance` with honest numbers only.
- Reddit configure only from user-supplied topics/subreddits.
- No product source edits; no new CLI wizard; this skill is the runbook.
- Close with connected / skipped / later and one clear next step for the user.
