# video-engine — generic clip render pipeline

Turns a **clip definition JSON** (schema in `docs/video_clip_spec.md`) into a
finished vertical MP4 (9:16, 1080×1920, 40–50s) with screen-recorded UI, TTS
voiceover, burned-in captions, hook caption, progress bar, and branded end card.

Generic: everything target-specific lives in the **target repo's**
`video.config.json`. Clip definitions (`video/clips/*.json`) also live in the
target repo. **Outputs are publish artifacts, not repo content** — they go to
`~/01_code/video-clip-backup/<project_id>/` (central, outside any repo, safe
from automation git-cleans; config `output_dir` overrides). This directory
holds only the engine and its runtime.

## Prerequisites

| Tool | Install |
|---|---|
| Node + pnpm | repo standard |
| ffmpeg + ffprobe | `brew install ffmpeg` |
| Playwright chromium | `playwright-core` ≥1.59 (pinned in `package.json`); browser via `pnpm exec playwright-core install chromium` (cached under `~/Library/Caches/ms-playwright`) |
| Python 3 venv | see setup below |

One-time engine setup:

```bash
cd video-engine
pnpm install            # playwright-core (≥1.59, pinned)
pnpm exec playwright-core install chromium   # browser build matching the pin
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
| `motion` | string | Motion preset during the recording window: `dwell` (stay put + wander), `dwell_scroll` (gentle down-scrolls), `slow_scroll` (scroll to `dwell_text`, dwell there), `agentic` (reuse pre-placed segment media from operator MCP take; see below). |
| `agentic_goal` | string | **Required when `motion` is `agentic`.** Free-text intent the operator agent executes (caption + UI goal). `ready[]` remains the scripted fallback path. |
| `dwell_text` | string | `slow_scroll`: text to scroll toward and dwell on. |
| `input_tweak` | object | `slow_scroll`: best-effort input edit while dwelling: `{"selector": "<css>", "index": 0, "value": "30"}`. |
| `hover_text` | string | `dwell`: exact text to hover the mouse over. |
| `interactions[]` | object[] | `dwell`: best-effort mid-window actions: `{"click_role": {...}, "sleep_after_ms": 1000}`. |

The timing_map in the clip JSON references these names via `ui_target`.
`end_card` is the built-in branded end card (config `brand`), always available.

### Agentic motion (`motion: "agentic"`)

Optional per-`ui_target` path for natural, resilient UI footage. **Scripted motions remain the default.** The engine never runs an LLM or MCP session — the operator skill produces takes; `record.mjs` only **reuses** or **falls back**.

| Step | Who | What |
|------|-----|------|
| 1. Pre-pass | Operator skill (`/video-clip`) | For each timing_map segment whose `ui_target` has `motion: "agentic"`, record via host Playwright MCP (or `kimi -p` with the recording profile), run `trim_deadair.py`, place media under `~/01_code/video-clip-backup/<project_id>/segments/seg{NN}_{ui_target}.mp4` |
| 2. Record | `record.mjs` | If that **`.mp4`** exists and is usable (≥ ~10 KB) → **reuse** it (`ready_offset_s: 0`), do not overwrite. Sibling `.webm` (e.g. leftover scripted fallback) is **ignored** for agentic reuse. Else → **scripted fallback** writes `.webm` (`dwell` if `interactions` / `hover_text` present, else `dwell_scroll`) and log clearly |
| 3. Rest | voice → composite | Unchanged |

**Config validation:** `node record.mjs … --check` exits 2 if any non-builtin target has `motion: "agentic"` and missing/empty `agentic_goal`.

**MCP recording profile:** copy [`mcp-recording.example.json`](./mcp-recording.example.json) into your host MCP config (Kimi/Grok/etc.). Do **not** auto-mutate `~/.kimi-code/mcp.json` from product code. Key args:

| Arg | Why |
|-----|-----|
| `--viewport-size 1080x1920` | Must be set at **startup** — mid-session resize leaves video at the small default (e.g. 800×450) |
| `--headless` + `--isolated` | Clean recording sessions |
| `--blocked-origins crisp.chat;intercom;fullstory;hotjar;…` | Aligns with target `overlays.block_routes` intent (third-party chat/analytics) |
| `--output-dir …/video-clip-backup/<project_id>/agentic/` | Staging for raw MCP takes before trim + move into `segments/` |

**Dead-air trim:** MCP takes often freeze while the agent thinks. After recording, run:

```bash
.venv/bin/python trim_deadair.py <raw-take.webm> --out \
  ~/01_code/video-clip-backup/<project_id>/segments/seg{NN}_{ui_target}.mp4
```

Then run the normal pipeline (`generate-clip.sh` / `video-clip-render`). Composite still scales segment duration to voiceover (`-t`); raw MCP duration may overrun the brief — trim first, then let composite scale. Full operator protocol: `.agents/skills/video-clip/SKILL.md` (agentic segment path).

## Onboarding a new target repo

1. Add `video.config.json` at the repo root (copy `days_to_expiry`'s as a
   template; adjust `base_url`, `dev_servers`, `brand`, `overlays`,
   `ui_targets`; optional `output_dir` to override the central default).
2. Add clip definitions under `video/clips/`.
3. Validate without recording: `node record.mjs <clip.json> --config <repo> --check`.
4. Start the repo's dev servers, then `./generate-clip.sh <repo> <clip.json>`
   — outputs land in `~/01_code/video-clip-backup/<project_id>/`.

## How it works

1. **record.mjs** (Playwright): per timing_map segment, opens the target page
   at 1080×1920 and runs ready steps — this warmup is **not** filmed. Recording
   then starts via `page.screencast` (Playwright ≥1.59) and the motion preset
   runs for `to_s - from_s + 4s`. `ready_offset_s` in `segments.json` is
   therefore always `0`. Segments with locator-driven steps (`click_role`,
   `scroll_to_text`, `input_tweak`, `hover_text`, `interactions`) get
   `showActions` cursor/highlight annotations (bottom-right); dwell segments
   keep the freehand mouse wander. For `motion: "agentic"`, reuses a pre-placed
   `.mp4` under `OUT/segments/` when present (no overwrite; sibling `.webm` is
   ignored); otherwise scripted fallback writes `.webm` — see
   [Agentic motion](#agentic-motion-motion-agentic). Browser launch is lazy
   (skipped entirely when every non-builtin segment reuses agentic media).
2. **voice.py**: edge-tts → `voice.mp3` + word-level `voice.srt`; retries with
   a computed speech rate if the voiceover lands outside 40–45.5s.
3. **composite.py**: scales segment durations to actual voiceover length,
   trims/concats recordings, renders captions (word cues grouped into ≤3-word
   phrases, each with a semi-opaque dark rounded-rect background), the hook
   keyword caption (first 3s), branded end card and progress bar as PNG
   overlays via **Pillow** (no ffmpeg drawtext dependency), muxes
   loudness-normalized voiceover, exports MP4 + thumbnail.

## Publish (YouTube + TikTok inbox + Instagram Reels)

Phase D: optional upload of a rendered short via `publish.py`. Stdlib Python
(`urllib` / `http.client`); no extra pip deps. Platforms:

| Platform | Mode | Issue |
|---|---|---|
| **YouTube** | Data API v3 resumable upload (`privacyStatus=private`) | #228 |
| **TikTok** | Content Posting API **Inbox Upload** (drafts; user finishes in app) | #231 |
| **Instagram** | Graph API **Reels** (Instagram Login `video_url` / stage, or FB Login rupload + `media_publish`) | #232 |

**Not supported:** TikTok Direct Post (`video.publish`).

```bash
# Dry-run (no network; secrets not required)
.venv/bin/python publish.py <clip.json> --platforms youtube --dry-run
.venv/bin/python publish.py <clip.json> --platforms tiktok --dry-run
.venv/bin/python publish.py <clip.json> --platforms instagram --dry-run
.venv/bin/python publish.py <clip.json> --platforms youtube,instagram --dry-run
.venv/bin/python publish.py <clip.json> --platforms youtube,tiktok --dry-run

# Real upload
.venv/bin/python publish.py <clip.json> --platforms youtube [--video path/to.mp4]
.venv/bin/python publish.py <clip.json> --platforms tiktok [--video path/to.mp4]
.venv/bin/python publish.py <clip.json> --platforms instagram [--video path/to.mp4]
.venv/bin/python publish.py <clip.json> --platforms youtube,instagram [--video path/to.mp4]
.venv/bin/python publish.py <clip.json> --platforms youtube,tiktok [--video path/to.mp4]
```

- **Idempotency:** the clip's `published` block is the ledger. A platform with
  an existing `published.<platform>.url` is **skipped** (`status: "skipped"`,
  `reason: "already_published"`) — re-running a publish pass is always safe.
  Pass `--force` to republish a platform deliberately.
- **Metadata** comes from the clip `packaging` block (`title`, `description`, `hashtags`).
  - **YouTube:** sent as snippet/tags. Description may be long-form SEO copy (see
    `docs/video_clip_spec.md`); if over 5,000 chars, `publish.py` **warns on stderr**
    and does not truncate. `--dry-run` plan JSON includes `description_chars`.
  - **TikTok inbox:** packaging is **operator preview only** — inbox init does **not**
    accept `post_info` / title / privacy. Caption is set by the creator in the app.
  - **Instagram Reels:** title + blank line + description + hashtags as `#tag`, hard
    limit **2200** chars (prefer drop trailing hashtags before hard truncate).
    `--dry-run` plan JSON includes `caption_preview` / `caption_chars`.
- **Video path:** `--video`, else the central output dir
  (`~/01_code/video-clip-backup/<project_id>/<slug>.mp4`, or config
  `output_dir`).
- **Stdout:** single-platform → one JSON object; multi-platform →
  `{"results":[...]}`. Each entry has `status=ok|dry_run|error`.
- **Clip write-back:** successful (non-dry-run) uploads merge into `published` without
  wiping other platform keys:
  - `published.youtube` — `video_id`, `url`, `published_at`, `privacy`
  - `published.tiktok` — `publish_id`, `mode` (`inbox`), `published_at`, `note`
  - `published.instagram` — `media_id`, `url` (permalink or `""`), `published_at`
  Dry-run does not mutate the file.
- **Multi-platform:** each platform runs independently; failure of one does not skip
  the other.
- **Exit codes:** `0` all succeeded (or dry-run) · `1` any platform failed after
  shared preflight · `2` bad args / unknown platform / unresolvable clip|video.

Secrets chain (same as PageSeeds `EnvResolver`):  
`~/.config/automation/secrets.env` → repo `.env.local` → repo `.env` → process env.  
First file wins per key. Missing credentials print a one-line stderr hint (no stacktrace)
and mark that platform as failed.

### YouTube auth setup (one-time)

1. Google Cloud project → enable **YouTube Data API v3**.
2. Create **OAuth client** type **Desktop / installed app**.
3. Mint a refresh token once (OAuth playground or a small installed-app consent flow)
   with scopes for **upload + channel preflight**:
   - `https://www.googleapis.com/auth/youtube.upload` **and**
   - `https://www.googleapis.com/auth/youtube.readonly`
   - **or** the single full scope `https://www.googleapis.com/auth/youtube`
4. Store credentials:

```bash
# ~/.config/automation/secrets.env
YOUTUBE_CLIENT_ID=....apps.googleusercontent.com
YOUTUBE_CLIENT_SECRET=...
YOUTUBE_REFRESH_TOKEN=...
# optional multi-brand guard (title or @handle; see table below)
# YOUTUBE_CHANNEL=My Brand Channel
```

**Multi-brand:** each brand needs its **own** refresh token minted while signed into
**that** channel. Copying the default token under a namespaced key is wrong (publish
warns when `{PREFIX}YOUTUBE_REFRESH_TOKEN` equals `YOUTUBE_REFRESH_TOKEN`). Real
(non-dry-run) publish always calls `channels.list?mine=true` after token refresh and
logs the channel to stderr; if `YOUTUBE_CHANNEL` / `{PROJECT}_YOUTUBE_CHANNEL` is set
and neither title nor customUrl matches, publish hard-fails (fix the token or the
expected channel — no override flag).

| project_id | Refresh token key | Optional expected channel | Mint while signed into |
|------------|-------------------|---------------------------|------------------------|
| (default) | `YOUTUBE_REFRESH_TOKEN` | `YOUTUBE_CHANNEL` | default brand channel |
| e.g. `coffee` | `COFFEE_YOUTUBE_REFRESH_TOKEN` | `COFFEE_YOUTUBE_CHANNEL` | that project’s channel |

Uploads use **`privacyStatus=private`** until the OAuth app is verified by Google
(unverified apps cannot set public without quota/app review friction). Change privacy
in YouTube Studio after upload if needed.

### TikTok auth setup (one-time) — Inbox Upload only

1. [TikTok for Developers](https://developers.tiktok.com/) → create / open an app.
2. Add the **Content Posting API** product.
3. Request scope **`video.upload`** only (inbox / drafts).  
   **Do not** use `video.publish` / Direct Post for this adapter — that path is out of
   scope and requires app audit.
4. Complete OAuth (user consent) and store a **refresh token**:

```bash
# ~/.config/automation/secrets.env
TIKTOK_CLIENT_KEY=...
TIKTOK_CLIENT_SECRET=...
TIKTOK_REFRESH_TOKEN=...
```

5. **Flow:** refresh access token → `POST /v2/post/publish/inbox/video/init/` with
   `source_info` only (`FILE_UPLOAD` + size/chunk fields; **no** `post_info`) →
   `PUT` binary chunks to the returned `upload_url` → optional status fetch → write
   `published.tiktok`. The creator finishes caption, privacy, and post in the **TikTok app**
   (inbox notification).
6. **Pending-share limit:** TikTok allows roughly **~5 unposted API uploads per 24h**
   per user (`spam_risk_too_many_pending_share`). Finish or discard drafts in-app before
   uploading more.
7. **Chunk rules** (media transfer guide): files **&lt; 5 MB** must be one chunk
   (`chunk_size = video_size`); **5–64 MB** may be a single chunk; **&gt; 64 MB** multi-chunk
   (chunk 5–64 MB, final chunk up to 128 MB). See `publish.py` `tiktok_source_info`.

Live TikTok upload is verified owner-side (not CI). Never commit secrets to the repo.

### Instagram auth setup (one-time) — Graph API Reels

Two login paths are supported. **`publish.py` auto-selects from the token shape.**

| Path | Token | Graph host | Local MP4 |
|---|---|---|---|
| **Instagram Login** (recommended when Facebook Login is blocked) | starts with `IGAA…` | `graph.instagram.com` | staged to a short-lived **public** URL (Meta fetches it), or set `INSTAGRAM_VIDEO_URL` |
| **Facebook Login** | classic `EAA…` user token | `graph.facebook.com` + `rupload.facebook.com` | **resumable rupload** (no public host) |

#### A) Instagram Login (current operator default)

1. [Meta for Developers](https://developers.facebook.com/) → app → **API setup with Instagram login**.
2. Add yourself as **Instagram Tester** (App roles → Instagram Tester → accept invite in IG
   [manage access](https://www.instagram.com/accounts/manage_access/)).
3. Connect the Business/Creator account (`dte_options`, etc.) and copy:
   - **Access token** (`IGAA…`)
   - **Instagram user id** — prefer the id from `GET graph.instagram.com/me?fields=id`
     (not always the same number shown next to the Page in some UIs).
4. Store credentials:

```bash
# ~/.config/automation/secrets.env
META_ACCESS_TOKEN=IGAA...          # Instagram Login user token
IG_USER_ID=2771...                 # from /me (or API setup UI if it matches /me)
# optional — short→long exchange (refresh of long-lived tokens needs no secret):
META_APP_SECRET=...
META_APP_ID=...                    # unused for IG Login refresh; kept for FB path
```

5. **Flow** (Instagram Login):
   - Optional: refresh long-lived token via
     `GET graph.instagram.com/refresh_access_token?grant_type=ig_refresh_token&…`
     or exchange short-lived with `ig_exchange_token` when `META_APP_SECRET` is set.
   - Resolve public `video_url`: env `INSTAGRAM_VIDEO_URL` **or** stage the local MP4
     to litterbox (1h TTL) unless `INSTAGRAM_STAGE_HOST=none`.
   - Create REELS container:
     `POST graph.instagram.com/{version}/{ig-user-id}/media`
     with `media_type=REELS`, `video_url=…`, `caption=…`.
   - Poll `GET …/{container_id}?fields=status_code` until `FINISHED`.
   - Publish: `POST …/{ig-user-id}/media_publish` with `creation_id=…`.
   - Write `published.instagram` (`media_id`, `url`, `published_at`).

Optional env for staging:

| Env | Default | Meaning |
|---|---|---|
| `INSTAGRAM_VIDEO_URL` | (unset) | Skip staging; Meta fetches this HTTPS URL |
| `INSTAGRAM_STAGE_HOST` | `litterbox` | `litterbox` or `none` |
| `INSTAGRAM_STAGE_TTL` | `1h` | litterbox TTL (`1h` / `12h` / `24h` / `72h`) |
| `INSTAGRAM_STAGE_ENDPOINT` | litterbox API URL | override staging endpoint |

#### B) Facebook Login (resumable rupload, no public host)

1. Same Meta app → **Facebook Login** healthy (not “Feature Unavailable”).
2. Graph API Explorer (or OAuth) → long-lived **user** token with
   `instagram_basic`, `instagram_content_publish`, `pages_show_list`,
   `pages_read_engagement`.
3. `IG_USER_ID` from `me/accounts?fields=instagram_business_account`.
4. Store `META_ACCESS_TOKEN` + `IG_USER_ID` (+ optional `META_APP_ID` /
   `META_APP_SECRET` for `fb_exchange_token`).
5. **Flow:** resumable container on `graph.facebook.com` → binary
   `rupload.facebook.com` → poll → `media_publish` (unchanged from #232).

Live Instagram publish is owner-side (not CI). Never commit secrets to the repo.
Graph API Explorer is optional when using Instagram Login tokens.

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
