#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
MODELSCOPE="$ROOT/runtime/qwen3-tts/.venv/bin/modelscope"
MODEL_ROOT="$ROOT/runtime/whisperx/models/faster-whisper-large-v3"

if [[ ! -x "$MODELSCOPE" ]]; then
  echo "ModelScope CLI is missing; run install-native-runtimes.sh first" >&2
  exit 2
fi
if [[ -f "$MODEL_ROOT/config.json" && -f "$MODEL_ROOT/model.bin" ]]; then
  echo "present: gpustack/faster-whisper-large-v3"
  exit 0
fi

mkdir -p "$MODEL_ROOT"
"$MODELSCOPE" download \
  --model gpustack/faster-whisper-large-v3 \
  --local_dir "$MODEL_ROOT"

test -f "$MODEL_ROOT/config.json"
test -f "$MODEL_ROOT/model.bin"
echo "Whisper large-v3 CTranslate2 model is present."
