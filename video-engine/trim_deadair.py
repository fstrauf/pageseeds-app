#!/usr/bin/env python3
"""trim_deadair.py — cut static (frozen) stretches out of an agentic video take.

Agentic MCP takes contain long dead periods while the agent "thinks" (MCP
round-trips). This detects frozen-frame intervals with ffmpeg's freezedetect
and shortens every freeze longer than --min-freeze to --keep seconds,
jump-cut style (normal pacing for shorts).

Usage: trim_deadair.py <input> [--out <path>] [--min-freeze 1.5] [--keep 0.4]

Prints a JSON summary: original/kept durations, freezes found, output path.
Exit codes: 0 ok · 1 processing failed · 2 bad args.
"""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

FREEZE_RE = re.compile(r"freeze_(start|end|duration):\s*([0-9.]+)")


def run(cmd: list[str]) -> str:
    p = subprocess.run([str(c) for c in cmd], capture_output=True, text=True)
    return (p.stdout or "") + (p.stderr or "")


def ffprobe_duration(path: Path) -> float:
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "csv=p=0", str(path)],
        capture_output=True, text=True, check=True)
    return float(out.stdout.strip())


def detect_freezes(src: Path, min_freeze: float) -> list[tuple[float, float]]:
    """Return [(start, end), ...] of frozen intervals >= min_freeze seconds."""
    log = run(["ffmpeg", "-i", src, "-vf",
               f"freezedetect=n=0.003:d={min_freeze}", "-an", "-f", "null", "-"])
    freezes, cur_start = [], None
    for line in log.splitlines():
        for key, val in FREEZE_RE.findall(line):
            if key == "start":
                cur_start = float(val)
            elif key == "end" and cur_start is not None:
                freezes.append((cur_start, float(val)))
                cur_start = None
    return freezes


def build_keep_segments(total: float, freezes: list[tuple[float, float]],
                        keep: float, min_freeze: float) -> list[tuple[float, float]]:
    """Timeline segments to keep: everything, with long freezes cut to `keep`s."""
    segs, t = [], 0.0
    for fs, fe in sorted(freezes):
        if fe - fs < min_freeze:
            continue
        if fs > t:
            segs.append((t, fs))
        segs.append((fs, min(fs + keep, fe)))  # short beat so the cut reads intentional
        t = fe
    if t < total:
        segs.append((t, total))
    return [(a, b) for a, b in segs if b - a > 0.05]


def main() -> None:
    ap = argparse.ArgumentParser(description="Cut frozen stretches from an agentic take.")
    ap.add_argument("input")
    ap.add_argument("--out")
    ap.add_argument("--min-freeze", type=float, default=1.5)
    ap.add_argument("--keep", type=float, default=0.4)
    args = ap.parse_args()

    src = Path(args.input).resolve()
    if not src.exists():
        print(f"trim_deadair: input not found: {src}", file=sys.stderr)
        sys.exit(2)
    out = Path(args.out).resolve() if args.out else src.with_suffix(".trimmed.mp4")

    total = ffprobe_duration(src)
    freezes = detect_freezes(src, args.min_freeze)
    keeps = build_keep_segments(total, freezes, args.keep, args.min_freeze)
    if not keeps:
        print("trim_deadair: nothing to keep", file=sys.stderr)
        sys.exit(1)

    select = "+".join(f"between(t,{a:.3f},{b:.3f})" for a, b in keeps)
    vf = f"select='{select}',setpts=N/FRAME_RATE/TB"
    p = subprocess.run(
        ["ffmpeg", "-y", "-i", src, "-vf", vf, "-af", f"aselect='{select}',asetpts=N/SR/TB",
         "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p",
         "-c:a", "aac", "-movflags", "+faststart", out],
        capture_output=True, text=True)
    if p.returncode != 0 or not out.exists():
        print(f"trim_deadair: ffmpeg failed: {(p.stderr or '')[-500:]}", file=sys.stderr)
        sys.exit(1)

    kept = sum(b - a for a, b in keeps)
    print(json.dumps({
        "input": str(src), "output": str(out),
        "original_s": round(total, 2), "kept_s": round(kept, 2),
        "cut_s": round(total - kept, 2),
        "freezes_found": len([f for f in freezes if f[1] - f[0] >= args.min_freeze]),
    }, indent=2))


if __name__ == "__main__":
    main()
