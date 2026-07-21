#!/usr/bin/env bash
set -euo pipefail

ROOT=${OPENCHATCUT_MODEL_FARM_ROOT:-/Volumes/External/openchatcut-models}
TOKEN_FILE="$ROOT/config/asr.token"

if [[ ! -x "$ROOT/runtime/whisperx/.venv/bin/whisperx" ]]; then
  echo "Run install-native-runtimes.sh before installing ASR" >&2
  exit 2
fi

mkdir -p "$ROOT/config" "$ROOT/cache/whisperx" "$ROOT/cache/whisperx-align"
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

echo "ASR service credential is ready. Model weights load on the first request."
