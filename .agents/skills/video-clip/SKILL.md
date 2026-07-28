---
name: video-clip
description: >-
  Produce one vertical short (40–50s MP4) from a PageSeeds article via
  pageseeds-cli: video-clip-context → video-script craft → video-clip-render →
  quality gate → packaging report → default YouTube + Instagram publish.
  Use when the user wants a video clip, short from an article, YouTube Short /
  Reels / TikTok package, or /video-clip. Operator only — never edit
  pageseeds-app source.
when-to-use: >-
  Triggers on "/video-clip", "video clip", "make a short from this article",
  "render video clip", "article to short", "YouTube Shorts from blog",
  "vertical video for this post", "publish short to Instagram".
argument-hint: "[project-name-or-id] [slug]"
user-invocable: true
metadata:
  short-description: "Article → clip → render → YT + IG publish (one short)"
---

# Video Clip — CLI Operator Bible

> **Purpose:** One verified vertical short from one article on a project that
> already has `video.config.json`. Policy lives here; craft lives in the
> embedded `video-script` skill; capability is `pageseeds-cli`.

**Canonical path:** edit only `.agents/skills/video-clip/SKILL.md`.  
`.grok/skills/video-clip/SKILL.md` is a **symlink** for Grok discovery — do not
edit the symlink target path as a second copy.

## Invocation

```
/video-clip
/video-clip <slug>
/video-clip <project> <slug>
```

Prefer the **customer project** (cwd outside `pageseeds-app`). Requires
`pageseeds-cli` on PATH. Prefer the prebuilt install:

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
```

Operator-tier **render** additionally needs a **pageseeds-app source checkout**
with `video-engine/` plus Node + FFmpeg on PATH (see
`docs/CLI_COMMERCIAL.md` Operator tier). Context reads work on free desk tools
without that.

| Layer | Role |
|-------|------|
| **Capability** | `pageseeds-cli video-clip-context` (free desk read), `pageseeds-cli video-clip-render` (operator tier) |
| **Craft** | Embedded `video-script` skill (schema v1 clip definition) — load/reference; do **not** duplicate long craft rules |
| **Policy** | This skill — rails, path conventions, quality gate, report |
| **Config** | Target repo `video.config.json` + `video/clips/` (outputs are central — see below) |
| **Product source** | **Out of scope** — never patch `pageseeds-app` mid-run |

---

## Separation of concerns (mandatory)

| Role | Workspace | May write |
|------|-----------|-----------|
| **This skill** | Customer project / neutral cwd | Clip JSON under `video/clips/`, optional short automation report; **after publish**, source MDX **body** embed at `source.content_path` (customer content only) |
| **pageseeds-cli** | N/A (binary on PATH) | Context JSON stdout; render via operator-tier engine |
| **video-engine** | pageseeds-app checkout | Outputs to central `~/01_code/video-clip-backup/<project_id>/` (never the repo); successful publish writes `published.youtube` / `published.tiktok` / `published.instagram` back into the clip JSON |
| **Product engineer** | `pageseeds-app` (separate session) | App source / PRs / new `ui_targets` |

If the session is inside the product repo *to implement features*, stop this
skill run. Operator render may *invoke* `video-engine` from a checkout; it must
not *edit* product crates for a shot.

---

## Inputs

Prefer **setup defaults**:

```bash
pageseeds-cli setup --path . --yes
pageseeds-cli list-projects
```

- `-i <project-id>` — optional after setup
- `-p <project-path>` — optional after setup; **required** when the registered
  path lacks `video.config.json` but a video worktree does (e.g. days_to_expiry
  packaged proof)

```bash
pageseeds-cli video-clip-context -i <id> -p <path> -S <slug>
pageseeds-cli video-clip-render -p <path> --clip video/clips/<slug>.json
```

All tools print **JSON** on success. Never invent paths, durations, or packaging.

### Required project files

| Path | Role |
|------|------|
| `video.config.json` | Engine config: `base_url`, `dev_servers[]`, `ui_targets`, brand |
| `video/clips/<slug>.json` | Clip definition (schema v1) — you write this |
| `~/01_code/video-clip-backup/<project_id>/` | Render outputs (engine-owned, central — not the repo) |

Spec: `docs/video_clip_spec.md`. Engine README: `video-engine/README.md` in the
pageseeds-app checkout.

If `video.config.json` is missing → **stop**. Report the gap with links to the
spec / engine README. **Never invent config.**

---

## Hard rails (always)

Breaking these fails the run.

| # | Rule |
|---|------|
| 1 | **CLI + skill only** for the packaged path — no ad-hoc reimplementation of record/voice/composite inside this skill. |
| 2 | **No product source edits** under `pageseeds-app`. Missing capability → escalate / document gap. |
| 3 | **Never invent `video.config.json`** — stop with gap report + docs link. |
| 4 | **Never modify the webapp** (or product code in the target repo) “for a shot.” New UI moments = product gap. |
| 5 | **Demo / config-declared targets only** — `timing_map[].ui_target` ⊆ config `ui_targets` (+ builtin `end_card`). Prefer stable demo routes (e.g. `/demo-portfolio`). |
| 6 | Clip path **`video/clips/<slug>.json`**; outputs central at **`~/01_code/video-clip-backup/<project_id>/`** (engine SoT). Not bare `clips/`. |
| 7 | **Verify frames + ffprobe** before claiming success. |
| 8 | Ground `spoken_script` in article body/context — no invented figures (`video-script` rule). |
| 9 | Operator-tier render assumes **dev machine** with node/ffmpeg + pageseeds-app `video-engine/`. Fail with install hints; do not paper over. |
| 10 | **One slug per run** unless the user explicitly forces more. |
| 11 | **Agentic segment media exception:** optional pre-pass may write segment media under the **central backup OUT only** (`~/01_code/video-clip-backup/<project_id>/segments/` and optional `…/agentic/` staging). Still **ban** product source edits mid operator run; still **ban** reimplementing voice/composite. Agentic is opt-in per config `ui_target`; scripted remains default. |

---

## Soft guidance (default path)

```text
resolve project → pick one slug → gate video.config.json
  → video-clip-context -S <slug>
  → draft clip JSON (video-script craft) → video/clips/<slug>.json
  → preflight dev servers from config
  → (if any timing_map ui_target has motion=agentic) agentic segment pre-pass
       → place trimmed media under central OUT/segments/
  → video-clip-render --clip video/clips/<slug>.json
       (record.mjs reuses agentic files → voice → composite)
  → ffprobe + ≥5 frames quality gate
  → packaging report
  → default publish: YouTube + Instagram Reels (one confirm)
       via video-engine/publish.py --platforms youtube,instagram
  → (if YouTube published) ensure clip has published.youtube
       (publish.py write-back; if older publish.py, skill may write it once from stdout)
       → embed into source MDX (default yes after default YT+IG publish; ask only if unclear)
  → (if Instagram published) ensure clip has published.instagram; report media_id +
       permalink — do NOT embed Instagram into MDX
  → (optional TikTok inbox — only if user asks; not in default pair)
  → report article path + embed status
```

### A. Resolve project

```bash
pageseeds-cli list-projects
# or rely on setup defaults in cwd
```

Match user args by project `id` / `name`. When the registered project path has
no `video.config.json` but a known video worktree does, pass `-p` to that
worktree explicitly.

### B. Pick slug

- User arg / `--slug` / `-S` wins.
- Else choose **one** article with clear visual potential from desk
  (`articles` / `article -S`) — calculators, dashboards, scanners, step demos.
- Prefer recently shipped posts when the user says “after publish.”

### C. Config gate

```bash
test -f <project-path>/video.config.json
```

Missing → stop with:

```text
Gap: video.config.json not found under <project-path>.
Cannot invent config. See docs/video_clip_spec.md and video-engine/README.md.
```

Also read `ui_targets` (and `dev_servers`, `base_url`, `ready_path`) so later
`timing_map` keys stay valid.

**Target review (multi-clip sessions):** before drafting several clips,
check every planned `timing_map` moment against `ui_targets`. Add missing
targets to `video.config.json` in ONE deliberate committed edit up front —
never invent a target mid-run. Prefer fine-grained anchors on the same page
(`scroll_to_text` to distinct sections) so adjacent segments look like
different shots.

### D. Context (free desk)

```bash
pageseeds-cli video-clip-context -i <id> -p <path> -S <slug>
```

Expect JSON: `project_id`, `slug`, `title`, `h1`, `file_path`, `published_at`,
`status`, `word_count`, `frontmatter`, `body`, `site_base_url`,
`packaging_hints` (`hashtags`, `canonical_url`).

On hard error (unknown slug, missing file) → stop; do not invent article body.

### E. Draft clip definition (craft)

Load/reference embedded **`video-script`** craft (schema v1 in
`docs/video_clip_spec.md`). Write:

```text
<video-project-path>/video/clips/<slug>.json
```

| Field | Source of truth |
|-------|-----------------|
| `schema_version` | `1` |
| `source.*` | Context JSON (`project_id`, `slug`, `title`, `file_path` → `content_path`) |
| `spoken_script` | video-script rules (hook first, TTS-safe, grounded in body) |
| `keywords` | frontmatter + article; hook keyword first |
| `timing_map` | 4–6 segments, 40–50s total; **`ui_target` ∈ config `ui_targets` ∪ {`end_card`}** |
| `cta` / `packaging` | packaging_hints + video-script |

Do **not** paste the entire craft doc into this skill — follow `video-script`.

### F. Preflight servers

From `video.config.json` `dev_servers[]`, print exact start commands
(`command`, `cwd` relative to project root, `port`, optional `ready_path`).
Ensure `base_url` (+ `ready_path` if set) responds before render.

Do **not** start a half-render hoping servers appear. If servers cannot be
started in this session, report the exact commands and stop.

### F2. Agentic segment pre-pass (optional)

**Default remains fully scripted.** Run this only when the clip’s
`timing_map` references a `ui_target` with `motion: "agentic"` in
`video.config.json`. Requires `agentic_goal` on that target (`record.mjs
--check` enforces). Engine docs: `video-engine/README.md` (Agentic motion);
MCP profile example: `video-engine/mcp-recording.example.json`.

```text
for each timing_map seg whose ui_target has motion=agentic:
  build brief from agentic_goal + caption_text + rails
    (demo/read-only pages; consent dismiss OK; no forms/auth; on page error → navigate back / continue)
  record via host Playwright MCP (preferred) OR `kimi -p "<brief>"` with recording MCP profile
    (hard time cap language in brief; start/stop video per segment)
  trim_deadair.py on the raw take → place under
    ~/01_code/video-clip-backup/<project_id>/segments/seg{NN}_{ui_target}.mp4
  verify: ffprobe duration + ≥1 frame view; retry once with tighter brief; else leave missing → record fallback
then: normal video-clip-render / generate-clip (record reuses agentic files → voice → composite)
quality gate unchanged
```

| Detail | Rule |
|--------|------|
| **When** | After preflight (F), **before** G. Render, when ≥1 used target has `motion: "agentic"` |
| **Host MCP preferred** | If this session already has a `playwright-recording` (or equivalent) MCP server with **startup** `--viewport-size 1080x1920`, use those tools. Mid-session resize is **not** enough — video stays at the MCP default (e.g. 800×450). |
| **Alternate headless** | `kimi -p "<brief>"` with the recording MCP profile merged into host config. Use **`kimi -p` only** — not `-p --auto` / `-p --yolo` (both rejected by the CLI). |
| **MCP profile** | Copy `video-engine/mcp-recording.example.json` into host MCP config (Kimi/Grok). Set `--output-dir` to `~/01_code/video-clip-backup/<project_id>/agentic/`. Blocked origins: crisp/intercom/hotjar/fullstory (align with `overlays.block_routes`). Do **not** auto-edit the user’s global MCP file from product scripts. |
| **Brief rails** | Demo/read-only pages only; consent dismiss OK; **no** forms, auth, purchases, or real mutations; on page error → navigate back / continue; include a **hard time cap** (e.g. “finish in ≤12s of action”); start/stop video **per segment**. |
| **Trim** | From a pageseeds-app checkout: `video-engine/.venv/bin/python video-engine/trim_deadair.py <raw> --out ~/01_code/video-clip-backup/<project_id>/segments/seg{NN}_{ui_target}.mp4` |
| **Naming** | `seg{NN}_{ui_target}.mp4` with zero-padded index matching `timing_map` order (same as `record.mjs`). |
| **Verify** | `ffprobe` duration + extract ≥1 frame and view. Retry **once** with a tighter brief on failure; else leave the file missing so `record.mjs` scripted fallback runs. |
| **Fallback** | Missing/unusable agentic file → `record.mjs` logs fallback and records with `dwell` (if interactions/hover) or `dwell_scroll`. Do not reimplement composite/voice. |
| **Timing** | Raw MCP takes may overrun the brief; `trim_deadair` then composite `-t` scale handle duration. No engine rewrite needed. |

Do **not** nest `kimi` or MCP inside `generate-clip.sh` / `record.mjs`. Judgment stays in this skill; the engine stays deterministic reuse/fallback.

### G. Render (operator tier)

```bash
pageseeds-cli video-clip-render -p <path> --clip video/clips/<slug>.json
```

Success: exit 0 and JSON with at least:

| Field | Meaning |
|-------|---------|
| `output_path` | Absolute path to final MP4 |
| `thumbnail_path` | Optional absolute thumbnail |
| `duration_s` | Optional ffprobe duration |
| `clip_path` | Absolute clip definition path |

Engine stdout contract (parsed by CLI): `video-engine: output=…`,
`video-engine: thumbnail=…`. Bare `OUTPUT:` markers are **not** contractual.

On missing node/ffmpeg/venv/engine → report install hints from the CLI error;
do not reimplement the pipeline.

### H. Quality gate (before success)

1. **ffprobe** on `output_path`:
   - Video: **1080×1920**
   - Duration: **40–50s** (allow tiny tolerance only if tool reports float)
   - **Audio** stream present
2. Extract **≥5** frames (early hook, mid segments, pre-end-card).
3. **Visually** inspect: hook caption / UI moment / end card look intentional.
4. **`packaging.description` (deterministic string checks on the written clip
   JSON — before claiming success / before optional publish):**
   - **Hard fail** if empty.
   - **Hard fail** if the canonical URL (`packaging_hints.canonical_url` or
     `cta.url`) is **not** present in the **first two non-empty lines** of the
     description (ignore blank lines when counting; preferred craft shape is
     hook on line 1, URL on line 2 with no intervening blank line).
   - **Hard fail** if `frontmatter.target_keyword` was available in context
     and does **not** appear in the description (case-insensitive).
   - **Hard fail / flag** if length **> 5,000** characters (YouTube hard
     limit). Report the char count; do not claim a clean packaging pass.
   - **Soft warn** if length **< ~800** characters ("too thin" for SEO
     packaging) — soft fail / report unless empty (empty is hard fail). Soft
     target band when substance allows: ~1,500–4,000 chars (see
     `docs/video_clip_spec.md` § packaging.description).

Any hard fail → report failure with evidence; do not claim a publishable short.

```bash
ffprobe -v error -select_streams v:0 -show_entries stream=width,height \
  -of csv=p=0:s=x "$OUTPUT"
ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUTPUT"
ffprobe -v error -select_streams a -show_entries stream=codec_type -of csv=p=0 "$OUTPUT"
# frames example:
ffmpeg -y -i "$OUTPUT" -vf "select=eq(n\,0)+eq(n\,30)+eq(n\,60)+eq(n\,90)+eq(n\,120)" \
  -vsync vfr /tmp/video-clip-frame-%02d.png
```

### I. Report

Prefer a **concise final user message** (paths + packaging). Optional file:

`<project-path>/.github/automation/video_clip_{slug}_{YYYYMMDD_HHMMSS}.md`

```markdown
# Video Clip — {project} / {slug}

**Date:** {ISO}

## Output
- MP4: {absolute path}
- Thumbnail: {path or none}
- Duration: {s}
- Clip def: video/clips/{slug}.json

## Packaging (upload block)
- Title: …
- Description: …
- Hashtags: …

## Quality gate
- ffprobe: 1080×1920 / 40–50s / audio: pass|fail
- Frames inspected: n (notes)
- packaging.description: URL in first 2 non-empty lines pass|fail · keyword present pass|fail|n/a · chars={n} (≤5000; soft-warn if <~800)

## Friction / gaps
- …
```

### Final user message (no JSON dumps)

```
## Video Clip — {project} / {slug}

**TL;DR:** {pass|stopped} — {one line}

**MP4:** {path}
**Thumbnail:** {path|none}

**Packaging**
- Title: …
- Description: …
- Hashtags: …

**Quality gate:** ffprobe + frames + packaging.description (URL/keyword/length) {pass|fail notes}

**Publish:**
- YouTube: {skipped|url + privacy} · clip published.youtube {written|n/a}
- Instagram: {skipped|media_id + permalink} · clip published.instagram {written|n/a}
- TikTok: {skipped|publish_id + “finish in TikTok app”} · clip published.tiktok {written|n/a}
**Article embed:** {inserted|skipped (already present)|skipped (no YouTube publish)|path + note}
  (YouTube only — never auto-embed TikTok or Instagram)

**Next:** default is YouTube + Instagram via
`publish.py --platforms youtube,instagram` (one confirm after quality gate).
YouTube: report URL + clip write-back + MDX embed. Instagram: report media_id +
permalink; no MDX embed. TikTok only if user asks.
```

### J. Post-publish — clip ledger + source MDX embed

#### Default platform publish (YouTube + Instagram)

After a **passing** quality gate, the default distribution step is **YouTube Shorts
+ Instagram Reels** in one multi-platform run. Secrets live in
`~/.config/automation/secrets.env` (`YOUTUBE_*`, `META_ACCESS_TOKEN`, `IG_USER_ID`).

| Policy | Detail |
|--------|--------|
| **Default platforms** | `youtube,instagram` |
| **Confirmation** | **One** yes/no after quality gate (not per-platform). Default intent when the user already said “publish” / “ship the short” / full `/video-clip` with publish expected: proceed. If unclear, ask once: “Publish to YouTube + Instagram?” |
| **Skip** | User says no / “local only” / “don’t publish” → report packaging only |
| **Partial** | User may override: YouTube only, Instagram only, or add TikTok |
| **TikTok** | **Not** in the default pair (inbox drafts; separate auth). Only if user explicitly asks |
| **Failure isolation** | Platforms run independently; one failure does not skip the other. Report each status |

```bash
# From a pageseeds-app checkout that has video-engine/ (not the customer repo).
# Prefer the same checkout used for video-clip-render.
PAGESEEDS_APP="${PAGESEEDS_APP:-$HOME/01_code/pageseeds-app}"
CLIP="<absolute path to video/clips/<slug>.json>"
VIDEO="<absolute path to rendered mp4>"   # or omit if publish.py can resolve it

"$PAGESEEDS_APP/video-engine/.venv/bin/python" \
  "$PAGESEEDS_APP/video-engine/publish.py" \
  "$CLIP" \
  --platforms youtube,instagram \
  --video "$VIDEO"
```

| Platform | Command subset | Report | MDX embed? |
|----------|----------------|--------|------------|
| **YouTube + Instagram (default)** | `--platforms youtube,instagram` | YT URL + privacy; IG `media_id` + permalink | YouTube only |
| YouTube only | `--platforms youtube` | URL + privacy | Yes (below) |
| Instagram Reels only | `--platforms instagram` | `media_id` + permalink | **No** |
| TikTok inbox (opt-in) | `--platforms tiktok` | `publish_id` + “finish in TikTok app” | **No** |

**Instagram Login path (current operator setup):** tokens starting with `IGAA…`
use `graph.instagram.com`. Local MP4 is staged to a short-lived public URL unless
`INSTAGRAM_VIDEO_URL` is set. See `video-engine/README.md`. Facebook Login
resumable rupload still works for classic `EAA…` tokens.

Confirm write-backs after a real upload:

```json
"published": {
  "youtube": {
    "video_id": "<id>",
    "url": "https://youtu.be/<id>",
    "published_at": "<ISO-8601 UTC>",
    "privacy": "private"
  },
  "instagram": {
    "media_id": "<id>",
    "url": "<permalink or empty>",
    "published_at": "<ISO-8601 UTC>"
  }
}
```

TikTok (opt-in only) uses **Inbox Upload** (`video.upload`): drafts in the TikTok
app. Confirm `published.tiktok` (`publish_id`, `mode: "inbox"`, note) if used.

Dry-run does not write the clip file. Re-publish overwrites that platform key only.

#### 1. Ensure `published.youtube` on the clip

Run only after a **successful** YouTube publish (user already said yes). Dry-run
does not trigger this path.

`video-engine/publish.py` (current) writes this back into `video/clips/<slug>.json`
after a real upload. Confirm the clip file has:

```json
"published": {
  "youtube": {
    "video_id": "<id>",
    "url": "https://youtu.be/<id>",
    "published_at": "<ISO-8601 UTC>",
    "privacy": "private"
  }
}
```

If the installed `publish.py` is older and only returned these fields on **stdout**,
the skill may write `published.youtube` **once** from that stdout into the clip
JSON (merge; do not wipe other keys). Prefer engine write-back when available.
Re-publish overwrites `published.youtube` with the latest successful upload (no
multi-version history).

#### 2. Embed into source MDX (YouTube only)

| Rule | Detail |
|------|--------|
| **Path** | `source.content_path` from the clip JSON, relative to the **customer project root** |
| **Position** | After intro / first paragraph block, **before the first `## ` H2** |
| **Snippet** | Portable responsive iframe to `https://www.youtube.com/embed/{video_id}` + short "Watch the short" lead-in; raw HTML/JSX OK. Do **not** require `LazyYouTubeEmbed` or `BlogTrialCta` |
| **Idempotency** | If body already contains `youtube.com/embed/{video_id}` or `youtu.be/{video_id}`, **skip** (no duplicate embeds) |
| **Frontmatter** | Leave frontmatter untouched; only body insert |
| **Privacy** | If privacy is `private`, **warn** that the public site will not play until the video is public (#234). Still allow insert unless the user aborts |
| **Scope** | Customer content file only; still ban product/webapp source edits |
| **Default** | After the user already approved publish → embed **yes** by default; ask only if unclear |

Concrete portable snippet (replace `VIDEO_ID`):

```mdx
## Watch the short

<div style={{ position: "relative", paddingBottom: "177.78%", height: 0, overflow: "hidden", maxWidth: "100%" }}>
  <iframe
    src="https://www.youtube.com/embed/VIDEO_ID"
    title="Watch the short"
    allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
    allowFullScreen
    style={{ position: "absolute", top: 0, left: 0, width: "100%", height: "100%", border: 0 }}
  />
</div>
```

Report: article absolute/relative path + embed status (`inserted` | `skipped —
already present` | `skipped — user aborted` | `failed` with reason).

---

## Explicit bans

| Ban | Do instead |
|-----|------------|
| Invent `video.config.json` / new `ui_targets` mid-run | Stop + gap report |
| Edit webapp / product for a shot | Product gap / separate PR |
| Reimplement record/voice/composite in the skill | `video-clip-render` only (agentic pre-pass may write **segment media** under central OUT only) |
| Nest `kimi` / MCP inside `generate-clip` or `record.mjs` | Host MCP or operator `kimi -p` **before** render; engine reuses files |
| Bare `clips/<slug>.json` path | `video/clips/<slug>.json` |
| Multi-clip batch / scheduler | One slug; user re-invokes |
| Claim success without ffprobe + frames | Run quality gate |
| Publish without any confirmation when intent is unclear | Default pair is YouTube + Instagram after quality gate; **one** confirm if unclear; honor “don’t publish” / platform overrides |
| Auto-embed TikTok or Instagram into MDX | YouTube iframe embed only; TikTok report `publish_id` + finish in app; Instagram report `media_id` + permalink |
| `create-task generate_video_clip` | Out of scope (#224); skill path only |
| Patch `pageseeds-app` for missing tools | Report product gap |
| Weekly SEO may-create expansion | Elective handoff only (see weekly-seo) |

---

## Relationship to weekly-seo

| Pass | Owns |
|------|------|
| `/weekly-seo` | GSC desk, content, research; **elective** post-publish video candidate (0–1, default 0) |
| `/video-clip` | Full clip context → craft → render → quality gate |

Weekly default: name the slug + suggest `/video-clip <slug>` in the weekly
report. Mid-pass Playwright render only when the user **explicitly** wants video
now. Video is **not** a `create-task` type and is **not** on the weekly may-create
list.

---

## Guardrails (summary)

- Config gate first; never invent `video.config.json`.
- CLI context + render only; craft via `video-script`; policy here.
- Paths: `video/clips/` in, `~/01_code/video-clip-backup/<project_id>/` out.
- Optional agentic pre-pass (F2) may write segment media under central OUT only.
- Demo/config `ui_targets` only; no webapp edits for shots.
- One slug per run; operator-tier deps required for render.
- ffprobe 1080×1920 / 40–50s / audio + ≥5 frames before success.
- **Default publish** after quality gate: YouTube + Instagram via
  `publish.py --platforms youtube,instagram` (one confirm if intent unclear).
  TikTok remains opt-in only.
- After YouTube publish: ensure `published.youtube` on clip (engine write-back or one-time skill merge from stdout); embed portable iframe into source MDX body (idempotent; privacy warn if private). Customer content only.
- After Instagram publish: ensure `published.instagram` (`media_id`, `url`, `published_at`); report permalink. **Do not** auto-embed Instagram into MDX.
- After TikTok publish (opt-in): ensure `published.tiktok` (`publish_id`, `mode: inbox`, note); report “finish in TikTok app”. **Do not** auto-embed TikTok into MDX.
- No product source edits; missing tools → report gap.

---

## Design note

```text
video-clip-context (free desk)
  → agent + video-script craft → video/clips/<slug>.json
  → (optional) agentic segment pre-pass: host MCP / kimi -p → trim_deadair
       → ~/01_code/video-clip-backup/<project_id>/segments/seg{NN}_*.mp4
  → video-clip-render (operator tier → video-engine/generate-clip.sh)
       record reuses agentic files or scripted fallback → voice → composite
  → ffprobe + frame gate
  → packaging report
  → default publish: YouTube + Instagram (one confirm; publish.py --platforms youtube,instagram)
  → published.youtube write-back + source MDX embed (YouTube only)
  → published.instagram write-back (media_id + permalink; no MDX embed)
  → optional TikTok inbox only if user asks
```

This skill is the **operator policy** layer (epic #220 / #222). Phase C task type
(`generate_video_clip`) is deliberately out of scope until several videos prove
the loop. Spec SoT: `docs/video_clip_spec.md`. Post-publish linkage: #235. TikTok
inbox adapter: #231. Instagram Reels adapter: #232 (Instagram Login path supported).
