#!/usr/bin/env python3
"""publish.py — upload a rendered clip to YouTube, TikTok, and/or Instagram (Phase D).

Usage:
  publish.py <clip.json> --platforms youtube[,tiktok][,instagram] [--video <mp4>] [--dry-run]

Reads packaging metadata from the clip definition, resolves the MP4 path,
and uploads via:
  - YouTube Data API v3 (OAuth refresh + resumable upload)
  - TikTok Content Posting API **Inbox Upload** (scope video.upload only —
    lands in creator drafts; user finishes in the TikTok app). Not Direct Post.
  - Instagram Graph API Reels (resumable container + rupload + media_publish)

Stdlib only (urllib / http.client).

Exit codes:
  0  all requested platforms succeeded (or dry-run)
  1  any platform failed after shared preflight (args/config ok)
  2  bad args / unknown platform / unresolvable clip|video
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# Script-dir bootstrap so `python3 path/to/publish.py` resolves local packages
_HERE = Path(__file__).resolve().parent
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))

from publish_common import (  # noqa: E402
    PublishError,
    SUPPORTED_PLATFORMS,
    die,
    resolve_video_path,
)
from publish_adapters.instagram import publish_instagram  # noqa: E402
from publish_adapters.tiktok import publish_tiktok  # noqa: E402
from publish_adapters.youtube import publish_youtube  # noqa: E402

PLATFORM_HANDLERS = {
    "youtube": publish_youtube,
    "tiktok": publish_tiktok,
    "instagram": publish_instagram,
}


def main() -> None:
    ap = argparse.ArgumentParser(
        description=(
            "Publish a rendered video clip to YouTube, TikTok (inbox drafts), "
            "and/or Instagram Reels."
        )
    )
    ap.add_argument("clip", help="clip definition JSON")
    ap.add_argument(
        "--platforms",
        required=True,
        help="comma-separated platforms (youtube, tiktok, instagram)",
    )
    ap.add_argument("--video", help="path to MP4 (default: video/out/<slug>.mp4)")
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help="print request plan as JSON; no network calls",
    )
    args = ap.parse_args()

    platforms = [p.strip().lower() for p in args.platforms.split(",") if p.strip()]
    if not platforms:
        die("publish: --platforms requires at least one platform")
    # de-dupe preserving order
    seen: set[str] = set()
    ordered: list[str] = []
    for p in platforms:
        if p not in seen:
            seen.add(p)
            ordered.append(p)
    platforms = ordered

    unknown = [p for p in platforms if p not in SUPPORTED_PLATFORMS]
    if unknown:
        die(
            f"publish: unsupported platform(s): {', '.join(unknown)} "
            f"(supported: {', '.join(sorted(SUPPORTED_PLATFORMS))})"
        )

    clip_path = Path(args.clip).expanduser()
    if not clip_path.is_file():
        die(f"publish: clip not found: {clip_path}")

    video_path = resolve_video_path(
        clip_path,
        args.video,
        require_exists=not args.dry_run,
        engine_dir=_HERE,
    )

    results: list[dict] = []
    any_failed = False

    for platform in platforms:
        handler = PLATFORM_HANDLERS[platform]
        try:
            result = handler(clip_path, video_path, args.dry_run)
            results.append(result)
        except PublishError as e:
            print(e.message, file=sys.stderr)
            results.append(
                {
                    "platform": platform,
                    "status": "error",
                    "error": e.message,
                }
            )
            any_failed = True
        except Exception as e:
            # Unexpected — still soft-fail so other platforms continue
            msg = f"publish: {platform} unexpected error: {e}"
            print(msg, file=sys.stderr)
            results.append(
                {
                    "platform": platform,
                    "status": "error",
                    "error": msg,
                }
            )
            any_failed = True

    if len(results) == 1:
        print(json.dumps(results[0], indent=2))
    else:
        print(json.dumps({"results": results}, indent=2))

    sys.exit(1 if any_failed else 0)


if __name__ == "__main__":
    main()
