# Video Clip Generation Spec

Tracking issue: [#220](https://github.com/fstrauf/pageseeds-app/issues/220) · Phase B toolchain: [#221](https://github.com/fstrauf/pageseeds-app/issues/221) · E2E: [#223](https://github.com/fstrauf/pageseeds-app/issues/223) · Phase D publish: [#228](https://github.com/fstrauf/pageseeds-app/issues/228)
Status: **Phase A complete**; **Phase B landed** (#221) and **Phase B validated**
(#223, 2026-07-27) — one-command packaged path on `days_to_expiry` video worktree:
`video-clip-render -p <worktree> --clip video/clips/best-stocks-csp.json` → exit 0,
`output_path` …/video/out/best-stocks-csp.mp4, `duration_s` ≈ 42.57, ffprobe
1080×1920 + audio. Context smoke: `video-clip-context -S best-stocks-csp` returns
body + `frontmatter.faq` / `target_keyword` + `packaging_hints`.
**Phase D (#228):** YouTube-only `video-engine/publish.py` (stdlib urllib; packaging block payload; optional skill step).
**Operator path:** `.agents/skills/video-clip/SKILL.md` (`/video-clip`, #222).
**Target footprint:** `video.config.json` + `video/clips/` + `video/out/` only (engine
lives in pageseeds-app `video-engine/`, not the customer repo).
Phase C still optional. PoC project: daystoexpiry

## Purpose

Produce 40–50 second vertical videos (YouTube Shorts / Reels / TikTok) from existing blog
posts, with as much of the pipeline scripted and repeatable as possible. This spec defines
the contract between PageSeeds (content intelligence) and the generic render engine (video
production), and the phased path from one manual-scripted video to batch generation.

## Architectural Boundary

| Half | What | Where |
|---|---|---|
| **Content intelligence** — article → spoken script, timing map, keywords, clip definition JSON, packaging metadata | Judgment over repo content; matches the existing Path B package/submit and skill patterns | **pageseeds-core / CLI / skill** (free desk + paid-safe surface) |
| **Generic engine** — Playwright journey, screen recording, TTS, word timestamps, captions, composite | Node/Python subprocess tooling (Playwright, edge-tts, whisper, FFmpeg) | **pageseeds-app `video-engine/`**, invoked only by **operator-tier** `video-clip-render` |
| **Target project repo** | Project config + clip artifacts | Owns `video.config.json`, clip definition JSON, journeys/out — **not** a second full engine tree for the packaged operator path |

**Subprocess policy:** Commercial free/paid tools remain pure Rust (AGENTS.md subprocess-by-tier;
`docs/CLI_COMMERCIAL.md`). Operator-tier tools may spawn external
processes when the capability cannot be Rust-native. `video-clip-render` is operator-tier:
it shells out to `video-engine/generate-clip.sh` (Node/FFmpeg) on a source checkout of
pageseeds-app. The prebuilt customer binary does not promise this path.

Content intelligence produces the versioned clip definition JSON; the engine consumes it
and emits contractual stdout lines (`video-engine: output=…`, `video-engine: thumbnail=…`).
This mirrors the Path B split: Rust emits a deterministic, versioned package; the outside
toolchain consumes it — with the operator-tier exception that pageseeds-core may *invoke*
that toolchain when the operator opts in.

## Clip Definition JSON — schema v1

The single artifact passed from content intelligence to the renderer.
`schema_version` is required and versioned independently of the CLI.

| Field | Type | Description |
|---|---|---|
| `schema_version` | integer | Currently `1`. Renderers must reject unknown versions. |
| `source` | object | `{ project_id, slug, title, content_path }` — the origin article. |
| `spoken_script` | string | 35–45s spoken voiceover. Strong hook in the first 3 seconds, one clear takeaway, soft CTA at the end. Plain text, no markup. |
| `keywords` | string[] | 4–6 target phrases from the article. First entry is the hook keyword shown as a big caption in the first 3 seconds. |
| `timing_map` | object[] | Ordered segments, see below. Total duration must cover the voiceover length (target 40–50s final cut). |
| `cta` | object | `{ text, url, subtitle? }` — end card call to action. `subtitle` (optional) renders under the domain and is per-clip; it overrides the config `brand.end_card.subtitle` default. |
| `packaging` | object | `{ title, description, hashtags, thumbnail_hint }` — derived from article keywords for upload metadata. |

### `timing_map` segment

| Field | Type | Description |
|---|---|---|
| `from_s` / `to_s` | number | Segment bounds in seconds, contiguous from 0. |
| `moment_template` | string | One of the moment templates below. |
| `caption_text` | string | On-screen caption for the segment (short phrase; renderer handles word-level timing from whisper). |
| `ui_target` | string | Logical name of the UI view/element to show (e.g. `income_dashboard`, `scanner_results`, `put_calculator`). Interpreted by the project's Playwright journey. |

### Moment templates (v1)

| Template | Visual |
|---|---|
| `income_snapshot` | Portfolio income dashboard: weekly income, coverage %, positions near expiry |
| `scanner_highlight` | Run a scan, highlight the top result row |
| `calculator_demo` | Calculator inputs → output numbers animate in |
| `expiry_decision` | Position near expiry: hold/roll/close decision view |
| `ai_ask` | AI assistant answering the article's core question |

Templates are the reusable "visual moments" library. v1 starts with these five; new ones
are added to this spec and the project's Playwright journey together.

### Example

```json
{
  "schema_version": 1,
  "source": {
    "project_id": 3,
    "slug": "best-stocks-cash-secured-puts-2026",
    "title": "Best Stocks for Selling Cash-Secured Puts in 2026",
    "content_path": "content/blog/best-stocks-cash-secured-puts-2026.mdx"
  },
  "spoken_script": "Selling puts on the wrong stock is how accounts blow up. Here are the three filters I run before every cash-secured put — and the one ticker passing all of them right now. ... Want the full list? It's on daystoexpiry.com.",
  "keywords": ["cash-secured puts", "best stocks for puts", "put selling filters", "options income", "wheel strategy"],
  "timing_map": [
    { "from_s": 0, "to_s": 4, "moment_template": "income_snapshot", "caption_text": "cash-secured puts", "ui_target": "income_dashboard" },
    { "from_s": 4, "to_s": 22, "moment_template": "scanner_highlight", "caption_text": "3 filters before every put", "ui_target": "scanner_results" },
    { "from_s": 22, "to_s": 38, "moment_template": "calculator_demo", "caption_text": "the one ticker passing all three", "ui_target": "put_calculator" },
    { "from_s": 38, "to_s": 45, "moment_template": "income_snapshot", "caption_text": "full list on daystoexpiry.com", "ui_target": "end_card" }
  ],
  "cta": { "text": "Stock list + portfolio scanner", "url": "https://daystoexpiry.com/blog/best-stocks-cash-secured-puts-2026", "subtitle": "the full CSP stock list" },
  "packaging": {
    "title": "3 filters before every cash-secured put",
    "description": "The exact checklist I run before selling puts, plus the ticker passing all three right now. Full article on daystoexpiry.com.",
    "hashtags": ["#options", "#cashsecuredputs", "#wheelstrategy", "#passiveincome"],
    "thumbnail_hint": "scanner_results top row"
  }
}
```

## Output requirements (renderer contract)

- 9:16 vertical, 1080×1920 MP4
- Final cut 40–50s; 25–40s of actual UI action in the base recording
- Large readable captions (word-by-word or short phrases), timed from whisper word timestamps
- Optional: subtle logo, progress bar, end card with CTA
- First 3 seconds: visually strong moment + big caption with the hook keyword

### Engine stdout contract

Only lines starting with `video-engine: ` are contractual (`video-engine/generate-clip.sh`).
pageseeds-core parses:

```
video-engine: output=<absolute path to final mp4>
video-engine: thumbnail=<absolute path to thumbnail jpg>
```

Last matching line of each key wins. Stage progress lines
(`video-engine: stage=… status=…`) are informational. Bare `OUTPUT:` markers are **not**
part of the contract.

## Workflow phases

### Canonical operator path (product)

**Source of truth for article → clip JSON → render → quality gate:**  
[`.agents/skills/video-clip/SKILL.md`](../.agents/skills/video-clip/SKILL.md)
(`/video-clip`). Grok discovery: `.grok/skills/video-clip/SKILL.md` is a symlink
to that file — edit only the `.agents` path.

Sequence:

1. Gate on target-repo `video.config.json` (never invent it).
2. Free desk: `pageseeds-cli video-clip-context -S <slug>`.
3. Agentic craft via embedded `video-script` → write `video/clips/<slug>.json`
   (schema v1 in this doc).
4. Preflight `dev_servers` / `base_url` from config.
5. Operator tier: `pageseeds-cli video-clip-render --clip video/clips/<slug>.json`.
6. Quality gate: ffprobe (1080×1920, 40–50s, audio) + ≥5 visual frames.
7. Report MP4 path + packaging block; optional YouTube publish via `video-engine/publish.py` (ask first).

Weekly SEO may **electively** suggest `/video-clip <slug>` after a Path B ship
when config exists (0–1 candidates, default 0) — not a weekly spine action and
not may-create. See `.agents/skills/weekly-seo/SKILL.md`.

### Phase A — PoC, zero Rust changes (complete; historical)

1. Pick one article with clear visual potential (income/wheel/calculator piece).
2. Build a **stable mocked demo portfolio** in the project repo — Playwright journeys must
   never fail on empty or boring data. This is the biggest PoC risk; solve it first.
3. Project-level `video-clip` skill in the **customer** repo was a **historical PoC** —
   reads via desk tools, drafts clip JSON, runs render scripts. **Product operator
   policy now lives in pageseeds-app** (`.agents/skills/video-clip/`); do not re-home
   the canonical skill under customer `.github/skills/`.
4. Render pipeline PoC in the project repo (`tools/video/`): Playwright record → edge-tts +
   faster-whisper → FFmpeg composite → `generate-clip --definition clip.json`.
5. Deliverable: one finished MP4 + documented friction notes.

### Phase B — Productize in PageSeeds (#221 landed; #223 E2E validated; skill #222)

1. Finalize this spec from PoC learnings.
2. Embedded `video-script` skill in `crates/pageseeds-core/skills/video-script/SKILL.md`
   (version marker + input/output contract), registered in `engine/skills.rs`.
3. Free-tier desk command `video-clip-context`: emits structured article context JSON
   (body, frontmatter, keyword metadata) built with content ops helpers. Free tier per
   `docs/CLI_COMMERCIAL.md` (local reads only). The session agent turns context → clip
   definition via the `video-script` skill (deterministic context, agentic prose — the
   canonical hybrid).
4. Operator-tier `video-clip-render`: spawns in-repo `video-engine/generate-clip.sh`
   (AGENTS.md subprocess-by-tier). Requires a source checkout + Node/FFmpeg on PATH;
   project owns `video.config.json` and clip JSON. Stdout contract:
   `video-engine: output=` / `video-engine: thumbnail=` (parsed by core; bare `OUTPUT:`
   is not contractual).
5. Document in `docs/TOOL_CATALOG.md` + `docs/CLI_COMMERCIAL.md`; ship with `pnpm test:cli`.
6. **Operator skill** `.agents/skills/video-clip/SKILL.md` sequences the packaged path
   and links from weekly-seo as elective post-publish only (#222).
7. **E2E gate (#223):** packaged path proven on target worktree without daystoexpiry-local
   `tools/video/` PoC tree. Quality bar matches Phase A PoC (boxed captions, hook caption,
   scanner segment, end card). Wipe-safe mirror: `~/01_code/video-clip-backup/daystoexpiry/`.

### Phase C — Optional task type + outcomes (after 3–5 videos)

1. Task type `generate_video_clip` in `config/task_definitions.rs`
   (`UserEnqueue` / `ArtifactReview` / `follow_up_policy: None`), canonical 4-step pipeline:
   deterministic context → agentic generate via `extract_with_backend::<ClipDefinition>()`
   (typed `serde` + `schemars` struct in `models/`) → deterministic write → verify.
   The task artifact is the clip definition; rendering still runs via operator-tier tool.
2. Outcome tracking: reuse `insert_content_outcome_result` / `list_content_outcome_results`
   for per-video retention metrics, feeding "which moments hold attention" back into the
   desk model.
3. Only then consider video as a hard action in the weekly-seo desk model.

### Phase D — YouTube publish (#228)

YouTube-only MVP for publishing rendered shorts. No TikTok/Instagram. No Rust changes.

1. **`video-engine/publish.py`** — stdlib only (`urllib`). CLI:
   `publish.py <clip.json> --platforms youtube [--video <mp4>] [--dry-run]`.
2. **Payload:** clip `packaging` block (`title`, `description`, `hashtags` → tags with `#` stripped).
3. **MP4 path:** `--video` or convention `video/clips/<slug>.json` → `video/out/<slug>.mp4`
   (fallback `video-engine/out/<slug>.mp4`).
4. **Secrets** via env-file chain (same precedence as Rust `EnvResolver`):  
   `~/.config/automation/secrets.env` → repo `.env.local` → repo `.env` → process env.  
   Keys: `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET`, `YOUTUBE_REFRESH_TOKEN`.
5. **Upload:** OAuth refresh_token grant → YouTube Data API v3 resumable upload;
   `privacyStatus=private` until the OAuth app is verified.
6. **`--dry-run`:** prints request plan JSON (no network; secrets not required).
7. **Skill:** `.agents/skills/video-clip/SKILL.md` may offer optional publish after the
   quality gate — **always ask first**; report the returned URL. Live upload is verified
   owner-side (not CI). Auth setup: `video-engine/README.md` Publish section.

## Non-goals

- Commercial free/paid tools remain pure Rust — no Playwright/FFmpeg/Node on that surface.
  Operator-tier may spawn the in-repo `video-engine/` (AGENTS.md subprocess-by-tier;
  `docs/CLI_COMMERCIAL.md` Operator tier). Do not treat operator render as a commercial promise.
- No new task type or handler in Phases A–B — skill first, per AGENTS.md (prefer skill over task type).
  (Phase C is the optional task-type lane.)
- No scheduler, batch runner, or cross-project orchestration.
- No AI avatar PiP in v1 (revisit after retention data exists).

## Success criteria (PoC / Phase A)

- One publishable 40–50s vertical MP4 from a real daystoexpiry blog post.
- Playwright recording repeatable on the mocked demo portfolio.
- Captions + voiceover clean and timed.
- Process documented and ≥70–80% scripted; clear path from one video to a batch of five.

## Success criteria (Phase B — packaged path)

- [x] Toolchain on main (#221): `video-clip-context`, `video-clip-render`, `video-engine/`.
- [x] Engine ↔ CLI stdout contract aligned (`video-engine: output=` / `thumbnail=`).
- [x] Context desk smoke returns body + keyword/faq + packaging_hints.
- [x] One-command render from clip JSON → exit 0 JSON with `output_path` + `duration_s`.
- [x] ffprobe 1080×1920, 40–50s, audio; visual frames: hook caption, boxed captions,
  scanner, end card — comparable to PoC backup.
- [x] Spec status Phase B validated; architecture matches operator-tier (not “never FFmpeg”).
