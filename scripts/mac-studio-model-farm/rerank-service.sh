#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
RUNTIME="$ROOT/runtime/rerank"
PID_FILE="$RUNTIME/service.pid"
LOG_FILE="$ROOT/logs/rerank-service.log"
TOKEN_FILE="$ROOT/config/rerank.token"
PORT=${OPENCHATCUT_RERANK_PORT:-8190}

is_running() {
  [[ -f "$PID_FILE" ]] || return 1
  local pid
  pid=$(cat "$PID_FILE" 2>/dev/null || true)
  [[ "$pid" =~ ^[0-9]+$ ]] && kill -0 "$pid" 2>/dev/null
}

start() {
  if is_running; then
    echo "rerank service is already running"
    return
  fi
  [[ -x "$RUNTIME/.venv/bin/python" ]] || {
    echo "rerank runtime is not installed" >&2
    exit 2
  }
  [[ -s "$TOKEN_FILE" ]] || {
    echo "rerank service token is missing" >&2
    exit 2
  }
  mkdir -p "$(dirname "$LOG_FILE")"
  nohup env \
    HF_HUB_OFFLINE=1 \
    HF_HOME="$ROOT/cache/huggingface" \
    "$RUNTIME/.venv/bin/python" "$RUNTIME/rerank-service.py" \
      --host 127.0.0.1 \
      --port "$PORT" \
      --model-path "$RUNTIME/model" \
      --token-file "$TOKEN_FILE" \
      >>"$LOG_FILE" 2>&1 </dev/null &
  echo $! >"$PID_FILE"
  for _ in $(seq 1 180); do
    if curl --fail --silent --max-time 2 \
      "http://127.0.0.1:$PORT/health" >/dev/null; then
      echo "rerank service is ready"
      return
    fi
    if ! is_running; then
      tail -80 "$LOG_FILE" >&2 || true
      exit 1
    fi
    sleep 2
  done
  echo "rerank service did not become ready" >&2
  exit 1
}

stop() {
  if ! is_running; then
    echo "rerank service is not running"
    return
  fi
  local pid
  pid=$(cat "$PID_FILE")
  kill "$pid"
  for _ in $(seq 1 30); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 1
  done
  if kill -0 "$pid" 2>/dev/null; then
    echo "rerank service did not stop cleanly" >&2
    exit 1
  fi
  echo "rerank service stopped"
}

status() {
  if is_running && curl --fail --silent --max-time 2 \
    "http://127.0.0.1:$PORT/health" >/dev/null; then
    echo "rerank service is ready"
  else
    echo "rerank service is not ready"
    return 1
  fi
}

case "${1:-status}" in
  start) start ;;
  stop) stop ;;
  restart) stop || true; start ;;
  status) status ;;
  *) echo "usage: $0 {start|stop|restart|status}" >&2; exit 2 ;;
esac
