"""YouTube Data API v3 resumable upload adapter."""
from __future__ import annotations

import json
import sys
import urllib.error
import urllib.request
from pathlib import Path

from publish_common import (
    PublishError,
    _http_json_post,
    _http_put,
    find_repo_root,
    load_clip,
    load_secrets,
    merge_published,
    tags_from_hashtags,
    utc_now_iso,
)

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
