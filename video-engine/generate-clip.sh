#!/usr/bin/env bash
# generate-clip.sh — run the full clip pipeline for one clip definition.
#
# Usage: generate-clip.sh <target-repo-path> <clip-json-path> [--skip-server-check]
#
# Config:  <target-repo-path>/video.config.json
# Outputs: <target-repo-path>/video/out/<slug>.mp4 + .jpg
#
# Stdout contract (parsed by `pageseeds-cli video-clip-render`):
#   video-engine: stage=<record|voice|composite> status=start
#   video-engine: stage=<record|voice|composite> status=ok
#   video-engine: output=<absolute path to final mp4>
#   video-engine: thumbnail=<absolute path to thumbnail jpg>
# Stage tools may print additional free-form log lines; only lines starting
# with "video-engine: " are contractual.
#
# Stderr on failure (single line):
#   video-engine: stage=<stage> status=error message=<one-line message>
#
# Exit codes:
#   0 success · 2 bad args/config · 3 dev servers unreachable
#   4 record failed · 5 voice failed · 6 composite failed
set -euo pipefail

ENGINE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PY="$ENGINE_DIR/.venv/bin/python"

err() { echo "video-engine: stage=$1 status=error message=$2" >&2; exit "$3"; }

SKIP_SERVER_CHECK=0
POSITIONAL=()
for arg in "$@"; do
  case "$arg" in
    --skip-server-check) SKIP_SERVER_CHECK=1 ;;
    -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) POSITIONAL+=("$arg") ;;
  esac
done

if [[ ${#POSITIONAL[@]} -ne 2 ]]; then
  err args "usage:_generate-clip.sh_<target-repo-path>_<clip-json-path>" 2
fi

TARGET="$(cd "${POSITIONAL[0]}" 2>/dev/null && pwd)" || err args "target_repo_not_found:_${POSITIONAL[0]}" 2
CLIP="$(cd "$(dirname "${POSITIONAL[1]}")" 2>/dev/null && pwd)/$(basename "${POSITIONAL[1]}")"
CONFIG="$TARGET/video.config.json"
OUT="$TARGET/video/out"

[[ -f "$CONFIG" ]] || err args "config_not_found:_$CONFIG" 2
[[ -f "$CLIP" ]] || err args "clip_not_found:_$CLIP" 2
"$PY" -c "import json,sys; json.load(open(sys.argv[1]))" "$CONFIG" || err args "config_invalid_json:_$CONFIG" 2

# Dev-server reachability check (base_url + first ready_path from config).
if [[ $SKIP_SERVER_CHECK -eq 0 ]]; then
  read -r BASE_URL READY_PATH < <("$PY" -c "
import json, sys
c = json.load(open(sys.argv[1]))
ds = c.get('dev_servers', [])
ready = next((d.get('ready_path') for d in ds if d.get('ready_path')), '/')
print(c.get('base_url', ''), ready)
" "$CONFIG")
  if ! curl -sf -o /dev/null --max-time 10 "$BASE_URL$READY_PATH"; then
    HINTS=$("$PY" -c "
import json, sys
c = json.load(open(sys.argv[1]))
print(';_'.join(f\"(cd_{d['cwd']}_&&_{d['command']})\" for d in c.get('dev_servers', [])))
" "$CONFIG")
    err check "dev_servers_unreachable:_$BASE_URL$READY_PATH-start_them_first:_$HINTS" 3
  fi
fi

SLUG="$("$PY" -c "import json,sys; print(json.load(open(sys.argv[1]))['source']['slug'])" "$CLIP")"

run_stage() {
  local name="$1" code="$2"; shift 2
  echo "video-engine: stage=$name status=start"
  if "$@"; then
    echo "video-engine: stage=$name status=ok"
  else
    err "$name" "${name}_failed_see_log_above" "$code"
  fi
}

run_stage record 4 node "$ENGINE_DIR/record.mjs" "$CLIP" --config "$CONFIG" --out "$OUT"
run_stage voice 5 "$PY" "$ENGINE_DIR/voice.py" "$CLIP" --config "$CONFIG" --out "$OUT"
run_stage composite 6 "$PY" "$ENGINE_DIR/composite.py" "$CLIP" --config "$CONFIG" --out "$OUT"

echo "video-engine: output=$OUT/$SLUG.mp4"
echo "video-engine: thumbnail=$OUT/$SLUG.jpg"
