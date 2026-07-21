#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
RUNTIME="$ROOT/runtime/whisperx"
PYTHON="$RUNTIME/.venv/bin/python"
SERVICE="$ROOT/runtime/asr-service.py"
TOKEN_FILE="$ROOT/config/asr.token"
PID_FILE="$ROOT/runtime/asr-service.pid"
LOG_ROOT=${OPENCHATCUT_MODEL_FARM_LOG_ROOT:-$HOME/Library/Logs/OpenChatCut}
STDOUT_LOG="$LOG_ROOT/asr-service.log"
STDERR_LOG="$LOG_ROOT/asr-service.err.log"
URL=http://127.0.0.1:8191/health
MODEL=${OPENCHATCUT_ASR_MODEL:-$ROOT/runtime/whisperx/models/faster-whisper-large-v3}

read_pid() {
  [[ -f "$PID_FILE" ]] || return 1
  local pid
  pid=$(cat "$PID_FILE")
  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  printf '%s\n' "$pid"
}

is_ready() {
  curl --fail --silent --max-time 3 "$URL" >/dev/null
}

start() {
  if is_ready; then
    echo "ASR service is already ready on 127.0.0.1:8191"
    return
  fi
  if [[ ! -x "$PYTHON" || ! -f "$SERVICE" || ! -f "$TOKEN_FILE" ]]; then
    echo "ASR runtime, service, or token is missing" >&2
    exit 2
  fi
  mkdir -p \
    "$LOG_ROOT" \
    "$ROOT/cache/whisperx" \
    "$ROOT/cache/whisperx-align" \
    "$ROOT/cache/asr-upload"
  nohup env \
    PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin" \
    HF_HOME="$ROOT/cache/huggingface" \
    HF_ENDPOINT="${HF_ENDPOINT:-https://hf-mirror.com}" \
    "$PYTHON" "$SERVICE" \
      --model "$MODEL" \
      --model-root "$ROOT/cache/whisperx" \
      --alignment-root "$ROOT/cache/whisperx-align" \
      --temporary-root "$ROOT/cache/asr-upload" \
      --token-file "$TOKEN_FILE" \
      --threads 16 \
      >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null &
  local pid=$!
  printf '%s\n' "$pid" >"$PID_FILE"
  for _ in $(seq 1 60); do
    if is_ready; then
      echo "ASR service is ready (pid $pid; model loads on first request)"
      return
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "ASR service exited before becoming ready" >&2
      tail -80 "$STDERR_LOG" >&2 || true
      exit 1
    fi
    sleep 1
  done
  echo "ASR service did not become ready within 60 seconds" >&2
  exit 1
}

stop() {
  local pid
  if ! pid=$(read_pid); then
    echo "ASR service is not running"
    return
  fi
  local process
  process=$(ps -p "$pid" -o command= 2>/dev/null || true)
  if [[ "$process" != *"asr-service.py"* ]]; then
    echo "Refusing to stop pid $pid because it is not the managed ASR service" >&2
    exit 1
  fi
  kill "$pid"
  for _ in $(seq 1 30); do
    kill -0 "$pid" 2>/dev/null || {
      echo "ASR service stopped"
      return
    }
    sleep 1
  done
  echo "ASR service did not stop within 30 seconds" >&2
  exit 1
}

case "${1:-status}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status)
    if is_ready; then echo "ready url=$URL"; else echo "not-ready url=$URL"; exit 1; fi
    ;;
  *) echo "usage: $0 {start|stop|restart|status}" >&2; exit 2 ;;
esac
