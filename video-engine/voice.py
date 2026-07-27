#!/usr/bin/env python3
"""voice.py — generate voiceover + subtitles from a clip definition.

Usage: voice.py <clip.json> [--config <video.config.json|dir>] [--out <dir>]

Reads spoken_script, synthesizes <out>/voice.mp3 + <out>/voice.srt with
edge-tts, adjusting speech rate so the voiceover lands near 40-45s.
Voice list and target duration can be overridden via the config's "tts"
section: {"voices": [...], "target_s": 42.5}.

Exit codes: 0 ok · 1 synthesis failed · 2 bad args / config error
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

import edge_tts
from edge_tts import SubMaker

DEFAULT_VOICES = ["en-US-AndrewNeural", "en-US-GuyNeural", "en-US-BrianNeural"]
DEFAULT_TARGET_S = 42.5


def synthesize(text: str, voice: str, rate_pct: int, mp3: Path, srt: Path) -> None:
    rate = f"{rate_pct:+d}%"
    communicate = edge_tts.Communicate(text, voice, rate=rate, boundary="WordBoundary")
    submaker = SubMaker()
    with open(mp3, "wb") as f:
        for chunk in communicate.stream_sync():
            if chunk["type"] == "audio":
                f.write(chunk["data"])
            elif chunk["type"] == "WordBoundary":
                submaker.feed(chunk)
    srt.write_text(submaker.get_srt(), encoding="utf-8")


def duration_s(path: Path) -> float:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "csv=p=0", str(path)],
        capture_output=True, text=True, check=True,
    )
    return float(out.stdout.strip())


def resolve_config(p: str | None) -> dict:
    if not p:
        return {}
    path = Path(p).resolve()
    if path.is_dir():
        path = path / "video.config.json"
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as e:
        sys.exit(f"voice: cannot read config at {path}: {e}")


def main() -> None:
    ap = argparse.ArgumentParser(description="Generate voiceover + subtitles for a clip.")
    ap.add_argument("clip", help="clip definition JSON")
    ap.add_argument("--config", help="video.config.json or a directory containing it")
    ap.add_argument("--out", default=str(Path(__file__).resolve().parent / "out"))
    args = ap.parse_args()

    try:
        clip = json.loads(Path(args.clip).read_text())
    except (OSError, json.JSONDecodeError) as e:
        sys.exit(f"voice: cannot read clip at {args.clip}: {e}")
    config = resolve_config(args.config)
    tts = config.get("tts", {})
    voices = tts.get("voices", DEFAULT_VOICES)
    target_s = tts.get("target_s", DEFAULT_TARGET_S)

    text = clip["spoken_script"]
    out_dir = Path(args.out).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    mp3, srt = out_dir / "voice.mp3", out_dir / "voice.srt"

    voice = None
    for candidate in voices:
        try:
            synthesize(text, candidate, 0, mp3, srt)
            voice = candidate
            break
        except Exception as e:  # voice retired / network hiccup -> try next
            print(f"[voice] {candidate} failed ({e}); trying next")
    if voice is None:
        sys.exit("[voice] no usable edge-tts voice")

    d = duration_s(mp3)
    print(f"[voice] {voice} at +0% -> {d:.1f}s")

    for _ in range(3):
        if 40.0 <= d <= 45.5:
            break
        # duration scales ~1/(1+rate); pick the rate that would hit target_s
        rate = round((d / target_s - 1) * 100)
        rate = max(-20, min(40, rate))
        print(f"[voice] {d:.1f}s outside 40-45.5s; retrying at {rate:+d}%")
        synthesize(text, voice, rate, mp3, srt)
        d = duration_s(mp3)

    print(f"[voice] final: {d:.1f}s -> {mp3.name}, {srt.name}")
    meta = {"voice": voice, "duration_s": d}
    (out_dir / "voice.json").write_text(json.dumps(meta, indent=2))


if __name__ == "__main__":
    main()
