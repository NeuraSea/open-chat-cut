#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
VENV="$ROOT/runtime/qwen3-tts/.venv"
MODEL_ROOT="$ROOT/runtime/qwen3-tts/models"

if [[ ! -x "$VENV/bin/modelscope" ]]; then
  echo "Run install-native-runtimes.sh before downloading Qwen3-TTS models" >&2
  exit 2
fi

mkdir -p "$MODEL_ROOT"

download() {
  local model=$1
  local directory=$2
  local destination="$MODEL_ROOT/$directory"
  if [[ -f "$destination/config.json" && \
        -f "$destination/model.safetensors" && \
        -f "$destination/speech_tokenizer/model.safetensors" ]]; then
    echo "present: $model"
    return
  fi
  "$VENV/bin/modelscope" download \
    --model "$model" \
    --local_dir "$destination"
}

# CustomVoice covers editable multilingual narration, Base supports reference-
# audio voice cloning, and VoiceDesign creates a voice from a text brief.
download Qwen/Qwen3-TTS-12Hz-1.7B-CustomVoice Qwen3-TTS-12Hz-1.7B-CustomVoice
download Qwen/Qwen3-TTS-12Hz-1.7B-Base Qwen3-TTS-12Hz-1.7B-Base
download Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign Qwen3-TTS-12Hz-1.7B-VoiceDesign

echo "Qwen3-TTS model set is present."
