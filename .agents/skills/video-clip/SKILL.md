---
name: video-clip
description: >-
  Produce one vertical short (40–50s MP4) from a PageSeeds article via
  pageseeds-cli: video-clip-context → video-script craft → video-clip-render →
  quality gate → packaging report. Use when the user wants a video clip, short
  from an article, YouTube Short / Reels / TikTok package, or /video-clip.
  Operator only — never edit pageseeds-app source.
when-to-use: >-
  Triggers on "/video-clip", "video clip", "make a short from this article",
  "render video clip", "article to short", "YouTube Shorts from blog",
  "vertical video for this post".
argument-hint: "[project-name-or-id] [slug]"
user-invocable: true
metadata:
  short-description: "Article → clip JSON → render → quality gate (one short)"
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

---

## Soft guidance (default path)

```text
resolve project → pick one slug → gate video.config.json
  → video-clip-context -S <slug>
  → draft clip JSON (video-script craft) → video/clips/<slug>.json
  → preflight dev servers from config
  → video-clip-render --clip video/clips/<slug>.json
  → ffprobe + ≥5 frames quality gate
  → packaging report
  → optional YouTube / TikTok inbox / Instagram Reels publish (ask first per platform)
       via video-engine/publish.py
  → (if YouTube published) ensure clip has published.youtube
       (publish.py write-back; if older publish.py, skill may write it once from stdout)
       → embed into source MDX (default yes after user said YouTube publish; ask only if unclear)
  → (if TikTok published) ensure clip has published.tiktok; report publish_id +
       “finish in TikTok app” — do NOT embed TikTok into MDX
  → (if Instagram published) ensure clip has published.instagram; report media_id +
       permalink — do NOT embed Instagram into MDX
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
- TikTok: {skipped|publish_id + “finish in TikTok app”} · clip published.tiktok {written|n/a}
- Instagram: {skipped|media_id + permalink} · clip published.instagram {written|n/a}
**Article embed:** {inserted|skipped (already present)|skipped (no YouTube publish)|path + note}
  (YouTube only — never auto-embed TikTok or Instagram)

**Next:** optional YouTube / TikTok inbox / Instagram Reels publish via
video-engine/publish.py (ask before each platform). YouTube: report URL + clip
write-back + MDX embed. TikTok: report publish_id + finish in app; no MDX embed.
Instagram: report media_id + permalink; no MDX embed.
```

### J. Post-publish — clip ledger + source MDX embed

#### Optional platform publish (ask first)

After the quality gate, you may offer publish — **always ask before each platform**:

| Platform | Command | Report | MDX embed? |
|----------|---------|--------|------------|
| YouTube | `publish.py <clip> --platforms youtube` | URL + privacy | Yes (below) |
| TikTok inbox | `publish.py <clip> --platforms tiktok` | `publish_id` + “finish in TikTok app” | **No** |
| Instagram Reels | `publish.py <clip> --platforms instagram` | `media_id` + permalink | **No** |
| Multi | `--platforms youtube,tiktok,instagram` (any subset) | per-platform results | YouTube only |

TikTok uses **Inbox Upload** (`video.upload`): the short lands in the creator’s
TikTok drafts/inbox; the human finishes caption and post in the TikTok app.
Confirm `published.tiktok` on the clip after a real upload:

```json
"published": {
  "tiktok": {
    "publish_id": "<id>",
    "mode": "inbox",
    "published_at": "<ISO-8601 UTC>",
    "note": "finish in TikTok app"
  }
}
```

Instagram uses Graph API **Reels** (resumable container + rupload). Confirm
`published.instagram` on the clip after a real upload:

```json
"published": {
  "instagram": {
    "media_id": "<id>",
    "url": "<permalink or empty>",
    "published_at": "<ISO-8601 UTC>"
  }
}
```

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
| Reimplement record/voice/composite in the skill | `video-clip-render` only |
| Bare `clips/<slug>.json` path | `video/clips/<slug>.json` |
| Multi-clip batch / scheduler | One slug; user re-invokes |
| Claim success without ffprobe + frames | Run quality gate |
| Publish without asking | Optional YouTube / TikTok inbox / Instagram Reels via `video-engine/publish.py` only after quality gate + **explicit user yes per platform** |
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
- Demo/config `ui_targets` only; no webapp edits for shots.
- One slug per run; operator-tier deps required for render.
- ffprobe 1080×1920 / 40–50s / audio + ≥5 frames before success.
- Optional YouTube / TikTok inbox / Instagram Reels publish via `video-engine/publish.py` after quality gate — **ask first per platform**, never publish without confirmation.
- After YouTube publish: ensure `published.youtube` on clip (engine write-back or one-time skill merge from stdout); embed portable iframe into source MDX body (idempotent; privacy warn if private). Customer content only.
- After TikTok publish: ensure `published.tiktok` (`publish_id`, `mode: inbox`, note); report “finish in TikTok app”. **Do not** auto-embed TikTok into MDX.
- After Instagram publish: ensure `published.instagram` (`media_id`, `url`, `published_at`); report permalink. **Do not** auto-embed Instagram into MDX.
- No product source edits; missing tools → report gap.

---

## Design note

```text
video-clip-context (free desk)
  → agent + video-script craft → video/clips/<slug>.json
  → video-clip-render (operator tier → video-engine/generate-clip.sh)
  → ffprobe + frame gate
  → packaging report
  → optional YouTube / TikTok inbox / Instagram Reels publish (ask first per platform)
  → published.youtube write-back + source MDX embed (YouTube only)
  → published.tiktok write-back (inbox; finish in app; no MDX embed)
  → published.instagram write-back (media_id + permalink; no MDX embed)
```

This skill is the **operator policy** layer (epic #220 / #222). Phase C task type
(`generate_video_clip`) is deliberately out of scope until several videos prove
the loop. Spec SoT: `docs/video_clip_spec.md`. Post-publish linkage: #235. TikTok
inbox adapter: #231. Instagram Reels adapter: #232.
