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
import http.client
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse

# ── YouTube ──────────────────────────────────────────────────────────────────
YOUTUBE_TOKEN_URL = "https://oauth2.googleapis.com/token"
UPLOAD_INIT_URL = (
    "https://www.googleapis.com/upload/youtube/v3/videos"
    "?uploadType=resumable&part=snippet,status"
)
YOUTUBE_KEYS = (
    "YOUTUBE_CLIENT_ID",
    "YOUTUBE_CLIENT_SECRET",
    "YOUTUBE_REFRESH_TOKEN",
)
YOUTUBE_CHUNK_SIZE = 8 * 1024 * 1024
DEFAULT_CATEGORY_ID = "22"  # People & Blogs
PRIVACY = "private"

# ── TikTok (Inbox Upload only — never Direct Post) ───────────────────────────
TIKTOK_TOKEN_URL = "https://open.tiktokapis.com/v2/oauth/token/"
TIKTOK_INBOX_INIT_URL = (
    "https://open.tiktokapis.com/v2/post/publish/inbox/video/init/"
)
TIKTOK_STATUS_URL = "https://open.tiktokapis.com/v2/post/publish/status/fetch/"
TIKTOK_KEYS = (
    "TIKTOK_CLIENT_KEY",
    "TIKTOK_CLIENT_SECRET",
    "TIKTOK_REFRESH_TOKEN",
)
# Chunk bounds from TikTok media transfer guide
TIKTOK_MIN_CHUNK = 5 * 1024 * 1024  # 5 MB
TIKTOK_MAX_CHUNK = 64 * 1024 * 1024  # 64 MB
TIKTOK_MULTI_CHUNK = 10 * 1024 * 1024  # 10 MB nominal for multi-chunk uploads

# ── Instagram (Graph API Reels — resumable / rupload) ────────────────────────
META_GRAPH_VERSION = "v21.0"
META_GRAPH_HOST = "https://graph.facebook.com"
META_RUPLOAD_HOST = "https://rupload.facebook.com"
INSTAGRAM_KEYS = (
    "META_ACCESS_TOKEN",
    "IG_USER_ID",
)
INSTAGRAM_EXTEND_KEYS = (
    "META_APP_ID",
    "META_APP_SECRET",
)
# Meta recommends ~once/minute; allow modest faster polls with backoff, cap ~8 min
IG_POLL_INITIAL_S = 15
IG_POLL_MAX_S = 30
IG_POLL_TIMEOUT_S = 8 * 60
IG_CAPTION_LIMIT = 2200

ALL_SECRET_KEYS = YOUTUBE_KEYS + TIKTOK_KEYS + INSTAGRAM_KEYS + INSTAGRAM_EXTEND_KEYS
SUPPORTED_PLATFORMS = frozenset({"youtube", "tiktok", "instagram"})


class PublishError(Exception):
    """Controlled per-platform failure (does not abort other platforms)."""

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message


def die(msg: str, code: int = 2) -> None:
    print(msg, file=sys.stderr)
    sys.exit(code)


def utc_now_iso() -> str:
    return (
        datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


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

    for key in ALL_SECRET_KEYS:
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


def load_clip(clip_path: Path) -> dict:
    try:
        clip = json.loads(clip_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        raise PublishError(f"publish: cannot read clip at {clip_path}: {e}") from e
    if not isinstance(clip, dict):
        raise PublishError("publish: clip JSON must be an object")
    return clip


def merge_published(clip_path: Path, clip: dict, platform_key: str, payload: dict) -> dict:
    """Merge published.<platform_key> into clip JSON without wiping other keys."""
    published = clip.get("published")
    if not isinstance(published, dict):
        published = {}
    published[platform_key] = payload
    clip["published"] = published
    clip_path.write_text(json.dumps(clip, indent=2) + "\n", encoding="utf-8")
    return published


def packaging_preview(packaging: dict) -> dict:
    return {
        "title": (packaging.get("title") or "").strip(),
        "description": (packaging.get("description") or "").strip(),
        "hashtags": packaging.get("hashtags") or [],
    }


# ── Shared HTTP helpers ──────────────────────────────────────────────────────


def _http_put(upload_url: str, body: bytes, headers: dict[str, str]) -> tuple[int, bytes]:
    """PUT via http.client so non-2xx (308/206) are not treated as redirects."""
    parsed = urlparse(upload_url)
    if parsed.scheme not in ("https", "http"):
        raise PublishError(f"publish: unexpected upload URL scheme: {parsed.scheme}")
    path = parsed.path or "/"
    if parsed.query:
        path = f"{path}?{parsed.query}"
    conn_cls = (
        http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
    )
    host = parsed.netloc
    conn = conn_cls(host, timeout=600)
    try:
        conn.request("PUT", path, body=body, headers=headers)
        resp = conn.getresponse()
        data = resp.read()
        return resp.status, data
    finally:
        conn.close()


def _http_json_post(
    url: str,
    body: dict | None,
    headers: dict[str, str],
    *,
    form: bool = False,
    form_fields: dict[str, str] | None = None,
    timeout: int = 60,
) -> tuple[int, dict | str]:
    """POST JSON or form-urlencoded; return (status, parsed_json_or_text)."""
    if form:
        data = urllib.parse.urlencode(form_fields or {}).encode("utf-8")
        hdrs = {**headers, "Content-Type": "application/x-www-form-urlencoded"}
    else:
        data = json.dumps(body if body is not None else {}).encode("utf-8")
        hdrs = {**headers, "Content-Type": "application/json; charset=UTF-8"}
    req = urllib.request.Request(url, data=data, method="POST", headers=hdrs)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8")
            status = resp.status
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", errors="replace")
        status = e.code
    except (urllib.error.URLError, TimeoutError) as e:
        raise PublishError(f"publish: request failed: {e}") from e

    try:
        return status, json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        return status, raw


def _http_json_get(
    url: str,
    headers: dict[str, str],
    *,
    timeout: int = 60,
) -> tuple[int, dict | str]:
    """GET JSON; return (status, parsed_json_or_text)."""
    req = urllib.request.Request(url, method="GET", headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw = resp.read().decode("utf-8")
            status = resp.status
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", errors="replace")
        status = e.code
    except (urllib.error.URLError, TimeoutError) as e:
        raise PublishError(f"publish: request failed: {e}") from e

    try:
        return status, json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        return status, raw


def _http_post_binary(
    url: str,
    body: bytes,
    headers: dict[str, str],
    *,
    timeout: int = 600,
) -> tuple[int, dict | str]:
    """POST raw binary body via http.client; return (status, parsed_json_or_text)."""
    parsed = urlparse(url)
    if parsed.scheme not in ("https", "http"):
        raise PublishError(f"publish: unexpected upload URL scheme: {parsed.scheme}")
    path = parsed.path or "/"
    if parsed.query:
        path = f"{path}?{parsed.query}"
    conn_cls = (
        http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
    )
    host = parsed.netloc
    conn = conn_cls(host, timeout=timeout)
    try:
        hdrs = {**headers, "Content-Length": str(len(body))}
        conn.request("POST", path, body=body, headers=hdrs)
        resp = conn.getresponse()
        raw = resp.read().decode("utf-8", errors="replace")
        status = resp.status
    except (OSError, TimeoutError) as e:
        raise PublishError(f"publish: binary POST failed: {e}") from e
    finally:
        conn.close()

    try:
        return status, json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        return status, raw


# ── YouTube adapter ──────────────────────────────────────────────────────────


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


def build_youtube_metadata(packaging: dict) -> dict:
    title = (packaging.get("title") or "").strip()
    if not title:
        raise PublishError("publish: packaging.title is required for YouTube")
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


def refresh_youtube_token(creds: dict[str, str]) -> str:
    status, data = _http_json_post(
        YOUTUBE_TOKEN_URL,
        None,
        {},
        form=True,
        form_fields={
            "grant_type": "refresh_token",
            "client_id": creds["YOUTUBE_CLIENT_ID"],
            "client_secret": creds["YOUTUBE_CLIENT_SECRET"],
            "refresh_token": creds["YOUTUBE_REFRESH_TOKEN"],
        },
    )
    if not isinstance(data, dict):
        raise PublishError(f"publish: YouTube token refresh failed ({status}): {data!r}"[:200])
    if status != 200 or not data.get("access_token"):
        detail = json.dumps(data)[:200]
        raise PublishError(f"publish: YouTube token refresh failed ({status}): {detail}")
    return data["access_token"]


def youtube_resumable_upload(access_token: str, video_path: Path, metadata: dict) -> dict:
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
        raise PublishError(f"publish: YouTube upload init failed ({e.code}): {detail}") from e
    except (urllib.error.URLError, TimeoutError) as e:
        raise PublishError(f"publish: YouTube upload init failed: {e}") from e

    if not upload_url:
        raise PublishError("publish: YouTube upload init missing Location header")

    auth = {"Authorization": f"Bearer {access_token}", "Content-Type": "video/mp4"}
    result_body = b""
    with open(video_path, "rb") as f:
        if size <= YOUTUBE_CHUNK_SIZE:
            data = f.read()
            status, result_body = _http_put(
                upload_url,
                data,
                {**auth, "Content-Length": str(size)},
            )
            if status not in (200, 201):
                detail = result_body.decode("utf-8", errors="replace")[:300]
                raise PublishError(f"publish: YouTube upload failed ({status}): {detail}")
        else:
            offset = 0
            while offset < size:
                chunk = f.read(YOUTUBE_CHUNK_SIZE)
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
                raise PublishError(f"publish: YouTube upload failed ({status}): {detail}")

    try:
        return json.loads(result_body.decode("utf-8")) if result_body else {}
    except json.JSONDecodeError as e:
        raise PublishError("publish: YouTube upload response was not JSON") from e


def publish_youtube(clip_path: Path, video_path: Path, dry_run: bool) -> dict:
    clip = load_clip(clip_path)
    packaging = clip.get("packaging")
    if not isinstance(packaging, dict):
        raise PublishError("publish: clip missing packaging object")

    metadata = build_youtube_metadata(packaging)

    if dry_run:
        description = metadata["snippet"]["description"]
        return {
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

    repo_root = find_repo_root(clip_path) or find_repo_root(Path.cwd())
    creds = load_secrets(repo_root)
    missing = [k for k in YOUTUBE_KEYS if not creds.get(k)]
    if missing:
        raise PublishError(
            "publish: missing YOUTUBE_* credentials — run the auth setup "
            "(see video-engine/README.md)"
        )

    token = refresh_youtube_token(creds)
    result = youtube_resumable_upload(token, video_path, metadata)
    video_id = result.get("id")
    if not video_id:
        raise PublishError("publish: YouTube upload succeeded but no video id in response")

    published_at = utc_now_iso()
    youtube_published = {
        "video_id": video_id,
        "url": f"https://youtu.be/{video_id}",
        "published_at": published_at,
        "privacy": PRIVACY,
    }
    # Re-read clip so we don't clobber concurrent platform write-backs
    clip = load_clip(clip_path)
    merge_published(clip_path, clip, "youtube", youtube_published)

    return {
        "platform": "youtube",
        "status": "ok",
        "video_id": video_id,
        "url": f"https://youtu.be/{video_id}",
        "privacy": PRIVACY,
        "published": {"youtube": youtube_published},
    }


# ── TikTok adapter (Inbox Upload only) ───────────────────────────────────────


def tiktok_source_info(video_size: int) -> dict:
    """Build source_info for inbox FILE_UPLOAD (no post_info).

    Rules (TikTok media transfer guide + issue #231):
      - size < 5 MB → single chunk, chunk_size = video_size, total_chunk_count = 1
      - 5 MB–64 MB → one chunk equal to full size
      - > 64 MB → multi-chunk (nominal 10 MB; final chunk may exceed chunk_size up to 128 MB)
    """
    if video_size <= TIKTOK_MAX_CHUNK:
        return {
            "source": "FILE_UPLOAD",
            "video_size": video_size,
            "chunk_size": video_size,
            "total_chunk_count": 1,
        }
    chunk_size = TIKTOK_MULTI_CHUNK
    # TikTok: total_chunk_count = floor(video_size / chunk_size); last chunk absorbs remainder
    total = max(1, video_size // chunk_size)
    return {
        "source": "FILE_UPLOAD",
        "video_size": video_size,
        "chunk_size": chunk_size,
        "total_chunk_count": total,
    }


def tiktok_chunk_ranges(video_size: int, chunk_size: int, total_chunk_count: int) -> list[tuple[int, int]]:
    """Return list of (offset, length) for each PUT."""
    if total_chunk_count <= 1:
        return [(0, video_size)]
    ranges: list[tuple[int, int]] = []
    offset = 0
    for i in range(total_chunk_count - 1):
        ranges.append((offset, chunk_size))
        offset += chunk_size
    ranges.append((offset, video_size - offset))
    return ranges


def refresh_tiktok_token(creds: dict[str, str]) -> str:
    """Refresh access token; does not write secrets.env."""
    status, data = _http_json_post(
        TIKTOK_TOKEN_URL,
        None,
        {},
        form=True,
        form_fields={
            "client_key": creds["TIKTOK_CLIENT_KEY"],
            "client_secret": creds["TIKTOK_CLIENT_SECRET"],
            "grant_type": "refresh_token",
            "refresh_token": creds["TIKTOK_REFRESH_TOKEN"],
        },
    )
    if not isinstance(data, dict):
        raise PublishError(f"publish: TikTok token refresh failed ({status}): {data!r}"[:200])
    token = data.get("access_token")
    if status != 200 or not token:
        detail = json.dumps(data)[:200]
        raise PublishError(f"publish: TikTok token refresh failed ({status}): {detail}")
    return token


def tiktok_inbox_init(access_token: str, source_info: dict) -> tuple[str, str]:
    """POST inbox init with source_info only (no post_info). Returns (publish_id, upload_url)."""
    status, data = _http_json_post(
        TIKTOK_INBOX_INIT_URL,
        {"source_info": source_info},
        {"Authorization": f"Bearer {access_token}"},
    )
    if not isinstance(data, dict):
        raise PublishError(f"publish: TikTok inbox init failed ({status}): {data!r}"[:300])

    err = data.get("error") or {}
    err_code = err.get("code") if isinstance(err, dict) else None
    if status != 200 or err_code != "ok":
        detail = json.dumps(data)[:300]
        raise PublishError(f"publish: TikTok inbox init failed ({status}): {detail}")

    payload = data.get("data") or {}
    publish_id = payload.get("publish_id")
    upload_url = payload.get("upload_url")
    if not publish_id or not upload_url:
        raise PublishError(
            "publish: TikTok inbox init missing publish_id or upload_url"
        )
    return publish_id, upload_url


def tiktok_put_chunks(
    upload_url: str, video_path: Path, source_info: dict
) -> None:
    """PUT binary chunks to full upload_url (query params included)."""
    video_size = source_info["video_size"]
    chunk_size = source_info["chunk_size"]
    total = source_info["total_chunk_count"]
    ranges = tiktok_chunk_ranges(video_size, chunk_size, total)

    with open(video_path, "rb") as f:
        for i, (offset, length) in enumerate(ranges):
            f.seek(offset)
            chunk = f.read(length)
            if len(chunk) != length:
                raise PublishError(
                    f"publish: TikTok chunk read short at offset {offset} "
                    f"(expected {length}, got {len(chunk)})"
                )
            end = offset + length - 1
            status, body = _http_put(
                upload_url,
                chunk,
                {
                    "Content-Type": "video/mp4",
                    "Content-Length": str(length),
                    "Content-Range": f"bytes {offset}-{end}/{video_size}",
                },
            )
            is_last = i == len(ranges) - 1
            if is_last:
                if status not in (200, 201):
                    detail = body.decode("utf-8", errors="replace")[:300]
                    raise PublishError(
                        f"publish: TikTok chunk upload failed ({status}): {detail}"
                    )
            else:
                # 206 = more chunks expected
                if status not in (206, 200, 201):
                    detail = body.decode("utf-8", errors="replace")[:300]
                    raise PublishError(
                        f"publish: TikTok chunk upload failed ({status}): {detail}"
                    )


def tiktok_status_fetch(access_token: str, publish_id: str) -> dict | None:
    """Optional status poll; returns data dict or None on soft failure."""
    try:
        status, data = _http_json_post(
            TIKTOK_STATUS_URL,
            {"publish_id": publish_id},
            {"Authorization": f"Bearer {access_token}"},
        )
    except PublishError:
        return None
    if not isinstance(data, dict):
        return None
    err = data.get("error") or {}
    if status != 200 or (isinstance(err, dict) and err.get("code") not in (None, "ok")):
        return None
    payload = data.get("data")
    return payload if isinstance(payload, dict) else data


def publish_tiktok(clip_path: Path, video_path: Path, dry_run: bool) -> dict:
    clip = load_clip(clip_path)
    packaging = clip.get("packaging")
    if not isinstance(packaging, dict):
        raise PublishError("publish: clip missing packaging object")

    preview = packaging_preview(packaging)
    # video_size: real stat when file exists; dry-run may use placeholder 0
    if video_path.is_file():
        video_size = video_path.stat().st_size
    else:
        video_size = 0

    source_info = tiktok_source_info(video_size) if video_size > 0 else {
        "source": "FILE_UPLOAD",
        "video_size": "<bytes>",
        "chunk_size": "<bytes>",
        "total_chunk_count": 1,
    }

    if dry_run:
        return {
            "platform": "tiktok",
            "status": "dry_run",
            "mode": "inbox",
            "video_path": str(video_path),
            "video_size": video_size if video_size > 0 else None,
            "source_info": source_info,
            "packaging_preview": preview,
            "plan": [
                "refresh_token",
                "inbox_init",
                "put_chunks",
                "status_fetch",
                "write_published_tiktok_to_clip",
            ],
            "published": {
                "tiktok": {
                    "publish_id": "<publish_id>",
                    "mode": "inbox",
                    "published_at": "<ISO-8601 UTC>",
                    "note": "finish in TikTok app",
                }
            },
            "note": (
                "Inbox Upload only (scope video.upload) — lands in creator drafts; "
                "user finishes caption/privacy in the TikTok app. "
                "Do not send post_info to inbox init. "
                "real upload needs TIKTOK_CLIENT_KEY/SECRET/REFRESH_TOKEN; "
                "dry-run does not write published.tiktok to the clip file. "
                "See video-engine/README.md"
            ),
        }

    if not video_path.is_file():
        raise PublishError(f"publish: video not found: {video_path}")
    if video_size <= 0:
        raise PublishError(f"publish: video is empty: {video_path}")

    repo_root = find_repo_root(clip_path) or find_repo_root(Path.cwd())
    creds = load_secrets(repo_root)
    missing = [k for k in TIKTOK_KEYS if not creds.get(k)]
    if missing:
        raise PublishError(
            "publish: missing TIKTOK_* credentials — set TIKTOK_CLIENT_KEY, "
            "TIKTOK_CLIENT_SECRET, TIKTOK_REFRESH_TOKEN "
            "(see video-engine/README.md)"
        )

    source_info = tiktok_source_info(video_size)
    token = refresh_tiktok_token(creds)
    publish_id, upload_url = tiktok_inbox_init(token, source_info)
    tiktok_put_chunks(upload_url, video_path, source_info)
    status_data = tiktok_status_fetch(token, publish_id)

    published_at = utc_now_iso()
    tiktok_published = {
        "publish_id": publish_id,
        "mode": "inbox",
        "published_at": published_at,
        "note": "finish in TikTok app",
    }
    clip = load_clip(clip_path)
    merge_published(clip_path, clip, "tiktok", tiktok_published)

    out: dict = {
        "platform": "tiktok",
        "status": "ok",
        "mode": "inbox",
        "publish_id": publish_id,
        "published": {"tiktok": tiktok_published},
        "note": "Video is in TikTok inbox/drafts — finish posting in the TikTok app",
    }
    if status_data is not None:
        out["status_fetch"] = status_data
    return out


# ── Instagram adapter (Graph API Reels — resumable / rupload) ────────────────


def build_instagram_caption(packaging: dict) -> str:
    """title + blank line + description + hashtags as #tag; cap at IG_CAPTION_LIMIT."""
    title = (packaging.get("title") or "").strip()
    description = (packaging.get("description") or "").strip()
    tags = tags_from_hashtags(packaging.get("hashtags") or [])
    tag_line = " ".join(f"#{t}" for t in tags)

    parts: list[str] = []
    if title:
        parts.append(title)
    if description:
        if parts:
            parts.append("")  # blank line between title and description
        parts.append(description)
    if tag_line:
        if parts:
            parts.append("")
        parts.append(tag_line)

    caption = "\n".join(parts).strip()
    if not caption:
        raise PublishError(
            "publish: Instagram needs a non-empty caption "
            "(packaging.title and/or packaging.description)"
        )

    if len(caption) <= IG_CAPTION_LIMIT:
        return caption

    # Prefer dropping trailing hashtags before hard-truncating prose
    without_tags = "\n".join(
        p for p in parts if p != tag_line
    ).strip()
    if without_tags and len(without_tags) <= IG_CAPTION_LIMIT:
        return without_tags

    # Drop hashtags one-by-one from the end if still over and tags were inline
    base = without_tags if without_tags else caption
    if len(base) <= IG_CAPTION_LIMIT:
        return base

    # Hard truncate; try not to end mid-word when easy
    cut = base[:IG_CAPTION_LIMIT]
    if len(base) > IG_CAPTION_LIMIT and base[IG_CAPTION_LIMIT : IG_CAPTION_LIMIT + 1] not in (
        " ",
        "\n",
        "",
    ):
        sp = cut.rfind(" ")
        if sp > IG_CAPTION_LIMIT // 2:
            cut = cut[:sp]
    return cut.rstrip()


def maybe_extend_meta_token(creds: dict[str, str]) -> str:
    """Optionally exchange for a long-lived token this session; soft-continue on failure."""
    token = creds["META_ACCESS_TOKEN"]
    app_id = creds.get("META_APP_ID", "").strip()
    app_secret = creds.get("META_APP_SECRET", "").strip()
    if not app_id or not app_secret:
        return token

    qs = urllib.parse.urlencode(
        {
            "grant_type": "fb_exchange_token",
            "client_id": app_id,
            "client_secret": app_secret,
            "fb_exchange_token": token,
        }
    )
    url = f"{META_GRAPH_HOST}/{META_GRAPH_VERSION}/oauth/access_token?{qs}"
    try:
        status, data = _http_json_get(url, {})
    except PublishError as e:
        print(
            f"publish: warning: Meta token extend failed ({e.message}); using original token",
            file=sys.stderr,
        )
        return token

    if not isinstance(data, dict) or status != 200 or not data.get("access_token"):
        detail = json.dumps(data)[:200] if isinstance(data, dict) else repr(data)[:200]
        print(
            f"publish: warning: Meta token extend failed ({status}): {detail}; "
            "using original token",
            file=sys.stderr,
        )
        return token
    return data["access_token"]


def ig_create_resumable_container(
    access_token: str, ig_user_id: str, caption: str
) -> str:
    """POST /{ig-user-id}/media with media_type=REELS, upload_type=resumable. Returns container id."""
    url = f"{META_GRAPH_HOST}/{META_GRAPH_VERSION}/{ig_user_id}/media"
    status, data = _http_json_post(
        url,
        {
            "media_type": "REELS",
            "upload_type": "resumable",
            "caption": caption,
        },
        {"Authorization": f"Bearer {access_token}"},
    )
    if not isinstance(data, dict):
        raise PublishError(
            f"publish: Instagram create container failed ({status}): {data!r}"[:300]
        )
    container_id = data.get("id")
    if status not in (200, 201) or not container_id:
        detail = json.dumps(data)[:300]
        raise PublishError(
            f"publish: Instagram create container failed ({status}): {detail}"
        )
    return str(container_id)


def ig_rupload_bytes(
    access_token: str, container_id: str, video_path: Path
) -> None:
    """POST raw MP4 to rupload.facebook.com/ig-api-upload/{version}/{container_id}."""
    size = video_path.stat().st_size
    if size <= 0:
        raise PublishError(f"publish: video is empty: {video_path}")
    url = f"{META_RUPLOAD_HOST}/ig-api-upload/{META_GRAPH_VERSION}/{container_id}"
    with open(video_path, "rb") as f:
        body = f.read()
    status, data = _http_post_binary(
        url,
        body,
        {
            "Authorization": f"OAuth {access_token}",
            "offset": "0",
            "file_size": str(size),
            "Content-Type": "application/octet-stream",
        },
    )
    if status not in (200, 201):
        detail = json.dumps(data)[:300] if isinstance(data, dict) else repr(data)[:300]
        raise PublishError(f"publish: Instagram rupload failed ({status}): {detail}")
    if isinstance(data, dict) and data.get("success") is False:
        detail = json.dumps(data)[:300]
        raise PublishError(f"publish: Instagram rupload failed: {detail}")


def ig_poll_container_finished(access_token: str, container_id: str) -> None:
    """Poll GET /{container_id}?fields=status_code until FINISHED or error/timeout."""
    url = (
        f"{META_GRAPH_HOST}/{META_GRAPH_VERSION}/{container_id}"
        f"?fields=status_code"
    )
    headers = {"Authorization": f"Bearer {access_token}"}
    deadline = time.monotonic() + IG_POLL_TIMEOUT_S
    interval = IG_POLL_INITIAL_S
    last_payload: dict | str = {}

    while time.monotonic() < deadline:
        status, data = _http_json_get(url, headers)
        last_payload = data
        if not isinstance(data, dict):
            raise PublishError(
                f"publish: Instagram status poll failed ({status}): {data!r}"[:300]
            )
        code = (data.get("status_code") or "").upper()
        if code == "FINISHED":
            return
        if code in ("ERROR", "EXPIRED"):
            detail = json.dumps(data)[:300]
            raise PublishError(
                f"publish: Instagram container status {code}: {detail}"
            )
        # IN_PROGRESS / PUBLISHED / empty — keep waiting
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        time.sleep(min(interval, remaining))
        interval = min(interval + 5, IG_POLL_MAX_S)

    detail = (
        json.dumps(last_payload)[:300]
        if isinstance(last_payload, dict)
        else repr(last_payload)[:300]
    )
    raise PublishError(
        f"publish: Instagram container poll timed out after ~{IG_POLL_TIMEOUT_S}s: {detail}"
    )


def ig_media_publish(access_token: str, ig_user_id: str, container_id: str) -> str:
    """POST /{ig-user-id}/media_publish with creation_id. Returns media_id."""
    url = f"{META_GRAPH_HOST}/{META_GRAPH_VERSION}/{ig_user_id}/media_publish"
    status, data = _http_json_post(
        url,
        {"creation_id": container_id},
        {"Authorization": f"Bearer {access_token}"},
    )
    if not isinstance(data, dict):
        raise PublishError(
            f"publish: Instagram media_publish failed ({status}): {data!r}"[:300]
        )
    media_id = data.get("id")
    if status not in (200, 201) or not media_id:
        detail = json.dumps(data)[:300]
        raise PublishError(
            f"publish: Instagram media_publish failed ({status}): {detail}"
        )
    return str(media_id)


def ig_fetch_permalink(access_token: str, media_id: str) -> str:
    """Optional GET /{media_id}?fields=permalink; empty string on failure."""
    url = (
        f"{META_GRAPH_HOST}/{META_GRAPH_VERSION}/{media_id}?fields=permalink"
    )
    try:
        status, data = _http_json_get(
            url, {"Authorization": f"Bearer {access_token}"}
        )
    except PublishError:
        return ""
    if status != 200 or not isinstance(data, dict):
        return ""
    permalink = data.get("permalink") or ""
    return str(permalink) if permalink else ""


def publish_instagram(clip_path: Path, video_path: Path, dry_run: bool) -> dict:
    clip = load_clip(clip_path)
    packaging = clip.get("packaging")
    if not isinstance(packaging, dict):
        raise PublishError("publish: clip missing packaging object")

    caption = build_instagram_caption(packaging)

    if dry_run:
        return {
            "platform": "instagram",
            "status": "dry_run",
            "video_path": str(video_path),
            "caption_preview": caption,
            "caption_chars": len(caption),
            "plan": [
                "optional_token_extend",
                "create_resumable_container",
                "rupload_bytes",
                "poll_status",
                "media_publish",
                "write_published_instagram_to_clip",
            ],
            "published": {
                "instagram": {
                    "media_id": "<id>",
                    "url": "<permalink or empty>",
                    "published_at": "<ISO-8601 UTC>",
                }
            },
            "note": (
                "real upload needs META_ACCESS_TOKEN + IG_USER_ID; "
                "dry-run does not write published.instagram; "
                "see video-engine/README.md"
            ),
        }

    if not video_path.is_file():
        raise PublishError(f"publish: video not found: {video_path}")

    repo_root = find_repo_root(clip_path) or find_repo_root(Path.cwd())
    creds = load_secrets(repo_root)
    missing = [k for k in INSTAGRAM_KEYS if not creds.get(k)]
    if missing:
        raise PublishError(
            "publish: missing META_ACCESS_TOKEN / IG_USER_ID — "
            "see video-engine/README.md Instagram auth setup"
        )

    token = maybe_extend_meta_token(creds)
    ig_user_id = creds["IG_USER_ID"]
    container_id = ig_create_resumable_container(token, ig_user_id, caption)
    ig_rupload_bytes(token, container_id, video_path)
    ig_poll_container_finished(token, container_id)
    media_id = ig_media_publish(token, ig_user_id, container_id)
    permalink = ig_fetch_permalink(token, media_id)

    published_at = utc_now_iso()
    ig_published = {
        "media_id": media_id,
        "url": permalink,
        "published_at": published_at,
    }
    # Re-read clip so we don't clobber concurrent platform write-backs
    clip = load_clip(clip_path)
    merge_published(clip_path, clip, "instagram", ig_published)

    return {
        "platform": "instagram",
        "status": "ok",
        "media_id": media_id,
        "url": permalink,
        "published": {"instagram": ig_published},
    }


# ── CLI ──────────────────────────────────────────────────────────────────────

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
        clip_path, args.video, require_exists=not args.dry_run
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
