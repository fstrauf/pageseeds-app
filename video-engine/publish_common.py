"""Shared publish helpers: secrets, clip I/O, HTTP, and packaging utils.

Used by publish.py (CLI) and publish_adapters/* (platform code).
Stdlib only.
"""
from __future__ import annotations

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

# Secret key names known to load_secrets process-env fallback.
# Platform-specific KEYS tuples live in each adapter; this is the union.
ALL_SECRET_KEYS = (
    "YOUTUBE_CLIENT_ID",
    "YOUTUBE_CLIENT_SECRET",
    "YOUTUBE_REFRESH_TOKEN",
    "TIKTOK_CLIENT_KEY",
    "TIKTOK_CLIENT_SECRET",
    "TIKTOK_REFRESH_TOKEN",
    "META_ACCESS_TOKEN",
    "IG_USER_ID",
    "META_APP_ID",
    "META_APP_SECRET",
)

# Config-ish keys overlaid with the same namespacing as secrets (not credentials).
# Included so multi-brand expected channel (YOUTUBE_CHANNEL / {PROJECT}_YOUTUBE_CHANNEL)
# resolves via load_secrets without a second env-file pass.
OVERLAY_CONFIG_KEYS = (
    "YOUTUBE_CHANNEL",
)

# Keys load_secrets overlays from files + process env + namespaced project keys.
OVERLAY_KEYS = ALL_SECRET_KEYS + OVERLAY_CONFIG_KEYS

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


def normalize_project_id(project_id) -> str | None:
    """Coerce project_id to a stripped string for namespacing; None/empty → None."""
    if project_id is None:
        return None
    s = str(project_id).strip()
    return s if s else None


def channel_identity_matches(expected: str, title: str, custom_url: str) -> bool:
    """True when expected equals channel title or customUrl after normalize.

    Normalize: casefold, strip, strip leading ``@``.
    """
    exp = _normalize_channel_identity(expected)
    if not exp:
        return False
    return exp == _normalize_channel_identity(title) or exp == _normalize_channel_identity(
        custom_url
    )


def _normalize_channel_identity(value: str) -> str:
    s = (value or "").strip().casefold()
    if s.startswith("@"):
        s = s[1:].strip()
    return s


def load_secrets(repo_root: Path | None, project_id: str | None = None) -> dict[str, str]:
    """Env-file chain matching Rust EnvResolver: first file wins per key, then process env.

    Multi-brand: keys may be namespaced per project as {PROJECT_ID_UPPER}_{KEY}
    (e.g. COFFEE_YOUTUBE_REFRESH_TOKEN). A namespaced value wins over the
    un-namespaced default for that project only.

    Also overlays OVERLAY_CONFIG_KEYS (e.g. YOUTUBE_CHANNEL) the same way.
    Warns on stderr when a namespaced YOUTUBE_REFRESH_TOKEN equals the default
    (likely copy-paste wrong-brand token); does not fail.
    """
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

    for key in OVERLAY_KEYS:
        if key not in resolved:
            env_val = os.environ.get(key, "").strip()
            if env_val:
                resolved[key] = env_val

    project_id = normalize_project_id(project_id)
    if project_id:
        prefix = project_id.upper().replace("-", "_") + "_"
        default_yt_refresh = (resolved.get("YOUTUBE_REFRESH_TOKEN") or "").strip()
        for key in OVERLAY_KEYS:
            namespaced = resolved.get(prefix + key) or os.environ.get(prefix + key, "").strip()
            if namespaced:
                if (
                    key == "YOUTUBE_REFRESH_TOKEN"
                    and default_yt_refresh
                    and namespaced == default_yt_refresh
                ):
                    print(
                        f"publish: WARNING: {prefix}YOUTUBE_REFRESH_TOKEN equals "
                        "YOUTUBE_REFRESH_TOKEN — namespaced token identical to default; "
                        "likely wrong brand",
                        file=sys.stderr,
                    )
                resolved[key] = namespaced
    return resolved


def resolve_video_path(
    clip_path: Path,
    video_flag: str | None,
    *,
    require_exists: bool = True,
    engine_dir: Path | None = None,
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
    # Fallback: video-engine/out/<slug>.mp4 next to the publish CLI / engine
    base = engine_dir if engine_dir is not None else Path(__file__).resolve().parent
    candidates.append(base / "out" / f"{slug}.mp4")

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


# ── HTTP helpers ─────────────────────────────────────────────────────────────


def _http_raw(
    method: str,
    url: str,
    body: bytes | None,
    headers: dict[str, str],
    *,
    timeout: int = 600,
) -> tuple[int, bytes]:
    """Low-level request via http.client so non-2xx (308/206) are not treated as redirects."""
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
        hdrs = dict(headers)
        if body is not None and "Content-Length" not in hdrs:
            hdrs["Content-Length"] = str(len(body))
        conn.request(method, path, body=body, headers=hdrs)
        resp = conn.getresponse()
        data = resp.read()
        return resp.status, data
    except (OSError, TimeoutError) as e:
        raise PublishError(f"publish: {method} request failed: {e}") from e
    finally:
        conn.close()


def _http_put(upload_url: str, body: bytes, headers: dict[str, str]) -> tuple[int, bytes]:
    """PUT via http.client so non-2xx (308/206) are not treated as redirects."""
    return _http_raw("PUT", upload_url, body, headers, timeout=600)


def _http_post_binary(
    url: str,
    body: bytes,
    headers: dict[str, str],
    *,
    timeout: int = 600,
) -> tuple[int, dict | str]:
    """POST raw binary body; return (status, parsed_json_or_text)."""
    status, raw_bytes = _http_raw("POST", url, body, headers, timeout=timeout)
    raw = raw_bytes.decode("utf-8", errors="replace")
    try:
        return status, json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        return status, raw


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
