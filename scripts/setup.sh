#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
HOME_DIR=${OPENCHATCUT_HOME:-"$HOME/.openchatcut"}
RUNTIME_DIR="$HOME_DIR/runtime"
WORKER_VENV="$RUNTIME_DIR/media-worker"
MG_RUNTIME_DIR="$RUNTIME_DIR/mg-runtime"
WITH_ML=1
BUILD_WEB=1

for argument in "$@"; do
  case "$argument" in
    --without-ml) WITH_ML=0 ;;
    --skip-web) BUILD_WEB=0 ;;
    *) echo "Unknown option: $argument" >&2; exit 2 ;;
  esac
done

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

has_system_chromium() {
  for candidate in \
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
    "/Applications/Chromium.app/Contents/MacOS/Chromium" \
    /usr/bin/google-chrome /usr/bin/google-chrome-stable \
    /usr/bin/chromium /usr/bin/chromium-browser; do
    [ -x "$candidate" ] && return 0
  done
  return 1
}

need cargo
need python3
need node
need npm
if [ "$BUILD_WEB" -eq 1 ]; then
  need docker
fi

echo "[0/3] Ensuring FFmpeg/ffprobe are available"
"$ROOT/scripts/install-ffmpeg.sh"

echo "[1/3] Building the native daemon"
(cd "$ROOT" && cargo build --release -p openchatcut-daemon --bin openchatcutd)
mkdir -p "$MG_RUNTIME_DIR/src"
cp "$ROOT/packages/mg-runtime/package.json" "$MG_RUNTIME_DIR/package.json"
cp "$ROOT/packages/mg-runtime/src/"*.mjs "$MG_RUNTIME_DIR/src/"
npm install --prefix "$MG_RUNTIME_DIR" --omit=dev --ignore-scripts --no-package-lock

echo "[2/3] Preparing the native media worker"
mkdir -p "$RUNTIME_DIR"
if [ ! -x "$WORKER_VENV/bin/python" ]; then
  python3 -m venv "$WORKER_VENV"
fi
"$WORKER_VENV/bin/python" -m pip install --disable-pip-version-check --upgrade pip
if [ "$WITH_ML" -eq 1 ]; then
  "$WORKER_VENV/bin/python" -m pip install --disable-pip-version-check "$ROOT/services/media-worker[transcription,diarization,render]"
else
  "$WORKER_VENV/bin/python" -m pip install --disable-pip-version-check "$ROOT/services/media-worker[render]"
fi
if has_system_chromium; then
  echo "Using the installed Chrome/Chromium browser for headless rendering"
else
  "$WORKER_VENV/bin/python" -m playwright install chromium
fi

if [ "$BUILD_WEB" -eq 1 ]; then
  echo "[3/3] Building the Web editor image"
  (cd "$ROOT" && docker compose build web)
else
  echo "[3/3] Skipping the Web editor image"
fi

echo
echo "OpenChatCut is installed. Next:"
echo "  1. codex login"
echo "  2. ./scripts/openchatcut.sh start"
echo "  3. ./scripts/install-codex-plugin.sh"
