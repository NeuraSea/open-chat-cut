#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
PYTHON="$ROOT/runtime/qwen3-tts/.venv/bin/python"
SERVICE="$ROOT/runtime/tts-service.py"
MODEL="$ROOT/runtime/qwen3-tts/models/Qwen3-TTS-12Hz-1.7B-CustomVoice"
TOKEN_FILE="$ROOT/config/tts.token"
PID_FILE="$ROOT/runtime/tts-service.pid"
LOG_ROOT=${OPENCHATCUT_MODEL_FARM_LOG_ROOT:-$HOME/Library/Logs/OpenChatCut}
STDOUT_LOG="$LOG_ROOT/tts-service.log"
STDERR_LOG="$LOG_ROOT/tts-service.err.log"
URL=http://127.0.0.1:8192/health

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
  if is_ready; then echo "TTS service is already ready on 127.0.0.1:8192"; return; fi
  if [[ ! -x "$PYTHON" || ! -f "$SERVICE" || \
        ! -f "$MODEL/config.json" || \
        ! -f "$MODEL/model.safetensors" || \
        ! -f "$MODEL/speech_tokenizer/model.safetensors" || \
        ! -f "$TOKEN_FILE" ]]; then
    echo "TTS runtime, model, service, or token is missing" >&2
    exit 2
  fi
  mkdir -p "$LOG_ROOT"
  nohup env PYTORCH_ENABLE_MPS_FALLBACK=1 \
    "$PYTHON" "$SERVICE" \
      --model-path "$MODEL" \
      --token-file "$TOKEN_FILE" \
      --ffmpeg /opt/homebrew/bin/ffmpeg \
      >"$STDOUT_LOG" 2>"$STDERR_LOG" </dev/null &
  local pid=$!
  printf '%s\n' "$pid" >"$PID_FILE"
  for _ in $(seq 1 60); do
    if is_ready; then echo "TTS service is ready (pid $pid; model loads on first request)"; return; fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "TTS service exited before becoming ready" >&2
      tail -80 "$STDERR_LOG" >&2 || true
      exit 1
    fi
    sleep 1
  done
  echo "TTS service did not become ready within 60 seconds" >&2
  exit 1
}

stop() {
  local pid
  if ! pid=$(read_pid); then echo "TTS service is not running"; return; fi
  local process
  process=$(ps -p "$pid" -o command= 2>/dev/null || true)
  if [[ "$process" != *"tts-service.py"* ]]; then
    echo "Refusing to stop pid $pid because it is not the managed TTS service" >&2
    exit 1
  fi
  kill "$pid"
  for _ in $(seq 1 30); do
    kill -0 "$pid" 2>/dev/null || { echo "TTS service stopped"; return; }
    sleep 1
  done
  echo "TTS service did not stop within 30 seconds" >&2
  exit 1
}

case "${1:-status}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status) if is_ready; then echo "ready url=$URL"; else echo "not-ready url=$URL"; exit 1; fi ;;
  *) echo "usage: $0 {start|stop|restart|status}" >&2; exit 2 ;;
esac
