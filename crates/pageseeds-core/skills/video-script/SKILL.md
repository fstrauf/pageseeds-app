# Video Script Skill

<!-- skill-version: 2 -->

Turn one article into a **clip definition JSON** (schema v1, docs/video_clip_spec.md)
for the external video render engine.

## Input

Run the free desk read first:

```
pageseeds-cli video-clip-context -S <slug>
```

You get structured JSON: `project_id`, `slug`, `title`, `h1`, `file_path`,
`published_at`, `status`, `word_count`, `frontmatter` (`target_keyword`,
`description`, `summary`, `canonical`, `faq`, `last_updated`), `body`
(frontmatter stripped), `site_base_url`, `packaging_hints` (`hashtags`,
`canonical_url`). Use `project_id` for clip `source.project_id`.

## Output

Write one clip definition file at `video/clips/<slug>.json` in the project repo
(engine SoT — not bare `clips/`) matching **schema v1**. Required shape:

```json
{
  "schema_version": 1,
  "source": { "project_id": "<id>", "slug": "<slug>", "title": "<title>", "content_path": "<file_path>" },
  "spoken_script": "…",
  "keywords": ["hook keyword first", "…"],
  "timing_map": [
    { "from_s": 0, "to_s": 4, "moment_template": "income_snapshot", "caption_text": "…", "ui_target": "…" }
  ],
  "cta": { "text": "…", "url": "…" },
  "packaging": { "title": "…", "description": "…", "hashtags": ["#…"], "thumbnail_hint": "…" }
}
```

### `spoken_script` (100–120 words)

- **Hook first**: the first sentence is the hook — a pain, a bold claim, or a
  number. It plays over the first 3 seconds with the big caption.
- One clear takeaway; soft CTA in the last sentence.
- **TTS-safe plain text**: no markup, no markdown, no abbreviations.
  - Write numbers as words ("three filters", "forty-two percent").
  - Write URLs phonetically ("daystoexpiry dot com").
- Ground every claim in the article body — never invent figures.

### `keywords` (4–6)

- From `frontmatter.target_keyword` first, then article phrases.
- **Hook keyword first** — it is the big caption in the first 3 seconds.

### `timing_map` (4–6 segments)

- Contiguous `from_s`/`to_s` starting at 0; total 40–50s covering the voiceover.
- `moment_template` is one of: `income_snapshot`, `scanner_highlight`,
  `calculator_demo`, `expiry_decision`, `ai_ask`.
- First segment (0–4s): visually strong moment + hook caption.
- `caption_text`: short on-screen phrase per segment.
- `ui_target`: logical UI view name the project's Playwright journey understands.
- Last segment: end card with the CTA.

**Choosing `ui_target`s (topic matching, not vibes):** the project's
`video.config.json` carries a `description` and `topics` list per target —
a capability map of what the app can do and where it lands. Read them, then
match the article's `target_keyword` and core topic to the targets whose
`topics` overlap. The hook segment goes to the strongest match (e.g. a decaf
article → the directory whose search can type "decaf", not a generic library
page). Prefer targets with real interactions (search boxes, filters, toggles)
over static pages. Name the topic → target mapping in your draft output so the
operator can sanity-check it.

### `cta` + `packaging`

- `cta.url`: prefer `packaging_hints.canonical_url`.
- `cta.subtitle`: always set — short reinforcement line under the end-card
  domain (e.g. "free 21 DTE guide + DTE tracking"). Per-clip; beats the
  config brand default.
- `packaging.hashtags`: start from `packaging_hints.hashtags`, add 1–2
  topic tags if needed.
- `packaging.thumbnail_hint`: the moment/frame worth thumbnailing.
- **`packaging.description` — long-form YouTube SEO** (same string field;
  not a 1–2 sentence teaser). Draft from the **article body** / FAQ — same
  substance as the script; no invented figures; never generic marketing fluff.

  Structure the multi-paragraph string as:

  1. **Lines 1–2 (above the fold):** hook sentence + **full canonical article
     URL** (`packaging_hints.canonical_url` / `cta.url`). The URL must appear
     in the first two **non-empty** lines (hook, then URL on the next line —
     no blank line between them).
  2. **Body (~150–300 words):** condensed facts, steps, and data from the
     article. When `frontmatter.target_keyword` is present, put it in the
     **first ~150 characters** of the description.
  3. **CTA block:** product/site link(s) from context as free-form text inside
     the description string (no new JSON fields).
  4. **Footer:** 3–5 hashtags (may mirror `packaging.hashtags`) + canonical
     URL again.

  **Length:** hard limit 5,000 characters (YouTube). Soft target band roughly
  1,500–4,000 when the article has enough substance; shorter only if the
  source is thin — never pad. See `docs/video_clip_spec.md` § packaging.description.

## Then render (operator tier)

```
pageseeds-cli video-clip-render --clip video/clips/<slug>.json
```

Requires node/ffmpeg and the project's `video.config.json`
(docs/CLI_COMMERCIAL.md "Operator tier"). For the full operator runbook
(config gate, servers, ffprobe/frame quality gate, packaging report), use
`.agents/skills/video-clip/SKILL.md` (`/video-clip`).
