#!/usr/bin/env python3
"""publish.py — upload a rendered clip to YouTube (Phase D).

Usage:
  publish.py <clip.json> --platforms youtube [--video <mp4>] [--dry-run]

Reads packaging metadata from the clip definition, resolves the MP4 path,
and uploads via YouTube Data API v3 (OAuth refresh + resumable upload).
Stdlib only (urllib). YouTube-only for this MVP — no TikTok/Instagram.

Exit codes: 0 ok · 1 upload/API failed · 2 bad args / config / missing secrets
"""
from __future__ import annotations

import argparse
import http.client
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

TOKEN_URL = "https://oauth2.googleapis.com/token"
UPLOAD_INIT_URL = (
    "https://www.googleapis.com/upload/youtube/v3/videos"
    "?uploadType=resumable&part=snippet,status"
)
YOUTUBE_KEYS = (
    "YOUTUBE_CLIENT_ID",
    "YOUTUBE_CLIENT_SECRET",
    "YOUTUBE_REFRESH_TOKEN",
)
SUPPORTED_PLATFORMS = frozenset({"youtube"})
# Shorts are small; single PUT is fine. Chunk if larger.
CHUNK_SIZE = 8 * 1024 * 1024
DEFAULT_CATEGORY_ID = "22"  # People & Blogs
PRIVACY = "private"


def die(msg: str, code: int = 2) -> None:
    print(msg, file=sys.stderr)
    sys.exit(code)


def find_repo_root(start: Path) -> Path | None:
    """Walk up looking for .git; return that directory or None."""
    cur = start.resolve()
    if cur.is_file():
        cur = cur.parent
    for parent in [cur, *cur.parents]:
        if (parent / ".git").exists():
            return parent
    return None


def parse_env_file(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return out
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, val = line.partition("=")
        key = key.strip()
        val = val.strip().strip('"').strip("'")
        if key and val and key not in out:
            out[key] = val
    return out


def load_secrets(repo_root: Path | None) -> dict[str, str]:
    """Env-file chain matching Rust EnvResolver: first file wins per key, then process env."""
    files: list[Path] = []
    home = Path.home()
    secrets = home / ".config" / "automation" / "secrets.env"
    if secrets.is_file():
        files.append(secrets)
    if repo_root is not None:
        for name in (".env.local", ".env"):
            p = repo_root / name
            if p.is_file():
                files.append(p)

    resolved: dict[str, str] = {}
    for path in files:
        for k, v in parse_env_file(path).items():
            if k not in resolved:
                resolved[k] = v

    for key in YOUTUBE_KEYS:
        if key not in resolved:
            env_val = os.environ.get(key, "").strip()
            if env_val:
                resolved[key] = env_val
    return resolved


def resolve_video_path(
    clip_path: Path, video_flag: str | None, *, require_exists: bool = True
) -> Path:
    if video_flag:
        p = Path(video_flag).expanduser().resolve()
        if require_exists and not p.is_file():
            die(f"publish: video not found: {p}")
        return p

    slug = clip_path.stem
    candidates: list[Path] = []
    parent = clip_path.resolve().parent
    # Convention: .../video/clips/<slug>.json → .../video/out/<slug>.mp4
    if parent.name == "clips" and parent.parent.name == "video":
        candidates.append(parent.parent / "out" / f"{slug}.mp4")
    elif parent.name == "clips":
        candidates.append(parent.parent / "out" / f"{slug}.mp4")
    # Fallback: video-engine/out/<slug>.mp4 next to this script
    candidates.append(Path(__file__).resolve().parent / "out" / f"{slug}.mp4")

    for candidate in candidates:
        if candidate.is_file():
            return candidate

    if not require_exists and candidates:
        return candidates[0]

    die(
        f"publish: cannot resolve MP4 for {clip_path.name} "
        f"(tried video/out/{slug}.mp4 and video-engine/out/{slug}.mp4); pass --video"
    )


def tags_from_hashtags(hashtags) -> list[str]:
    if not hashtags:
        return []
    tags: list[str] = []
    for h in hashtags:
        if not isinstance(h, str):
            continue
        t = h.strip().lstrip("#").strip()
        if t:
            tags.append(t)
    return tags


def build_metadata(packaging: dict) -> dict:
    title = (packaging.get("title") or "").strip()
    if not title:
        die("publish: packaging.title is required")
    description = (packaging.get("description") or "").strip()
    # YouTube description hard limit 5000 — warn only; do not truncate or fail
    if len(description) > 5000:
        print(
            f"publish: warning: packaging.description is {len(description)} chars "
            f"(YouTube limit 5000); not truncating",
            file=sys.stderr,
        )
    tags = tags_from_hashtags(packaging.get("hashtags") or [])
    # YouTube title hard limit 100
    if len(title) > 100:
        title = title[:97] + "..."
    return {
        "snippet": {
            "title": title,
            "description": description,
            "tags": tags,
            "categoryId": DEFAULT_CATEGORY_ID,
        },
        "status": {
            "privacyStatus": PRIVACY,
            "selfDeclaredMadeForKids": False,
        },
    }


def refresh_access_token(creds: dict[str, str]) -> str:
    body = urllib.parse.urlencode(
        {
            "grant_type": "refresh_token",
            "client_id": creds["YOUTUBE_CLIENT_ID"],
            "client_secret": creds["YOUTUBE_CLIENT_SECRET"],
            "refresh_token": creds["YOUTUBE_REFRESH_TOKEN"],
        }
    ).encode("utf-8")
    req = urllib.request.Request(
        TOKEN_URL,
        data=body,
        method="POST",
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", errors="replace")[:200]
        die(f"publish: token refresh failed ({e.code}): {detail}", code=1)
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as e:
        die(f"publish: token refresh failed: {e}", code=1)
    token = data.get("access_token")
    if not token:
        die("publish: token refresh returned no access_token", code=1)
    return token


def _http_put(upload_url: str, body: bytes, headers: dict[str, str]) -> tuple[int, bytes]:
    """PUT via http.client so Google's 308 Resume Incomplete is not treated as a redirect."""
    parsed = urlparse(upload_url)
    if parsed.scheme not in ("https", "http"):
        die(f"publish: unexpected upload URL scheme: {parsed.scheme}", code=1)
    path = parsed.path or "/"
    if parsed.query:
        path = f"{path}?{parsed.query}"
    conn_cls = http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
    host = parsed.netloc
    conn = conn_cls(host, timeout=600)
    try:
        conn.request("PUT", path, body=body, headers=headers)
        resp = conn.getresponse()
        data = resp.read()
        return resp.status, data
    finally:
        conn.close()


def resumable_upload(access_token: str, video_path: Path, metadata: dict) -> dict:
    size = video_path.stat().st_size
    init_body = json.dumps(metadata).encode("utf-8")
    init_req = urllib.request.Request(
        UPLOAD_INIT_URL,
        data=init_body,
        method="POST",
        headers={
            "Authorization": f"Bearer {access_token}",
            "Content-Type": "application/json; charset=UTF-8",
            "X-Upload-Content-Length": str(size),
            "X-Upload-Content-Type": "video/mp4",
        },
    )
    try:
        with urllib.request.urlopen(init_req, timeout=60) as resp:
            upload_url = resp.headers.get("Location")
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", errors="replace")[:300]
        die(f"publish: upload init failed ({e.code}): {detail}", code=1)
    except (urllib.error.URLError, TimeoutError) as e:
        die(f"publish: upload init failed: {e}", code=1)

    if not upload_url:
        die("publish: upload init missing Location header", code=1)

    auth = {"Authorization": f"Bearer {access_token}", "Content-Type": "video/mp4"}
    result_body = b""
    with open(video_path, "rb") as f:
        if size <= CHUNK_SIZE:
            data = f.read()
            status, result_body = _http_put(
                upload_url,
                data,
                {**auth, "Content-Length": str(size)},
            )
            if status not in (200, 201):
                detail = result_body.decode("utf-8", errors="replace")[:300]
                die(f"publish: upload failed ({status}): {detail}", code=1)
        else:
            offset = 0
            while offset < size:
                chunk = f.read(CHUNK_SIZE)
                end = offset + len(chunk) - 1
                status, body = _http_put(
                    upload_url,
                    chunk,
                    {
                        **auth,
                        "Content-Length": str(len(chunk)),
                        "Content-Range": f"bytes {offset}-{end}/{size}",
                    },
                )
                if status == 308:
                    offset = end + 1
                    continue
                if status in (200, 201):
                    result_body = body
                    break
                detail = body.decode("utf-8", errors="replace")[:300]
                die(f"publish: upload failed ({status}): {detail}", code=1)

    try:
        return json.loads(result_body.decode("utf-8")) if result_body else {}
    except json.JSONDecodeError:
        die("publish: upload response was not JSON", code=1)


def publish_youtube(clip_path: Path, video_path: Path, dry_run: bool) -> None:
    try:
        clip = json.loads(clip_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        die(f"publish: cannot read clip at {clip_path}: {e}")

    packaging = clip.get("packaging")
    if not isinstance(packaging, dict):
        die("publish: clip missing packaging object")

    metadata = build_metadata(packaging)

    if dry_run:
        description = metadata["snippet"]["description"]
        plan = {
            "platform": "youtube",
            "status": "dry_run",
            "video_path": str(video_path),
            "description_chars": len(description),
            "metadata": metadata,
            "plan": [
                "refresh_token",
                "resumable_init",
                "upload_bytes",
                "write_published_youtube_to_clip",
            ],
            "published": {
                "youtube": {
                    "video_id": "<id>",
                    "url": "https://youtu.be/<id>",
                    "published_at": "<ISO-8601 UTC>",
                    "privacy": PRIVACY,
                }
            },
            "note": "real upload needs YOUTUBE_CLIENT_ID/SECRET/REFRESH_TOKEN; "
            "dry-run does not write published.youtube to the clip file",
        }
        print(json.dumps(plan, indent=2))
        return

    repo_root = find_repo_root(clip_path) or find_repo_root(Path.cwd())
    creds = load_secrets(repo_root)
    missing = [k for k in YOUTUBE_KEYS if not creds.get(k)]
    if missing:
        die(
            "publish: missing YOUTUBE_* credentials — run the auth setup "
            "(see video-engine/README.md)"
        )

    token = refresh_access_token(creds)
    result = resumable_upload(token, video_path, metadata)
    video_id = result.get("id")
    if not video_id:
        die("publish: upload succeeded but no video id in response", code=1)

    published_at = (
        datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )
    youtube_published = {
        "video_id": video_id,
        "url": f"https://youtu.be/{video_id}",
        "published_at": published_at,
        "privacy": PRIVACY,
    }
    published = clip.get("published")
    if not isinstance(published, dict):
        published = {}
    published["youtube"] = youtube_published
    clip["published"] = published
    clip_path.write_text(
        json.dumps(clip, indent=2) + "\n", encoding="utf-8"
    )

    out = {
        "platform": "youtube",
        "status": "ok",
        "video_id": video_id,
        "url": f"https://youtu.be/{video_id}",
        "privacy": PRIVACY,
        "published": {"youtube": youtube_published},
    }
    print(json.dumps(out, indent=2))


def main() -> None:
    ap = argparse.ArgumentParser(
        description="Publish a rendered video clip (YouTube only)."
    )
    ap.add_argument("clip", help="clip definition JSON")
    ap.add_argument(
        "--platforms",
        required=True,
        help="comma-separated platforms (only 'youtube' supported)",
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
        clip_path, args.video, require_exists=not args.dry_run
    )

    if "youtube" in platforms:
        publish_youtube(clip_path, video_path, args.dry_run)


if __name__ == "__main__":
    main()
