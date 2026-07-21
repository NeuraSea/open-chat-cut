#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
TOKEN_FILE="$ROOT/config/tts.token"

if [[ ! -x "$ROOT/runtime/qwen3-tts/.venv/bin/python" ]]; then
  echo "Run install-native-runtimes.sh before installing TTS" >&2
  exit 2
fi
mkdir -p "$ROOT/config"
if [[ ! -f "$TOKEN_FILE" ]]; then
  /usr/bin/python3 - "$TOKEN_FILE" <<'PY'
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
echo "TTS service credential is ready."
