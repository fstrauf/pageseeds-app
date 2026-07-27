"""Instagram Graph API Reels adapter (resumable / rupload)."""
from __future__ import annotations

import json
import sys
import time
import urllib.parse
from pathlib import Path

from publish_common import (
    PublishError,
    _http_json_get,
    _http_json_post,
    _http_post_binary,
    find_repo_root,
    load_clip,
    load_secrets,
    merge_published,
    tags_from_hashtags,
    utc_now_iso,
)

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
    without_tags = "\n".join(p for p in parts if p != tag_line).strip()
    if without_tags and len(without_tags) <= IG_CAPTION_LIMIT:
        return without_tags

    # Hard truncate base (prose without tags, or full caption if no prose)
    base = without_tags if without_tags else caption
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
