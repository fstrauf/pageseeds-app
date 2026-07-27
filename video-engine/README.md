# video-engine — generic clip render pipeline

Turns a **clip definition JSON** (schema in `docs/video_clip_spec.md`) into a
finished vertical MP4 (9:16, 1080×1920, 40–50s) with screen-recorded UI, TTS
voiceover, burned-in captions, hook caption, progress bar, and branded end card.

Generic: everything target-specific lives in the **target repo's**
`video.config.json`. Clip definitions (`video/clips/*.json`) and outputs
(`video/out/`) also live in the target repo. This directory holds only the
engine and its runtime.

## Prerequisites

| Tool | Install |
|---|---|
| Node + pnpm | repo standard |
| ffmpeg + ffprobe | `brew install ffmpeg` |
| Playwright chromium | reuses a cached browser (`~/Library/Caches/ms-playwright`) or `npx playwright install chromium` |
| Python 3 venv | see setup below |

One-time engine setup:

```bash
cd video-engine
pnpm install            # playwright-core
python3 -m venv .venv
.venv/bin/pip install edge-tts pillow
```

## Run

Standalone:

```bash
./generate-clip.sh <target-repo-path> <clip-json-path> [--skip-server-check]
# e.g.
./generate-clip.sh ~/01_code/call-analyzer-video-clip ~/01_code/call-analyzer-video-clip/video/clips/best-stocks-csp.json
```

Via the CLI (preferred): `pageseeds-cli video-clip-render` shells out to this
script and parses the stdout contract below.

Steps can also be run individually:

```bash
node record.mjs <clip.json> --config <target-repo-or-config> [--out <dir>] [--check]
.venv/bin/python voice.py <clip.json> [--config ...] [--out <dir>]
.venv/bin/python composite.py <clip.json> [--config ...] [--out <dir>]
```

## Stdout / exit-code contract (for pageseeds-cli)

Only lines starting with `video-engine: ` are contractual; everything else is
free-form log output.

```
video-engine: stage=<record|voice|composite> status=start
video-engine: stage=<record|voice|composite> status=ok
video-engine: output=<absolute path to final mp4>
video-engine: thumbnail=<absolute path to thumbnail jpg>
```

On failure, exactly one stderr line and a stage-specific exit code:

```
video-engine: stage=<stage> status=error message=<one_line_underscored_message>

0 success · 2 bad args/config · 3 dev servers unreachable
4 record failed · 5 voice failed · 6 composite failed
```

## video.config.json schema (per target repo, at repo root)

| Field | Type | Description |
|---|---|---|
| `schema_version` | int | Currently `1`. |
| `project_id` | string | Project identifier (matches PageSeeds project). |
| `base_url` | string | Origin the target dev server serves, e.g. `http://localhost:3000`. |
| `dev_servers[]` | object[] | How to start the target app: `name`, `command`, `cwd` (relative to repo root), `port`, optional `ready_path` (used for the pre-flight reachability check against `base_url`). |
| `brand.domain` | string | Domain shown big on the end card. |
| `brand.progress_bar_color` | `[r,g,b]` | Top progress bar color. |
| `brand.end_card` | object | `bg`, `accent`, `text`, `muted` (all `[r,g,b]`), `subtitle` (string, may be empty). |
| `brand.thumbnail_ui_target` | string | Optional. `ui_target` name whose segment midpoint is used for the thumbnail frame. Prefer clip `packaging.thumbnail_hint` as numeric seconds when set; otherwise this; else video midpoint. |
| `tts` | object | Optional. `voices` (edge-tts voice fallbacks, first that works wins), `target_s` (voiceover target seconds, default 42.5). |
| `overlays.hide_css` | string | CSS selector list hidden during recording (dev overlays, chat widgets). |
| `overlays.dismiss_button_pattern` | string | Regex for buttons to best-effort click away (cookie/consent/info banners). |
| `overlays.block_routes` | string | Regex for request URLs to abort (third-party widgets). |
| `ui_targets` | object | Map of `ui_target` name → target definition, see below. |

### `ui_targets.<name>`

| Field | Type | Description |
|---|---|---|
| `path` | string | URL path appended to `base_url`. |
| `builtin` | string | If set (e.g. `"end_card"`), the target is rendered in composite, never recorded. |
| `ready[]` | object[] | Ordered steps run before the recording window starts: `{"wait_text": "..."}`, `{"scroll_to": "<css>"}`, `{"scroll_to_text": "..."}`, `{"click_role": {"role": "button", "name": "...", "exact": false}}`, `{"sleep_ms": 500}`. |
| `motion` | string | Motion preset during the recording window: `dwell` (stay put + wander), `dwell_scroll` (gentle down-scrolls), `slow_scroll` (scroll to `dwell_text`, dwell there). |
| `dwell_text` | string | `slow_scroll`: text to scroll toward and dwell on. |
| `input_tweak` | object | `slow_scroll`: best-effort input edit while dwelling: `{"selector": "<css>", "index": 0, "value": "30"}`. |
| `hover_text` | string | `dwell`: exact text to hover the mouse over. |
| `interactions[]` | object[] | `dwell`: best-effort mid-window actions: `{"click_role": {...}, "sleep_after_ms": 1000}`. |

The timing_map in the clip JSON references these names via `ui_target`.
`end_card` is the built-in branded end card (config `brand`), always available.

## Onboarding a new target repo

1. Add `video.config.json` at the repo root (copy `days_to_expiry`'s as a
   template; adjust `base_url`, `dev_servers`, `brand`, `overlays`,
   `ui_targets`).
2. Add clip definitions under `video/clips/`.
3. Add `video/out/` to the repo's `.gitignore`.
4. Validate without recording: `node record.mjs <clip.json> --config <repo> --check`.
5. Start the repo's dev servers, then `./generate-clip.sh <repo> <clip.json>`.

## How it works

1. **record.mjs** (Playwright): per timing_map segment, opens the target page
   at 1080×1920, runs ready steps, performs the motion preset for
   `to_s - from_s + 4s` while recording. Writes ready offsets so composite can
   trim past page load.
2. **voice.py**: edge-tts → `voice.mp3` + word-level `voice.srt`; retries with
   a computed speech rate if the voiceover lands outside 40–45.5s.
3. **composite.py**: scales segment durations to actual voiceover length,
   trims/concats recordings, renders captions (word cues grouped into ≤3-word
   phrases, each with a semi-opaque dark rounded-rect background), the hook
   keyword caption (first 3s), branded end card and progress bar as PNG
   overlays via **Pillow** (no ffmpeg drawtext dependency), muxes
   loudness-normalized voiceover, exports MP4 + thumbnail.

## Publish (YouTube)

Phase D (#228): optional upload of a rendered short to YouTube. **YouTube only** —
no TikTok/Instagram. Stdlib Python (`urllib`); no extra pip deps.

```bash
# Dry-run (no network; secrets not required)
.venv/bin/python publish.py <clip.json> --platforms youtube --dry-run

# Real upload
.venv/bin/python publish.py <clip.json> --platforms youtube [--video path/to.mp4]
```

- **Metadata** comes from the clip `packaging` block (`title`, `description`, `hashtags`).
- **Video path:** `--video`, else `…/video/out/<slug>.mp4` when the clip lives under
  `video/clips/`, else `video-engine/out/<slug>.mp4`.
- **Stdout:** JSON (`status=ok` + `url`, or `status=dry_run` plan).
- **Exit codes:** `0` ok · `1` upload/API failed · `2` bad args / config / missing secrets.

### Auth setup (one-time)

1. Google Cloud project → enable **YouTube Data API v3**.
2. Create **OAuth client** type **Desktop / installed app**.
3. Mint a refresh token once (OAuth playground or a small installed-app consent flow)
   with scope `https://www.googleapis.com/auth/youtube.upload`.
4. Store credentials (highest priority path shown; same chain as PageSeeds secrets):

```bash
# ~/.config/automation/secrets.env
YOUTUBE_CLIENT_ID=....apps.googleusercontent.com
YOUTUBE_CLIENT_SECRET=...
YOUTUBE_REFRESH_TOKEN=...
```

Fallback files (first file wins per key, then process env): repo `.env.local`, repo `.env`.

Uploads use **`privacyStatus=private`** until the OAuth app is verified by Google
(unverified apps cannot set public without quota/app review friction). Change privacy
in YouTube Studio after upload if needed.

Missing credentials print a one-line stderr hint and exit `2` (no stacktrace).

## Known limitations

- Text is rendered with Pillow to PNGs and overlaid (the local ffmpeg build
  has no libfreetype/drawtext): no caption fades/animation.
- Captions are phrase-level; word tokens carry no punctuation, so a phrase can
  occasionally span a sentence boundary.
- Recording depends on dev-server responsiveness; ready waits use 120s
  timeouts to absorb dev compiles.
- Motion presets are intentionally coarse; encode per-page behavior via
  `ui_targets` interactions rather than new code where possible.
- Voice quality/rate is validated by duration only, not by listening.
