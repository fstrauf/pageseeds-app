"""Instagram Reels adapter — Instagram Login + Facebook Login paths.

Instagram Login (tokens starting with IGAA…):
  Host graph.instagram.com. Local MP4 requires a publicly fetchable video_url
  (Meta curls the file). We optionally stage the file on a short-lived public
  host (litterbox) when no INSTAGRAM_VIDEO_URL is set. Resumable rupload is
  not available on this login path.

Facebook Login (classic EAA… user tokens):
  Host graph.facebook.com + rupload.facebook.com (upload_type=resumable).
"""
from __future__ import annotations

import json
import mimetypes
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
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
META_GRAPH_HOST_FB = "https://graph.facebook.com"
META_GRAPH_HOST_IG = "https://graph.instagram.com"
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

# Short-lived public staging for Instagram Login (Meta must HTTP-GET the MP4).
# litterbox: free, no account, 1h/12h/24h/72h TTL. Override with INSTAGRAM_STAGE_HOST=none
# to force INSTAGRAM_VIDEO_URL / fail instead of staging.
DEFAULT_STAGE_ENDPOINT = (
    "https://litterbox.catbox.moe/resources/internals/api.php"
)
DEFAULT_STAGE_TTL = "1h"


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


def is_instagram_login_token(token: str) -> bool:
    """IG user tokens from Instagram Login typically start with IGAA."""
    t = (token or "").strip()
    return t.upper().startswith("IGAA")


def graph_host_for_token(token: str) -> str:
    return META_GRAPH_HOST_IG if is_instagram_login_token(token) else META_GRAPH_HOST_FB


def maybe_extend_meta_token(creds: dict[str, str]) -> str:
    """Refresh/extend token for this session; soft-continue on failure."""
    token = creds["META_ACCESS_TOKEN"]
    if is_instagram_login_token(token):
        return _maybe_refresh_instagram_login_token(token, creds)
    return _maybe_extend_facebook_login_token(token, creds)


def _maybe_refresh_instagram_login_token(token: str, creds: dict[str, str]) -> str:
    """Long-lived IG token refresh, or short→long exchange when app secret present."""
    app_secret = creds.get("META_APP_SECRET", "").strip()
    # Prefer refresh of an existing long-lived token (no secret required).
    qs_refresh = urllib.parse.urlencode(
        {
            "grant_type": "ig_refresh_token",
            "access_token": token,
        }
    )
    url = f"{META_GRAPH_HOST_IG}/refresh_access_token?{qs_refresh}"
    try:
        status, data = _http_json_get(url, {})
        if isinstance(data, dict) and status == 200 and data.get("access_token"):
            return str(data["access_token"])
    except PublishError as e:
        print(
            f"publish: warning: Instagram token refresh failed ({e.message}); "
            "trying exchange or original token",
            file=sys.stderr,
        )

    if not app_secret:
        return token

    qs = urllib.parse.urlencode(
        {
            "grant_type": "ig_exchange_token",
            "client_secret": app_secret,
            "access_token": token,
        }
    )
    url = f"{META_GRAPH_HOST_IG}/access_token?{qs}"
    try:
        status, data = _http_json_get(url, {})
    except PublishError as e:
        print(
            f"publish: warning: Instagram token exchange failed ({e.message}); "
            "using original token",
            file=sys.stderr,
        )
        return token

    if not isinstance(data, dict) or status != 200 or not data.get("access_token"):
        detail = json.dumps(data)[:200] if isinstance(data, dict) else repr(data)[:200]
        print(
            f"publish: warning: Instagram token exchange failed ({status}): {detail}; "
            "using original token",
            file=sys.stderr,
        )
        return token
    return str(data["access_token"])


def _maybe_extend_facebook_login_token(token: str, creds: dict[str, str]) -> str:
    """Optional Facebook long-lived exchange (fb_exchange_token)."""
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
    url = f"{META_GRAPH_HOST_FB}/{META_GRAPH_VERSION}/oauth/access_token?{qs}"
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
    return str(data["access_token"])


def resolve_ig_user_id(access_token: str, creds: dict[str, str]) -> str:
    """Prefer IG_USER_ID secret; fall back to graph /me for Instagram Login."""
    configured = (creds.get("IG_USER_ID") or "").strip()
    if configured:
        return configured

    host = graph_host_for_token(access_token)
    url = f"{host}/{META_GRAPH_VERSION}/me?fields=id,username"
    status, data = _http_json_get(
        url, {"Authorization": f"Bearer {access_token}"}
    )
    if (
        status == 200
        and isinstance(data, dict)
        and data.get("id")
    ):
        return str(data["id"])
    raise PublishError(
        "publish: missing IG_USER_ID and could not resolve via /me — "
        "see video-engine/README.md Instagram auth setup"
    )


def stage_local_video_public(video_path: Path) -> str:
    """Upload local MP4 to a short-lived public host; return HTTPS URL for Meta to fetch.

    Uses litterbox.catbox.moe by default (1h TTL). Override endpoint/TTL via
    INSTAGRAM_STAGE_ENDPOINT / INSTAGRAM_STAGE_TTL. Set INSTAGRAM_STAGE_HOST=none
    to disable staging (then INSTAGRAM_VIDEO_URL is required).
    """
    mode = (os.environ.get("INSTAGRAM_STAGE_HOST") or "litterbox").strip().lower()
    if mode in ("none", "off", "0", "false"):
        raise PublishError(
            "publish: Instagram Login needs a public video_url — set INSTAGRAM_VIDEO_URL "
            "or enable staging (INSTAGRAM_STAGE_HOST=litterbox)"
        )

    endpoint = (
        os.environ.get("INSTAGRAM_STAGE_ENDPOINT") or DEFAULT_STAGE_ENDPOINT
    ).strip()
    ttl = (os.environ.get("INSTAGRAM_STAGE_TTL") or DEFAULT_STAGE_TTL).strip()

    size = video_path.stat().st_size
    if size <= 0:
        raise PublishError(f"publish: video is empty: {video_path}")
    # litterbox free limit is roughly 1 GB; our shorts are tiny.
    if size > 200 * 1024 * 1024:
        raise PublishError(
            f"publish: video too large for temp staging ({size} bytes); "
            "host it yourself and set INSTAGRAM_VIDEO_URL"
        )

    print(
        f"publish: staging {video_path.name} ({size} bytes) for Instagram Login "
        f"via public temp host (ttl={ttl})…",
        file=sys.stderr,
    )

    boundary = f"----pageseeds{int(time.time())}{os.getpid()}"
    filename = video_path.name or "clip.mp4"
    content_type = mimetypes.guess_type(filename)[0] or "video/mp4"
    file_bytes = video_path.read_bytes()

    def part(name: str, value: str) -> bytes:
        return (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="{name}"\r\n\r\n'
            f"{value}\r\n"
        ).encode("utf-8")

    body = b"".join(
        [
            part("reqtype", "fileupload"),
            part("time", ttl),
            (
                f"--{boundary}\r\n"
                f'Content-Disposition: form-data; name="fileToUpload"; '
                f'filename="{filename}"\r\n'
                f"Content-Type: {content_type}\r\n\r\n"
            ).encode("utf-8"),
            file_bytes,
            f"\r\n--{boundary}--\r\n".encode("utf-8"),
        ]
    )

    req = urllib.request.Request(
        endpoint,
        data=body,
        method="POST",
        headers={
            "Content-Type": f"multipart/form-data; boundary={boundary}",
            "Content-Length": str(len(body)),
            "User-Agent": "pageseeds-publish/1.0",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=300) as resp:
            raw = resp.read().decode("utf-8", errors="replace").strip()
            status = resp.status
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", errors="replace")
        raise PublishError(
            f"publish: temp video staging failed ({e.code}): {raw[:300]}"
        ) from e
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        raise PublishError(f"publish: temp video staging failed: {e}") from e

    if status not in (200, 201) or not raw.startswith("http"):
        raise PublishError(
            f"publish: temp video staging returned unexpected response ({status}): "
            f"{raw[:300]}"
        )
    print(f"publish: staged video_url ready ({raw[:60]}…)", file=sys.stderr)
    return raw


def resolve_public_video_url(video_path: Path) -> str:
    """Public HTTPS URL Meta can fetch. Prefer INSTAGRAM_VIDEO_URL, else stage local file."""
    override = (os.environ.get("INSTAGRAM_VIDEO_URL") or "").strip()
    if override:
        if not override.startswith("https://"):
            raise PublishError(
                "publish: INSTAGRAM_VIDEO_URL must be an https:// URL Meta can fetch"
            )
        return override
    return stage_local_video_public(video_path)


def ig_create_container_video_url(
    access_token: str,
    ig_user_id: str,
    caption: str,
    video_url: str,
    graph_host: str,
) -> str:
    """POST /{ig-user-id}/media with media_type=REELS and video_url (Instagram Login)."""
    url = f"{graph_host}/{META_GRAPH_VERSION}/{ig_user_id}/media"
    status, data = _http_json_post(
        url,
        {
            "media_type": "REELS",
            "video_url": video_url,
            "caption": caption,
            "share_to_feed": "true",
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


def ig_create_resumable_container(
    access_token: str, ig_user_id: str, caption: str, graph_host: str
) -> str:
    """POST /{ig-user-id}/media with media_type=REELS, upload_type=resumable (FB Login)."""
    url = f"{graph_host}/{META_GRAPH_VERSION}/{ig_user_id}/media"
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


def ig_poll_container_finished(
    access_token: str, container_id: str, graph_host: str
) -> None:
    """Poll GET /{container_id}?fields=status_code until FINISHED or error/timeout."""
    url = (
        f"{graph_host}/{META_GRAPH_VERSION}/{container_id}"
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


def ig_media_publish(
    access_token: str, ig_user_id: str, container_id: str, graph_host: str
) -> str:
    """POST /{ig-user-id}/media_publish with creation_id. Returns media_id."""
    url = f"{graph_host}/{META_GRAPH_VERSION}/{ig_user_id}/media_publish"
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


def ig_fetch_permalink(
    access_token: str, media_id: str, graph_host: str
) -> str:
    """Optional GET /{media_id}?fields=permalink; empty string on failure."""
    url = f"{graph_host}/{META_GRAPH_VERSION}/{media_id}?fields=permalink"
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

    # Peek token only to shape dry-run plan (secrets optional for dry-run).
    repo_root = find_repo_root(clip_path) or find_repo_root(Path.cwd())
    peek = load_secrets(repo_root)
    peek_token = (peek.get("META_ACCESS_TOKEN") or "").strip()
    login_path = (
        "instagram_login"
        if (not peek_token or is_instagram_login_token(peek_token))
        else "facebook_login"
    )

    if dry_run:
        if login_path == "instagram_login":
            plan = [
                "optional_token_refresh",
                "resolve_public_video_url_or_stage_local",
                "create_reels_container_video_url",
                "poll_status",
                "media_publish",
                "write_published_instagram_to_clip",
            ]
            note = (
                "Instagram Login path (graph.instagram.com): needs META_ACCESS_TOKEN "
                "(IGAA…) + IG_USER_ID; local MP4 is staged to a short-lived public URL "
                "unless INSTAGRAM_VIDEO_URL is set; dry-run does not write "
                "published.instagram; see video-engine/README.md"
            )
        else:
            plan = [
                "optional_token_extend",
                "create_resumable_container",
                "rupload_bytes",
                "poll_status",
                "media_publish",
                "write_published_instagram_to_clip",
            ]
            note = (
                "Facebook Login path (graph.facebook.com + rupload): needs "
                "META_ACCESS_TOKEN + IG_USER_ID; dry-run does not write "
                "published.instagram; see video-engine/README.md"
            )
        return {
            "platform": "instagram",
            "status": "dry_run",
            "login_path": login_path,
            "video_path": str(video_path),
            "caption_preview": caption,
            "caption_chars": len(caption),
            "plan": plan,
            "published": {
                "instagram": {
                    "media_id": "<id>",
                    "url": "<permalink or empty>",
                    "published_at": "<ISO-8601 UTC>",
                }
            },
            "note": note,
        }

    if not video_path.is_file():
        raise PublishError(f"publish: video not found: {video_path}")

    creds = load_secrets(repo_root)
    if not (creds.get("META_ACCESS_TOKEN") or "").strip():
        raise PublishError(
            "publish: missing META_ACCESS_TOKEN — "
            "see video-engine/README.md Instagram auth setup"
        )

    token = maybe_extend_meta_token(creds)
    ig_user_id = resolve_ig_user_id(token, creds)
    graph_host = graph_host_for_token(token)
    use_ig_login = is_instagram_login_token(token)

    if use_ig_login:
        video_url = resolve_public_video_url(video_path)
        container_id = ig_create_container_video_url(
            token, ig_user_id, caption, video_url, graph_host
        )
    else:
        container_id = ig_create_resumable_container(
            token, ig_user_id, caption, graph_host
        )
        ig_rupload_bytes(token, container_id, video_path)

    ig_poll_container_finished(token, container_id, graph_host)
    media_id = ig_media_publish(token, ig_user_id, container_id, graph_host)
    permalink = ig_fetch_permalink(token, media_id, graph_host)

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
        "login_path": "instagram_login" if use_ig_login else "facebook_login",
        "media_id": media_id,
        "url": permalink,
        "published": {"instagram": ig_published},
    }
