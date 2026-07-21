#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
LMS=${LMS_BIN:-$HOME/.lmstudio/bin/lms}

task_state() {
  local task=$1
  local pid_file="$ROOT/runtime/$task.pid"
  local pid=none
  local state=stopped
  if [[ -f "$pid_file" ]]; then
    pid=$(cat "$pid_file" 2>/dev/null || echo invalid)
    if [[ "$pid" =~ ^[0-9]+$ ]] && kill -0 "$pid" 2>/dev/null; then
      state=running
    fi
  fi
  printf '%s=%s pid=%s\n' "$task" "$state" "$pid"
}

echo "root=$ROOT"
df -h "$ROOT" | awk 'NR == 2 { print "disk_used=" $5 " disk_free=" $4 }'

if [[ -L "$HOME/.lmstudio/models" ]]; then
  echo "lm_studio_models=$(readlink "$HOME/.lmstudio/models")"
else
  echo "lm_studio_models=not-linked"
fi

if [[ -x "$LMS" ]]; then
  "$LMS" ps --json | /usr/bin/python3 -c '
import json, sys
for model in json.load(sys.stdin):
    print("lm_model={} status={} context={}".format(
        model.get("identifier"), model.get("status"), model.get("contextLength")
    ))
'
fi

if curl --fail --silent --max-time 3 \
  http://127.0.0.1:8188/system_stats >/dev/null; then
  echo "comfyui=ready"
else
  echo "comfyui=not-ready"
fi

if curl --fail --silent --max-time 3 \
  http://127.0.0.1:8190/health >/dev/null; then
  echo "rerank=ready"
else
  echo "rerank=not-ready"
fi

if curl --fail --silent --max-time 3 \
  http://127.0.0.1:8191/health >/dev/null; then
  echo "asr=ready"
else
  echo "asr=not-ready"
fi

if curl --fail --silent --max-time 3 \
  http://127.0.0.1:8192/health >/dev/null; then
  echo "tts=ready"
else
  echo "tts=not-ready"
fi

if curl --fail --silent --max-time 3 \
  http://127.0.0.1:8193/health >/dev/null; then
  echo "image=ready"
else
  echo "image=not-ready"
fi

task_state install-native-runtimes
task_state download-comfy-models
task_state download-qwen-tts-models

complete=$(find "$ROOT/runtime/comfyui/models" -type f -name '*.safetensors' 2>/dev/null | wc -l | tr -d ' ')
partial=$(find "$ROOT/runtime/comfyui/models" -type f -name '*.part' 2>/dev/null | wc -l | tr -d ' ')
printf 'comfy_weights_complete=%s partial=%s\n' "$complete" "$partial"

if [[ -x "$ROOT/runtime/whisperx/.venv/bin/whisperx" ]]; then
  echo "whisperx=ready"
else
  echo "whisperx=missing"
fi

if [[ -x "$ROOT/runtime/deepfilternet/.venv/bin/deepFilter" ]]; then
  echo "deepfilternet=ready"
else
  echo "deepfilternet=missing"
fi

if [[ -x "$ROOT/runtime/qwen3-tts/.venv/bin/python" ]] && \
  "$ROOT/runtime/qwen3-tts/.venv/bin/python" \
    -c 'import qwen_tts' >/dev/null 2>&1; then
  echo "qwen3-tts=ready"
else
  echo "qwen3-tts=missing"
fi
