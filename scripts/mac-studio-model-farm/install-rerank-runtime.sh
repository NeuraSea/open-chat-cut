#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
UV=${UV_BIN:-$HOME/.local/bin/uv}
PYTHON311=${PYTHON311:-/opt/homebrew/opt/python@3.11/bin/python3.11}
MODEL_ID=${OPENCHATCUT_RERANK_MODEL_ID:-BAAI/bge-reranker-v2-m3}
SCRIPT_ROOT=$(cd "$(dirname "$0")" && pwd)
RUNTIME="$ROOT/runtime/rerank"
ENVIRONMENT="$RUNTIME/.venv"
MODEL_ROOT="$ROOT/cache/huggingface/hub"
TOKEN_FILE="$ROOT/config/rerank.token"

if [[ ! -x "$UV" ]]; then
  echo "uv is required at $UV" >&2
  exit 2
fi
if [[ ! -x "$PYTHON311" ]]; then
  echo "Python 3.11 is required at $PYTHON311" >&2
  exit 2
fi

mkdir -p "$RUNTIME" "$MODEL_ROOT" "$(dirname "$TOKEN_FILE")"
if [[ ! -x "$ENVIRONMENT/bin/python" ]]; then
  "$UV" venv --python "$PYTHON311" "$ENVIRONMENT"
fi
export UV_NO_PROGRESS=1
export UV_HTTP_TIMEOUT=${UV_HTTP_TIMEOUT:-300}
"$UV" pip install --python "$ENVIRONMENT/bin/python" \
  "sentence-transformers==5.1.2" \
  "huggingface-hub>=0.34,<1" \
  "modelscope>=1.28,<2"

install -m 0755 "$SCRIPT_ROOT/rerank-service.py" "$RUNTIME/rerank-service.py"
if [[ ! -s "$TOKEN_FILE" ]]; then
  "$ENVIRONMENT/bin/python" - "$TOKEN_FILE" <<'PY'
import os
from pathlib import Path
import secrets
import sys

path = Path(sys.argv[1])
descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
    stream.write(secrets.token_urlsafe(48) + "\n")
PY
fi
chmod 600 "$TOKEN_FILE"

MODELSCOPE_CACHE="$ROOT/cache/modelscope" \
  "$ENVIRONMENT/bin/python" - "$MODEL_ID" "$RUNTIME/model" <<'PY'
from modelscope import snapshot_download
from pathlib import Path
import sys

model_id, target = sys.argv[1:]
path = snapshot_download(
    model_id,
    local_dir=target,
)
assert Path(path).is_dir()
print("rerank model snapshot is ready")
PY

HF_HUB_OFFLINE=1 "$ENVIRONMENT/bin/python" - "$RUNTIME/model" <<'PY'
from sentence_transformers import CrossEncoder
import sys

model = CrossEncoder(
    sys.argv[1],
    device="cpu",
    trust_remote_code=True,
    local_files_only=True,
    max_length=128,
)
scores = model.predict(
    [("video editing", "offline editor"), ("video editing", "weather report")],
    show_progress_bar=False,
)
assert len(scores) == 2
print("rerank runtime import and inference check passed")
PY

echo "Pinned rerank runtime is ready."
