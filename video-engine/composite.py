#!/usr/bin/env python3
"""composite.py — assemble the final vertical MP4 from recorded segments.

Usage: composite.py <clip.json> [--config <video.config.json|dir>] [--out <dir>]

Pipeline:
  1. Trim per-segment recordings (<out>/segments/) to durations scaled to the
     actual voiceover length (timing_map proportions), concat into a base video.
  2. Render caption PNGs from voice.srt (word-level -> short phrases) plus the
     hook keyword caption and an end card PNG (Pillow). Every phrase caption
     gets a semi-opaque dark rounded-rectangle background for contrast.
     End-card domain/colors come from the config's "brand" section.
  3. Overlay captions + hook + top progress bar, mux loudness-normalized
     voiceover, export H.264 1080x1920 30fps ~8Mbps MP4 + thumbnail jpg.

drawtext is intentionally NOT used: the local ffmpeg build lacks libfreetype,
so all text is rendered to PNG with Pillow and overlaid.

Exit codes: 0 ok · 1 ffmpeg/pipeline failure · 2 bad args / config error
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

W, H = 1080, 1920
FPS = 30
FONT = "/System/Library/Fonts/Supplemental/Arial Bold.ttf"
END_CARD_TAIL_S = 3.0  # extra seconds the end card holds after the voice ends
HOOK_CARD_S = 1.5  # branded hook card prepended at frame 0 (the Shorts "thumbnail")
CAPTION_BOX_RGBA = (0, 0, 0, 178)  # ~70% opaque black behind phrase captions
CAPTION_BOX_RADIUS = 18
ZOOM_PCT = "0.08"  # slow push-in on alternating segments (keeps static UI alive)

DEFAULT_BRAND = {
    "domain": "example.com",
    "progress_bar_color": [140, 180, 255],
    "end_card": {
        "bg": [11, 16, 32],
        "accent": [140, 180, 255],
        "text": [235, 240, 255],
        "muted": [150, 160, 185],
        "subtitle": "",
    },
}


def run(cmd: list[str]) -> None:
    print("+", " ".join(str(c) for c in cmd[:6]), "..." if len(cmd) > 6 else "")
    subprocess.run([str(c) for c in cmd], check=True)


def ffprobe_duration(path: Path) -> float:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "csv=p=0", str(path)],
        capture_output=True, text=True, check=True,
    )
    return float(out.stdout.strip())


def resolve_config(p: str | None) -> tuple[dict, Path | None]:
    """Return (config dict, config file's parent dir for resolving relative paths)."""
    if not p:
        return {}, None
    path = Path(p).resolve()
    if path.is_dir():
        path = path / "video.config.json"
    try:
        return json.loads(path.read_text()), path.parent
    except (OSError, json.JSONDecodeError) as e:
        sys.exit(f"composite: cannot read config at {path}: {e}")


def brand_config(config: dict) -> dict:
    b = {**DEFAULT_BRAND, **config.get("brand", {})}
    b["end_card"] = {**DEFAULT_BRAND["end_card"], **config.get("brand", {}).get("end_card", {})}
    return b


# --- SRT -> phrases ------------------------------------------------------------

SRT_TS = re.compile(r"(\d+):(\d+):(\d+),(\d+)")


def parse_srt(path: Path) -> list[dict]:
    cues = []
    for block in re.split(r"\n\s*\n", path.read_text(encoding="utf-8").strip()):
        lines = [l for l in block.strip().splitlines() if l.strip()]
        if len(lines) < 3:
            continue
        m = re.findall(SRT_TS, lines[1])
        if len(m) != 2:
            continue
        to_s = lambda g: int(g[0]) * 3600 + int(g[1]) * 60 + int(g[2]) + int(g[3]) / 1000
        cues.append({"start": to_s(m[0]), "end": to_s(m[1]),
                     "text": " ".join(lines[2:]).strip()})
    return cues


def group_phrases(words: list[dict], max_words=3, max_dur=2.2) -> list[dict]:
    """Group word-level cues into short caption phrases."""
    phrases, cur = [], []
    for w in words:
        cur.append(w)
        text = " ".join(x["text"] for x in cur)
        dur = cur[-1]["end"] - cur[0]["start"]
        if len(cur) >= max_words or dur >= max_dur or re.search(r"[.,!?—:;]$", w["text"]):
            phrases.append({"start": cur[0]["start"], "end": cur[-1]["end"], "text": text})
            cur = []
    if cur:
        phrases.append({"start": cur[0]["start"], "end": cur[-1]["end"],
                        "text": " ".join(x["text"] for x in cur)})
    # avoid 1-frame gaps between consecutive phrases
    for a, b in zip(phrases, phrases[1:]):
        if b["start"] > a["end"]:
            a["end"] = b["start"]
    return phrases


# --- PNG rendering (Pillow) ------------------------------------------------------

def wrap_text(text: str, font: ImageFont.FreeTypeFont, max_w: int) -> list[str]:
    lines, cur = [], ""
    for word in text.split():
        trial = f"{cur} {word}".strip()
        if font.getbbox(trial)[2] <= max_w or not cur:
            cur = trial
        else:
            lines.append(cur)
            cur = word
    if cur:
        lines.append(cur)
    return lines


def render_caption_png(text: str, path: Path, fontsize=64, box=False) -> int:
    """Render a 1080-wide transparent caption strip. Returns canvas height.

    box=False: one semi-opaque dark rounded rect behind the whole text block
               (default for phrase captions — contrast over busy UI).
    box=True:  per-line rounded rects (kept for the extra-large hook caption).
    """
    font = ImageFont.truetype(FONT, fontsize)
    lines = wrap_text(text, font, max_w=940)
    line_h = int(fontsize * 1.32)
    pad = 24
    canvas_h = line_h * len(lines) + 2 * pad
    img = Image.new("RGBA", (W, canvas_h), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    if not box:
        # measure the widest line to size a single background block
        max_tw = max(font.getbbox(l)[2] - font.getbbox(l)[0] for l in lines)
        x0 = (W - max_tw) // 2 - 28
        x1 = (W + max_tw) // 2 + 28
        draw.rounded_rectangle([x0, 8, x1, canvas_h - 8],
                               radius=CAPTION_BOX_RADIUS, fill=CAPTION_BOX_RGBA)
    y = pad
    for line in lines:
        bbox = font.getbbox(line)
        x = (W - (bbox[2] - bbox[0])) // 2
        if box:
            draw.rounded_rectangle(
                [x - 28, y - 12, x + (bbox[2] - bbox[0]) + 28, y + line_h],
                radius=18, fill=(0, 0, 0, 130))
        draw.text((x, y), line, font=font, fill="white",
                  stroke_width=max(4, fontsize // 12), stroke_fill="black")
        y += line_h
    img.save(path)
    return canvas_h


def render_hook_card_png(clip: dict, brand: dict, path: Path, config_dir: Path | None) -> None:
    """Branded frame-0 hook card: logo, accent rule, big hook text, domain.
    Text = clip.hook_text (optional, punchy) else keywords[0] uppercased."""
    ec = brand["end_card"]
    bg, accent, text_c, muted = tuple(ec["bg"]), tuple(ec["accent"]), tuple(ec["text"]), tuple(ec["muted"])
    img = Image.new("RGB", (W, H), bg)
    draw = ImageDraw.Draw(img)
    y_rule = 760
    logo_ref = brand.get("logo_path")
    if logo_ref:
        lp = Path(logo_ref)
        if not lp.is_absolute() and config_dir:
            lp = config_dir / lp
        try:
            logo = Image.open(lp).convert("RGBA")
            lw = int(brand.get("logo_width", 200)) + 60
            lh = int(logo.height * lw / logo.width)
            logo = logo.resize((lw, lh), Image.LANCZOS)
            img.paste(logo, ((W - lw) // 2, 470), logo)
            y_rule = 800
        except OSError:
            pass
    draw.rectangle([W // 2 - 120, y_rule, W // 2 + 120, y_rule + 8], fill=accent)
    hook_text = (clip.get("hook_text") or "").strip() or (clip.get("keywords") or [""])[0].upper()
    hook_font = ImageFont.truetype(FONT, 92)
    y = y_rule + 90
    for line in wrap_text(hook_text, hook_font, max_w=920):
        bbox = hook_font.getbbox(line)
        draw.text(((W - (bbox[2] - bbox[0])) // 2, y), line, font=hook_font, fill=text_c)
        y += 118
    dom_font = ImageFont.truetype(FONT, 46)
    dom = brand["domain"]
    bbox = dom_font.getbbox(dom)
    draw.text(((W - (bbox[2] - bbox[0])) // 2, H - 260), dom, font=dom_font, fill=muted)
    img.save(path)


def render_end_card_png(clip: dict, brand: dict, path: Path, config_dir: Path | None) -> None:
    ec = brand["end_card"]
    bg, accent = tuple(ec["bg"]), tuple(ec["accent"])
    img = Image.new("RGB", (W, H), bg)
    draw = ImageDraw.Draw(img)
    # optional brand logo above the CTA (brand.logo_path, relative to config dir)
    y_rule, y_cta = 700, 800
    logo_ref = brand.get("logo_path")
    if logo_ref:
        lp = Path(logo_ref)
        if not lp.is_absolute() and config_dir:
            lp = config_dir / lp
        try:
            logo = Image.open(lp).convert("RGBA")
            lw = int(brand.get("logo_width", 200))
            lh = int(logo.height * lw / logo.width)
            logo = logo.resize((lw, lh), Image.LANCZOS)
            img.paste(logo, ((W - lw) // 2, 500), logo)
            y_rule, y_cta = 740, 840
        except OSError:
            pass  # missing logo is not fatal — text-only card
    # accent rule
    draw.rectangle([W // 2 - 120, y_rule, W // 2 + 120, y_rule + 8], fill=accent)
    cta_font = ImageFont.truetype(FONT, 62)
    y = y_cta
    for line in wrap_text(clip["cta"]["text"], cta_font, max_w=880):
        bbox = cta_font.getbbox(line)
        draw.text(((W - (bbox[2] - bbox[0])) // 2, y), line, font=cta_font,
                  fill=tuple(ec["text"]))
        y += 84
    dom_font = ImageFont.truetype(FONT, 116)
    domain = brand["domain"]
    bbox = dom_font.getbbox(domain)
    draw.text(((W - (bbox[2] - bbox[0])) // 2, y + 70), domain, font=dom_font,
              fill=accent)
    # per-clip subtitle (cta.subtitle) wins over the brand default
    subtitle = (clip.get("cta") or {}).get("subtitle") or ec.get("subtitle")
    if subtitle:
        sub_font = ImageFont.truetype(FONT, 40)
        bbox = sub_font.getbbox(subtitle)
        draw.text(((W - (bbox[2] - bbox[0])) // 2, y + 260), subtitle,
                  font=sub_font, fill=tuple(ec["muted"]))
    img.save(path)


def render_bar_png(brand: dict, path: Path) -> None:
    Image.new("RGB", (W, 10), tuple(brand["progress_bar_color"])).save(path)


def resolve_thumbnail_time(
    clip: dict, brand: dict, seg_bounds: list[dict], total: float
) -> float:
    """Pick a thumbnail frame time without product-specific ui_target hardcodes.

    Order:
      1. packaging.thumbnail_hint as numeric seconds (if parseable and in range)
      2. brand.thumbnail_ui_target matching a segment's ui_target (mid-segment)
      3. midpoint of the full video
    """
    hint = (clip.get("packaging") or {}).get("thumbnail_hint")
    if hint is not None:
        try:
            t = float(str(hint).strip())
            if 0 <= t <= total:
                return t
        except (TypeError, ValueError):
            pass

    target = brand.get("thumbnail_ui_target")
    if isinstance(target, str) and target:
        for b in seg_bounds:
            if b["ui_target"] == target:
                return b["start"] + (b["end"] - b["start"]) / 2

    return total / 2


# --- main -------------------------------------------------------------------------

def main() -> None:
    ap = argparse.ArgumentParser(description="Assemble the final vertical MP4 for a clip.")
    ap.add_argument("clip", help="clip definition JSON")
    ap.add_argument("--config", help="video.config.json or a directory containing it")
    ap.add_argument("--out", default=str(Path(__file__).resolve().parent / "out"))
    args = ap.parse_args()

    try:
        clip = json.loads(Path(args.clip).read_text())
    except (OSError, json.JSONDecodeError) as e:
        sys.exit(f"composite: cannot read clip at {args.clip}: {e}")
    config, config_dir = resolve_config(args.config)
    brand = brand_config(config)
    out_dir = Path(args.out).resolve()

    slug = clip["source"]["slug"]
    manifest_path = out_dir / "segments/segments.json"
    try:
        manifest = json.loads(manifest_path.read_text())
    except (OSError, json.JSONDecodeError) as e:
        sys.exit(f"composite: cannot read segments manifest at {manifest_path}: {e} — run record.mjs first")
    segs_by_index = {m["index"]: m for m in manifest}

    voice_dur = ffprobe_duration(out_dir / "voice.mp3")
    intent_total = clip["timing_map"][-1]["to_s"]
    scale = voice_dur / intent_total
    total = voice_dur + END_CARD_TAIL_S + HOOK_CARD_S
    print(f"[composite] voice {voice_dur:.2f}s, intent {intent_total}s, scale {scale:.3f}, total {total:.2f}s")

    parts_dir = out_dir / "parts"
    parts_dir.mkdir(exist_ok=True)

    # Frame-0 hook card (branded; also becomes the thumbnail file)
    hook_card = parts_dir / "hookcard.png"
    render_hook_card_png(clip, brand, hook_card, config_dir)
    hook_part = parts_dir / "part_hook.mp4"
    run(["ffmpeg", "-y", "-loop", "1", "-t", f"{HOOK_CARD_S:.3f}", "-i", hook_card,
         "-vf", f"fps={FPS},setsar=1", "-c:v", "libx264", "-preset", "medium",
         "-crf", "18", "-an", hook_part])

    part_files, seg_bounds = [hook_part], []
    t = HOOK_CARD_S
    for i, seg in enumerate(clip["timing_map"]):
        dur = (seg["to_s"] - seg["from_s"]) * scale
        is_end_card = seg["ui_target"] == "end_card"
        if is_end_card:
            dur += END_CARD_TAIL_S
        part = parts_dir / f"part{i:02d}.mp4"
        if i in segs_by_index:
            m = segs_by_index[i]
            src = out_dir / "segments" / m["file"]
            base_vf = (f"scale={W}:{H}:force_original_aspect_ratio=decrease,"
                       f"pad={W}:{H}:(ow-iw)/2:(oh-ih)/2:color=black")
            if i % 2 == 1:
                # alternating segments get a slow 0→8% push-in so back-to-back
                # segments on the same page don't look like one static shot.
                # fps MUST be set before and inside zoompan (a trailing fps=30
                # after zoompan mis-times frames and balloons duration ~40x).
                frames = max(int(dur * FPS), 1)
                vf = (base_vf
                      + f",fps={FPS}"
                      + f",zoompan=z='1+{ZOOM_PCT}*on/{frames}':"
                        f"x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:fps={FPS}:s={W}x{H}"
                      + f",setsar=1")
            else:
                vf = base_vf + f",fps={FPS},setsar=1"
            run(["ffmpeg", "-y", "-ss", f"{m['ready_offset_s']:.2f}", "-t", f"{dur:.3f}",
                 "-i", src,
                 "-vf", vf,
                 "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-an", part])
        elif is_end_card:
            card = parts_dir / "endcard.png"
            render_end_card_png(clip, brand, card, config_dir)
            run(["ffmpeg", "-y", "-loop", "1", "-t", f"{dur:.3f}", "-i", card,
                 "-vf", f"fps={FPS},setsar=1", "-c:v", "libx264", "-preset", "medium",
                 "-crf", "18", "-an", part])
        else:
            sys.exit(f"[composite] no recording for ui_target {seg['ui_target']}")
        part_files.append(part)
        seg_bounds.append({"index": i, "ui_target": seg["ui_target"], "start": t, "end": t + dur})
        t += dur

    concat_list = parts_dir / "concat.txt"
    concat_list.write_text("".join(f"file '{p}'\n" for p in part_files))
    base = parts_dir / "base.mp4"
    run(["ffmpeg", "-y", "-f", "concat", "-safe", "0", "-i", concat_list, "-c", "copy", base])

    # captions
    cap_dir = out_dir / "captions"
    cap_dir.mkdir(exist_ok=True)
    phrases = group_phrases(parse_srt(out_dir / "voice.srt"))
    print(f"[composite] {len(phrases)} caption phrases")
    inputs = [str(base), str(out_dir / "voice.mp3")]
    chain = "[0:v]"
    for j, ph in enumerate(phrases):
        png = cap_dir / f"cue{j:03d}.png"
        ch = render_caption_png(ph["text"], png)
        inputs.append(str(png))
        idx = j + 2
        y = 1650 - ch
        cs, ce = ph["start"] + HOOK_CARD_S, ph["end"] + HOOK_CARD_S
        chain += f"[{idx}:v]overlay=0:{y}:enable='between(t,{cs:.3f},{ce:.3f})'[c{j}];[c{j}]"
    # hook keyword, first 3s of app footage (after the hook card)
    hook = cap_dir / "hook.png"
    render_caption_png(clip["keywords"][0].upper(), hook, fontsize=104, box=True)
    inputs.append(str(hook))
    hi = len(inputs) - 1
    hook_y = 760
    he = HOOK_CARD_S + 3
    chain += f"[{hi}:v]overlay=0:{hook_y}:enable='between(t,{HOOK_CARD_S:.3f},{he:.3f})'[hk];[hk]"
    # progress bar
    bar = cap_dir / "bar.png"
    render_bar_png(brand, bar)
    inputs.append(str(bar))
    bi = len(inputs) - 1
    chain += f"[{bi}:v]overlay=x='-{W}+{W}*t/{total:.3f}':y=0[vout]"

    audio = (f"[1:a]loudnorm=I=-16:TP=-1.5:LRA=11,aresample=48000,apad,atrim=0:{total:.3f},"
             f"asetpts=PTS-STARTPTS[aout]")

    # caption/bar/hook PNGs are single frames that EOF after 0.04s; without
    # -loop 1 the overlay silently stops compositing them after their first
    # frame (discovered 2026-07-29: captions vanished from the whole video)
    cmd = ["ffmpeg", "-y", "-i", inputs[0], "-itsoffset", f"{HOOK_CARD_S:.3f}",
           "-i", inputs[1]]
    for inp in inputs[2:]:
        cmd += ["-loop", "1", "-framerate", str(FPS), "-i", inp]
    cmd += ["-filter_complex", chain + ";" + audio,
            "-map", "[vout]", "-map", "[aout]",
            "-c:v", "libx264", "-preset", "medium", "-b:v", "8M", "-maxrate", "9M",
            "-bufsize", "16M", "-pix_fmt", "yuv420p", "-r", str(FPS),
            "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart",
            "-t", f"{total:.3f}", out_dir / f"{slug}.mp4"]
    run(cmd)

    # Thumbnail = the hook card itself (consistent, punchy channel grid)
    run(["ffmpeg", "-y", "-i", hook_card, "-frames:v", "1", "-update", "1",
         "-q:v", "2", out_dir / f"{slug}.jpg"])

    print(f"[composite] done -> {out_dir / (slug + '.mp4')}")


if __name__ == "__main__":
    main()
