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
| **Config** | Target repo `video.config.json` + `video/clips/` + `video/out/` |
| **Product source** | **Out of scope** — never patch `pageseeds-app` mid-run |

---

## Separation of concerns (mandatory)

| Role | Workspace | May write |
|------|-----------|-----------|
| **This skill** | Customer project / neutral cwd | Clip JSON under `video/clips/`, optional short automation report |
| **pageseeds-cli** | N/A (binary on PATH) | Context JSON stdout; render via operator-tier engine |
| **video-engine** | pageseeds-app checkout | Outputs under target `video/out/` |
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
| `video/out/` | Render outputs (engine-owned) |

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
| 6 | Clip path **`video/clips/<slug>.json`**; outputs under **`video/out/`** (engine SoT). Not bare `clips/`. |
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
  → optional YouTube publish (ask first) via video-engine/publish.py
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

Any fail → report failure with evidence; do not claim a publishable short.

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

**Quality gate:** ffprobe + frames {pass|fail notes}

**Next:** optional YouTube publish via video-engine/publish.py (ask before publishing; report URL). TikTok/Reels remain manual — no auto-upload for those.
```

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
| TikTok/IG auto-upload; publish without asking | Optional YouTube via `video-engine/publish.py` only after quality gate + explicit user yes; packaging block for other platforms |
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
- Paths: `video/clips/` in, `video/out/` out.
- Demo/config `ui_targets` only; no webapp edits for shots.
- One slug per run; operator-tier deps required for render.
- ffprobe 1080×1920 / 40–50s / audio + ≥5 frames before success.
- Optional YouTube publish via `video-engine/publish.py` after quality gate — **ask first**, never publish without confirmation. TikTok/IG stay manual (packaging block only).
- No product source edits; missing tools → report gap.

---

## Design note

```text
video-clip-context (free desk)
  → agent + video-script craft → video/clips/<slug>.json
  → video-clip-render (operator tier → video-engine/generate-clip.sh)
  → ffprobe + frame gate
  → packaging report
```

This skill is the **operator policy** layer (epic #220 / #222). Phase C task type
(`generate_video_clip`) is deliberately out of scope until several videos prove
the loop. Spec SoT: `docs/video_clip_spec.md`.
