# Video Clip Generation Spec

Tracking issue: [#220](https://github.com/fstrauf/pageseeds-app/issues/220) · Phase B toolchain: [#221](https://github.com/fstrauf/pageseeds-app/issues/221) · E2E: [#223](https://github.com/fstrauf/pageseeds-app/issues/223) · Phase D publish: [#228](https://github.com/fstrauf/pageseeds-app/issues/228) · TikTok inbox: [#231](https://github.com/fstrauf/pageseeds-app/issues/231) · Instagram Reels: [#232](https://github.com/fstrauf/pageseeds-app/issues/232)
Status: **Phase A complete**; **Phase B landed** (#221) and **Phase B validated**
(#223, 2026-07-27) — one-command packaged path on `days_to_expiry` video worktree:
`video-clip-render -p <worktree> --clip video/clips/best-stocks-csp.json` → exit 0,
`output_path` …/video/out/best-stocks-csp.mp4, `duration_s` ≈ 42.57, ffprobe
1080×1920 + audio. Context smoke: `video-clip-context -S best-stocks-csp` returns
body + `frontmatter.faq` / `target_keyword` + `packaging_hints`.
**Phase D (#228 + #231 + #232):** `video-engine/publish.py` — YouTube resumable upload + TikTok
**Inbox Upload** (drafts via Content Posting API `video.upload`) + Instagram Graph API
**Reels** (resumable container + rupload; stdlib urllib; optional skill step).
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
| `schema_version` | integer | Currently `1`. Renderers must reject unknown versions. Additive optional fields (e.g. `published`) do not bump the version. |
| `source` | object | `{ project_id, slug, title, content_path }` — the origin article. |
| `spoken_script` | string | 35–45s spoken voiceover. Strong hook in the first 3 seconds, one clear takeaway, soft CTA at the end. Plain text, no markup. |
| `keywords` | string[] | 4–6 target phrases from the article. First entry is the hook keyword shown as a big caption in the first 3 seconds. |
| `timing_map` | object[] | Ordered segments, see below. Total duration must cover the voiceover length (target 40–50s final cut). |
| `cta` | object | `{ text, url, subtitle? }` — end card call to action. `subtitle` (optional) renders under the domain and is per-clip; it overrides the config `brand.end_card.subtitle` default. |
| `packaging` | object | `{ title, description, hashtags, thumbnail_hint }` — upload metadata. `description` is long-form SEO copy grounded in the article (see below), not a 1–2 sentence teaser. |
| `published` | object (optional) | Post-upload platform linkage. Platform keys (`youtube`, `tiktok`, `instagram`, …). Written by `publish.py` after a successful upload; absent until then. |

### `packaging.description`

Same string field as schema v1 — richer semantics for YouTube SEO. Craft
rules live in `crates/pageseeds-core/skills/video-script/SKILL.md`; quality
gates in `.agents/skills/video-clip/SKILL.md`.

| Zone | Content |
|---|---|
| **Lines 1–2 (above the fold)** | Hook sentence + **full canonical article URL** (`packaging_hints.canonical_url` / `cta.url`). URL must appear in the first two **non-empty** lines (hook on line 1, URL on line 2 — no blank line between them). |
| **Body** | ~150–300 words condensed from the article (facts, steps, data grounded in `body` / FAQ — same substance as the script; no invented figures). Put `frontmatter.target_keyword` in the **first ~150 characters** when present. |
| **CTA block** | Product/site link(s) from context — free-form text inside the description string, not new JSON fields. |
| **Footer** | 3–5 hashtags (may mirror `packaging.hashtags`) + canonical URL again. |

**Limits**

| Limit | Rule |
|---|---|
| Hard | **5,000 characters** (YouTube). `publish.py` warns on stderr if over; does not hard-truncate or fail. |
| Soft target | Roughly **1,500–4,000 chars** when the article has enough substance. |
| Thin source | Shorter only if the article is thin — never pad with fluff. Empty description is invalid. |
| Soft warn band | Operator quality gate flags descriptions under ~800 chars as "too thin". |

Draft from **article body**, not generic marketing blurbs. The description
should let a searcher/reader get value without watching the short.

### `timing_map` segment

| Field | Type | Description |
|---|---|---|
| `from_s` / `to_s` | number | Segment bounds in seconds, contiguous from 0. |
| `moment_template` | string | One of the moment templates below. |
| `caption_text` | string | On-screen caption for the segment (short phrase; renderer handles word-level timing from whisper). |
| `ui_target` | string | Logical name of the UI view/element to show (e.g. `income_dashboard`, `scanner_results`, `put_calculator`). Interpreted by the project's Playwright journey. |

### `published.youtube` (post-upload)

Written by `video-engine/publish.py` after a successful (non-dry-run) YouTube
upload. Re-publish overwrites this object (no multi-version history). Dry-run
does not touch the clip file. Stdout success JSON includes the same block.

| Field | Type | Description |
|---|---|---|
| `video_id` | string | YouTube video id from the upload response. |
| `url` | string | Canonical short URL, `https://youtu.be/{video_id}`. |
| `published_at` | string | ISO-8601 UTC timestamp of the successful upload write-back. |
| `privacy` | string | Upload privacy (`private` until OAuth app is verified; see #234). |

### `published.tiktok` (post-upload, #231)

Written by `video-engine/publish.py` after a successful (non-dry-run) TikTok
**Inbox Upload**. The video lands in the creator’s TikTok inbox/drafts; the human
finishes caption, privacy, and post in the TikTok app. Re-publish overwrites
this object. Dry-run does not touch the clip file. Other `published.*` keys
are preserved on write-back.

| Field | Type | Description |
|---|---|---|
| `publish_id` | string | TikTok publish id from inbox init (`data.publish_id`). |
| `mode` | string | Always `"inbox"` for this adapter (not Direct Post). |
| `published_at` | string | ISO-8601 UTC timestamp of the successful upload write-back. |
| `note` | string | Human note, e.g. `"finish in TikTok app"`. |

### `published.instagram` (post-upload, #232)

Written by `video-engine/publish.py` after a successful (non-dry-run) Instagram
**Reels** publish via Graph API resumable upload. Re-publish overwrites this
object. Dry-run does not touch the clip file. Other `published.*` keys are
preserved on write-back.

| Field | Type | Description |
|---|---|---|
| `media_id` | string | Instagram media id from `media_publish` (`id`). |
| `url` | string | Permalink from `GET /{media_id}?fields=permalink`, or `""` if unavailable. |
| `published_at` | string | ISO-8601 UTC timestamp of the successful upload write-back. |

### Article embed (skill-owned, not engine)

After **YouTube** publish, the operator skill (`.agents/skills/video-clip/SKILL.md`) may
insert a portable responsive iframe embed into the source MDX body at
`source.content_path` (customer content only). Position: after the intro /
first paragraph, before the first `## ` H2. Idempotent on
`youtube.com/embed/{video_id}` / `youtu.be/{video_id}`. Not part of the render
engine; no `LazyYouTubeEmbed` requirement. **Do not** auto-embed TikTok or Instagram
into MDX.

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
    "description": "Cash-secured puts can print income — or blow up accounts when you pick the wrong underlyings. Here are the three filters I run before every CSP, plus the one ticker clearing all three right now.\nhttps://daystoexpiry.com/blog/best-stocks-cash-secured-puts-2026\n\nFilter one: liquidity. I want tight bid-ask spreads and enough open interest that filling (and rolling) is not a lottery. Filter two: fundamental quality — businesses I would actually own if assigned, not lottery tickets. Filter three: options premium that pays for the risk without chasing meme IV crush.\n\nIn the full guide I walk through how to score a shortlist, what fails each filter in practice, and how a portfolio scanner surfaces names that pass all three in one pass. You will also see a put calculator moment: inputs in, expected credit and assignment price out, so the risk is concrete before you click sell.\n\nIf you are new to the wheel, start with cash-secured puts on names you already understand, size so assignment is survivable, and treat the premium as compensation for obligation — not free money.\n\nFull stock list, filter checklist, and free DTE tracking:\nhttps://daystoexpiry.com/blog/best-stocks-cash-secured-puts-2026\n\n#options #cashsecuredputs #wheelstrategy #passiveincome #putselling\n\nhttps://daystoexpiry.com/blog/best-stocks-cash-secured-puts-2026",
    "hashtags": ["#options", "#cashsecuredputs", "#wheelstrategy", "#passiveincome"],
    "thumbnail_hint": "scanner_results top row"
  }
}
```

Optional post-upload block (written by `publish.py`; not present at craft time):

```json
{
  "published": {
    "youtube": {
      "video_id": "dQw4w9WgXcQ",
      "url": "https://youtu.be/dQw4w9WgXcQ",
      "published_at": "2026-07-27T12:00:00Z",
      "privacy": "private"
    },
    "tiktok": {
      "publish_id": "v_inbox_file~v2.123456789",
      "mode": "inbox",
      "published_at": "2026-07-27T12:05:00Z",
      "note": "finish in TikTok app"
    },
    "instagram": {
      "media_id": "17895695668004550",
      "url": "https://www.instagram.com/reel/ABC123/",
      "published_at": "2026-07-27T12:10:00Z"
    }
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
7. Report MP4 path + packaging block; optional YouTube / TikTok inbox / Instagram Reels
   publish via `video-engine/publish.py` (ask first per platform).
8. After successful YouTube publish: clip gets `published.youtube` write-back; skill embeds
   the short into source MDX (see skill post-publish section). TikTok / Instagram write-back
   is `published.tiktok` / `published.instagram` only — no MDX embed for those platforms.

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

### Phase D — YouTube (#228) + TikTok inbox (#231) + Instagram Reels (#232) + post-publish linkage (#235)

Publish adapters for rendered shorts. No Rust changes. Platforms:
`youtube`, `tiktok`, `instagram`. TikTok is **Inbox Upload only** (drafts); not
Direct Post / `video.publish`. Instagram uses Graph API **Reels** resumable
upload (local MP4 via rupload — no public host required).

1. **`video-engine/publish.py`** — stdlib only (`urllib` / `http.client`). CLI:
   `publish.py <clip.json> --platforms youtube[,tiktok][,instagram] [--video <mp4>] [--dry-run]`.
   Soft multi-platform runner: each platform independent; failure of one does not skip
   the other. Exit `0` all ok · `1` any platform failed · `2` bad args / unknown platform /
   unresolvable paths.
2. **Payload:**
   - **YouTube:** clip `packaging` → snippet/tags (`hashtags` with `#` stripped).
   - **TikTok inbox:** packaging is operator preview only; inbox init body is
     `source_info` only (`FILE_UPLOAD`, `video_size`, `chunk_size`, `total_chunk_count`) —
     **no** `post_info` / title / privacy.
   - **Instagram Reels:** packaging → caption (title + description + `#hashtags`,
     hard limit 2200 chars).
3. **MP4 path:** `--video` or convention `video/clips/<slug>.json` → `video/out/<slug>.mp4`
   (fallback `video-engine/out/<slug>.mp4`).
4. **Secrets** via env-file chain (same precedence as Rust `EnvResolver`):  
   `~/.config/automation/secrets.env` → repo `.env.local` → repo `.env` → process env.  
   YouTube: `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET`, `YOUTUBE_REFRESH_TOKEN`.  
   TikTok: `TIKTOK_CLIENT_KEY`, `TIKTOK_CLIENT_SECRET`, `TIKTOK_REFRESH_TOKEN`.  
   Instagram: `META_ACCESS_TOKEN`, `IG_USER_ID` (optional session extend:
   `META_APP_ID`, `META_APP_SECRET`).
5. **YouTube upload:** OAuth refresh_token grant → Data API v3 resumable upload;
   `privacyStatus=private` until the OAuth app is verified.
6. **TikTok inbox upload (#231):** OAuth refresh →
   `POST /v2/post/publish/inbox/video/init/` → `PUT` chunks to `upload_url` → optional
   `POST /v2/post/publish/status/fetch/`. Scope **`video.upload`** only. Creator finishes
   in the TikTok app (~5 pending API shares / 24h).
7. **Instagram Reels upload (#232):** optional long-lived token exchange →
   `POST /{ig-user-id}/media` (`media_type=REELS`, `upload_type=resumable`, `caption`) →
   `POST rupload.facebook.com/ig-api-upload/{version}/{container_id}` (raw MP4) →
   poll `status_code` until `FINISHED` → `POST /{ig-user-id}/media_publish` → optional
   permalink. Permission `instagram_content_publish`.
8. **`--dry-run`:** prints request plan JSON per platform (no network; secrets not required;
   does **not** write `published` into the clip file).
9. **Clip write-back:** on success, merge platform key into `published` (indent=2 +
   trailing newline) without wiping siblings:
   - `published.youtube` — `video_id`, `url`, `published_at`, `privacy` (#235)
   - `published.tiktok` — `publish_id`, `mode: "inbox"`, `published_at`, human `note`
   - `published.instagram` — `media_id`, `url` (permalink or empty), `published_at`
10. **Skill:** `.agents/skills/video-clip/SKILL.md` may offer optional YouTube, TikTok,
    and/or Instagram publish after the quality gate — **always ask first**. YouTube:
    report URL + embed into source MDX. TikTok: report `publish_id` + “finish in TikTok
    app”; **no** MDX embed. Instagram: report `media_id` + permalink; **no** MDX embed.
    Live upload is verified owner-side (not CI). Auth: `video-engine/README.md`.

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
