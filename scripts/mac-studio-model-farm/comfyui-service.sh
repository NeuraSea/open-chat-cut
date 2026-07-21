#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
COMFY="$ROOT/runtime/comfyui"
PYTHON="$COMFY/.venv/bin/python"
PID_FILE="$ROOT/runtime/comfyui.pid"
LOG_ROOT=${OPENCHATCUT_MODEL_FARM_LOG_ROOT:-$HOME/Library/Logs/OpenChatCut}
STDOUT_LOG="$LOG_ROOT/comfyui.log"
STDERR_LOG="$LOG_ROOT/comfyui.err.log"
URL=http://127.0.0.1:8188/system_stats

command_line=(
  "$PYTHON"
  "$COMFY/main.py"
  --base-directory "$COMFY"
  --listen 127.0.0.1
  --port 8188
  --disable-auto-launch
  --preview-method none
  --temp-directory "$ROOT/cache/comfyui"
  --output-directory "$ROOT/output/comfyui"
  --user-directory "$ROOT/config/comfyui-user"
  --database-url "sqlite:///$ROOT/config/comfyui.sqlite3"
)

read_pid() {
  [[ -f "$PID_FILE" ]] || return 1
  local pid
  pid=$(cat "$PID_FILE")
  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  printf '%s\n' "$pid"
}

is_ready() {
  curl --fail --silent --show-error --max-time 3 "$URL" >/dev/null
}

start() {
  if is_ready; then
    echo "ComfyUI is already ready on 127.0.0.1:8188"
    return
  fi
  if [[ ! -x "$PYTHON" ]]; then
    echo "ComfyUI virtual environment is missing at $PYTHON" >&2
    exit 2
  fi
  mkdir -p \
    "$LOG_ROOT" \
    "$ROOT/cache/comfyui" \
    "$ROOT/output/comfyui" \
    "$ROOT/config/comfyui-user"
  nohup env \
    PYTHONUNBUFFERED=1 \
    PYTORCH_ENABLE_MPS_FALLBACK=1 \
    HF_HOME="$ROOT/cache/huggingface" \
    "${command_line[@]}" \
    >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null &
  local pid=$!
  printf '%s\n' "$pid" >"$PID_FILE"
  for _ in $(seq 1 90); do
    if is_ready; then
      echo "ComfyUI is ready (pid $pid)"
      return
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "ComfyUI exited before becoming ready" >&2
      tail -80 "$STDERR_LOG" >&2 || true
      exit 1
    fi
    sleep 1
  done
  echo "ComfyUI did not become ready within 90 seconds" >&2
  exit 1
}

stop() {
  local pid
  if ! pid=$(read_pid); then
    echo "ComfyUI is not running"
    return
  fi
  local process
  process=$(ps -p "$pid" -o command= 2>/dev/null || true)
  if [[ "$process" != *"$COMFY/main.py"* ]]; then
    echo "Refusing to stop pid $pid because it is not the managed ComfyUI process" >&2
    exit 1
  fi
  kill "$pid"
  for _ in $(seq 1 30); do
    kill -0 "$pid" 2>/dev/null || {
      echo "ComfyUI stopped"
      return
    }
    sleep 1
  done
  echo "ComfyUI did not stop within 30 seconds" >&2
  exit 1
}

status() {
  local pid=unknown
  pid=$(read_pid 2>/dev/null || echo unknown)
  if is_ready; then
    echo "ready pid=$pid url=$URL"
    return
  fi
  echo "not-ready pid=$pid url=$URL"
  exit 1
}

case "${1:-status}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status) status ;;
  foreground)
    export PYTHONUNBUFFERED=1 PYTORCH_ENABLE_MPS_FALLBACK=1
    export HF_HOME="$ROOT/cache/huggingface"
    exec "${command_line[@]}"
    ;;
  *)
    echo "usage: $0 {start|stop|restart|status|foreground}" >&2
    exit 2
    ;;
esac
