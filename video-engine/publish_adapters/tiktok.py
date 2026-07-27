"""TikTok Content Posting API Inbox Upload adapter (drafts only)."""
from __future__ import annotations

import json
from pathlib import Path

from publish_common import (
    PublishError,
    _http_json_post,
    _http_put,
    find_repo_root,
    load_clip,
    load_secrets,
    merge_published,
    packaging_preview,
    utc_now_iso,
)

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
