#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
PYTHON=${OPENCHATCUT_IMAGE_PYTHON:-/usr/bin/python3}
SERVICE="$ROOT/runtime/image-service.py"
TOKEN_FILE="$ROOT/config/image.token"
OUTPUT_ROOT="$ROOT/runtime/comfyui/output"
PID_FILE="$ROOT/runtime/image-service.pid"
LOG_ROOT=${OPENCHATCUT_MODEL_FARM_LOG_ROOT:-$HOME/Library/Logs/OpenChatCut}
STDOUT_LOG="$LOG_ROOT/image-service.log"
STDERR_LOG="$LOG_ROOT/image-service.err.log"
URL=http://127.0.0.1:8193/health

read_pid() {
  [[ -f "$PID_FILE" ]] || return 1
  local pid
  pid=$(cat "$PID_FILE")
  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  printf '%s\n' "$pid"
}
is_ready() { curl --fail --silent --max-time 3 "$URL" >/dev/null; }
start() {
  if is_ready; then echo "Image service is already ready on 127.0.0.1:8193"; return; fi
  if [[ ! -x "$PYTHON" || ! -f "$SERVICE" || ! -f "$TOKEN_FILE" || ! -d "$OUTPUT_ROOT" ]]; then
    echo "Image service, token, Python, or ComfyUI output root is missing" >&2
    exit 2
  fi
  curl --fail --silent --max-time 5 http://127.0.0.1:8188/system_stats >/dev/null || {
    echo "ComfyUI is not ready on 127.0.0.1:8188" >&2; exit 2;
  }
  mkdir -p "$LOG_ROOT"
  nohup "$PYTHON" "$SERVICE" --token-file "$TOKEN_FILE" --output-root "$OUTPUT_ROOT" \
    >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null &
  local pid=$!
  printf '%s\n' "$pid" >"$PID_FILE"
  for _ in $(seq 1 30); do
    if is_ready; then echo "Image service is ready (pid $pid)"; return; fi
    kill -0 "$pid" 2>/dev/null || {
      echo "Image service exited before becoming ready" >&2
      tail -80 "$STDERR_LOG" >&2 || true
      exit 1
    }
    sleep 1
  done
  echo "Image service did not become ready within 30 seconds" >&2
  exit 1
}
stop() {
  local pid
  if ! pid=$(read_pid); then echo "Image service is not running"; return; fi
  local process
  process=$(ps -p "$pid" -o command= 2>/dev/null || true)
  [[ "$process" == *"image-service.py"* ]] || {
    echo "Refusing to stop pid $pid because it is not the managed image service" >&2; exit 1;
  }
  kill "$pid"
  for _ in $(seq 1 30); do
    kill -0 "$pid" 2>/dev/null || { echo "Image service stopped"; return; }
    sleep 1
  done
  echo "Image service did not stop within 30 seconds" >&2
  exit 1
}
case "${1:-status}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status) if is_ready; then echo "ready url=$URL"; else echo "not-ready url=$URL"; exit 1; fi ;;
  *) echo "usage: $0 {start|stop|restart|status}" >&2; exit 2 ;;
esac
