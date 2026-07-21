#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
UV=${UV_BIN:-$HOME/.local/bin/uv}
PYTHON311=${PYTHON311:-/opt/homebrew/opt/python@3.11/bin/python3.11}

WHISPERX_VERSION=3.8.6
DEEPFILTERNET_VERSION=0.5.6
QWEN_TTS_VERSION=0.1.1

if [[ ! -x "$UV" ]]; then
  echo "uv is required at $UV" >&2
  exit 2
fi
if [[ ! -x "$PYTHON311" ]]; then
  echo "Python 3.11 is required at $PYTHON311" >&2
  exit 2
fi

export UV_NO_PROGRESS=1
export UV_HTTP_TIMEOUT=${UV_HTTP_TIMEOUT:-300}
export UV_PYTHON_INSTALL_DIR="$ROOT/runtime/python"
mkdir -p "$ROOT/runtime"

install_package() {
  local name=$1
  local python=$2
  local requirement=$3
  local env="$ROOT/runtime/$name/.venv"

  mkdir -p "$ROOT/runtime/$name"
  if [[ ! -x "$env/bin/python" ]]; then
    "$UV" venv --python "$python" "$env"
  fi
  "$UV" pip install --python "$env/bin/python" "$requirement"
}

install_package whisperx "$PYTHON311" "whisperx==$WHISPERX_VERSION"
install_package deepfilternet "$PYTHON311" "deepfilternet==$DEEPFILTERNET_VERSION"

"$UV" python install 3.12
install_package qwen3-tts 3.12 "qwen-tts==$QWEN_TTS_VERSION"
"$UV" pip install \
  --python "$ROOT/runtime/qwen3-tts/.venv/bin/python" \
  "modelscope>=1.28,<2" \
  "huggingface-hub>=0.34,<1"

"$ROOT/runtime/whisperx/.venv/bin/whisperx" --help >/dev/null
"$ROOT/runtime/deepfilternet/.venv/bin/deepFilter" --version
"$ROOT/runtime/whisperx/.venv/bin/python" -c \
  'from importlib.metadata import version; print("whisperx={}".format(version("whisperx")))'
"$ROOT/runtime/deepfilternet/.venv/bin/python" -c \
  'from importlib.metadata import version; print("deepfilternet={}".format(version("deepfilternet")))'
"$ROOT/runtime/qwen3-tts/.venv/bin/python" - <<'PY'
from importlib.metadata import version
from qwen_tts import Qwen3TTSModel

assert Qwen3TTSModel is not None
print(f"qwen-tts={version('qwen-tts')}")
PY

echo "Pinned native runtime environments are ready."
