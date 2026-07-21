#!/usr/bin/env sh
set -u

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
HOME_DIR=${OPENCHATCUT_HOME:-"$HOME/.openchatcut"}
failed=0

check_command() {
  label=$1
  command_name=$2
  if command -v "$command_name" >/dev/null 2>&1; then
    printf "[ok]   %-24s %s\n" "$label" "$(command -v "$command_name")"
  else
    printf "[miss] %-24s %s\n" "$label" "$command_name"
    failed=1
  fi
}

check_command "Rust toolchain" cargo
check_command "Docker" docker
check_command "Python" python3
check_command "Node.js bridge runtime" node
check_command "Node package installer" npm
check_command "Codex CLI" codex

if command -v ffmpeg >/dev/null 2>&1 && command -v ffprobe >/dev/null 2>&1; then
  echo "[ok]   FFmpeg/ffprobe"
else
  echo "[miss] FFmpeg/ffprobe           run ./scripts/install-ffmpeg.sh"
  failed=1
fi

WORKER=${OPENCHATCUT_MEDIA_WORKER:-"$HOME_DIR/runtime/media-worker/bin/openchatcut-media-worker"}
if [ -x "$WORKER" ]; then
  if PROBE=$(OPENCHATCUT_VIDEO_ACCELERATION="${OPENCHATCUT_VIDEO_ACCELERATION:-auto}" "$WORKER" --probe-capabilities 2>/dev/null); then
    PROBE="$PROBE" python3 -c 'import json, os; p=json.loads(os.environ["PROBE"]); v=p["videoEncoding"]; print("[ok]   Video encoding           requested=%s selected=%s accelerated=%s" % (v["requested"], v["selected"] or "none", str(v["accelerated"]).lower())); print("[info] Video fallback           %s" % v["fallbackReason"] if v.get("fallbackReason") else "")'
  else
    echo "[miss] media worker probe       reinstall with ./scripts/setup.sh"
    failed=1
  fi
else
  echo "[miss] native media worker      run ./scripts/setup.sh"
  failed=1
fi

if [ -x "$HOME_DIR/bin/openchatcutd" ]; then
  echo "[ok]   installed daemon         $HOME_DIR/bin/openchatcutd"
elif [ -x "$ROOT/target/release/openchatcutd" ]; then
  echo "[ok]   release daemon           $ROOT/target/release/openchatcutd"
else
  echo "[miss] installed daemon         run ./scripts/setup.sh"
  failed=1
fi

MG_RUNTIME_DIR=${OPENCHATCUT_MG_RUNTIME_DIR:-"$HOME_DIR/runtime/mg-runtime"}
if [ -f "$MG_RUNTIME_DIR/node_modules/@babel/parser/package.json" ]; then
  echo "[ok]   advanced MG compiler"
else
  echo "[miss] advanced MG compiler     run ./scripts/setup.sh"
  failed=1
fi

if command -v curl >/dev/null 2>&1 && curl --noproxy '*' -fsS http://127.0.0.1:3210/health >/dev/null 2>&1; then
  echo "[ok]   daemon API               http://127.0.0.1:3210"
else
  echo "[info] daemon API               offline"
fi

if command -v codex >/dev/null 2>&1 && codex plugin list --json 2>/dev/null | grep -q 'open-chat-cut'; then
  echo "[ok]   Codex plugin"
else
  echo "[info] Codex plugin             run ./scripts/install-codex-plugin.sh"
fi

exit "$failed"
