#!/usr/bin/env bash
set -euo pipefail

OPENCHATCUT_HOME=${OPENCHATCUT_HOME:-$HOME/.openchatcut}
PROVIDER_FILE=${OPENCHATCUT_PROVIDER_CONFIG:-$OPENCHATCUT_HOME/providers.json}
BASE_URL=${OPENCHATCUT_NEW_API_BASE_URL:-https://api.singularity-x.ai:9443/v1}
ACCOUNT=openchatcut
SERVICE=singularity-x-new-api-token
ENABLE_VIDEO=${OPENCHATCUT_ENABLE_NEW_API_VIDEO:-0}
ENABLE_TTS=${OPENCHATCUT_ENABLE_NEW_API_TTS:-1}
ENABLE_ASR=${OPENCHATCUT_ENABLE_NEW_API_ASR:-1}
ENABLE_IMAGE=${OPENCHATCUT_ENABLE_NEW_API_IMAGE:-1}

validate_toggle() {
  local setting=$1
  local value=$2
  if [[ "$value" != 0 && "$value" != 1 ]]; then
    echo "$setting must be 0 or 1" >&2
    exit 2
  fi
}
validate_toggle OPENCHATCUT_ENABLE_NEW_API_VIDEO "$ENABLE_VIDEO"
validate_toggle OPENCHATCUT_ENABLE_NEW_API_TTS "$ENABLE_TTS"
validate_toggle OPENCHATCUT_ENABLE_NEW_API_ASR "$ENABLE_ASR"
validate_toggle OPENCHATCUT_ENABLE_NEW_API_IMAGE "$ENABLE_IMAGE"

if ! security find-generic-password \
  -a "$ACCOUNT" \
  -s "$SERVICE" >/dev/null 2>&1; then
  echo "The New API token is missing from macOS Keychain." >&2
  echo "Create it interactively before configuring OpenChatCut:" >&2
  echo "security add-generic-password -U -a $ACCOUNT -s $SERVICE -w" >&2
  exit 2
fi

mkdir -p "$(dirname "$PROVIDER_FILE")"
python3 - \
  "$PROVIDER_FILE" "$BASE_URL" "$ACCOUNT" "$SERVICE" "$ENABLE_VIDEO" "$ENABLE_TTS" "$ENABLE_ASR" "$ENABLE_IMAGE" <<'PY'
import json
import os
from pathlib import Path
import stat
import sys
import tempfile

path = Path(sys.argv[1]).expanduser()
base_url, account, service, enable_video, enable_tts, enable_asr, enable_image = sys.argv[2:]
if path.is_symlink():
    raise SystemExit("Refusing to replace a symlinked provider configuration")
if path.exists():
    mode = path.lstat().st_mode
    if not stat.S_ISREG(mode):
        raise SystemExit("Provider configuration must be a regular file")
    config = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(config, dict):
        raise SystemExit("Provider configuration root must be an object")
else:
    config = {}

credential = {
    "account": account,
    "service": service,
}
config["openaiCompatible"] = {
    "baseUrl": base_url,
    "model": "occ-edit-planner",
    "apiKeyKeychain": credential,
}
if enable_video == "1":
    config["newApiVideo"] = {
        "baseUrl": base_url,
        "defaultModel": "occ-video",
        "apiKeyKeychain": credential,
    }
if enable_tts == "1":
    config["newApiVoice"] = {
        "baseUrl": base_url,
        "defaultModel": "occ-tts",
        "submitPath": "audio/speech",
        "apiKeyKeychain": credential,
    }
if enable_asr == "1":
    config["newApiAsr"] = {
        "baseUrl": base_url,
        "defaultModel": "occ-asr",
        "submitPath": "audio/transcriptions",
        "apiKeyKeychain": credential,
    }
if enable_image == "1":
    config["newApiImage"] = {
        "baseUrl": base_url,
        "defaultModel": "occ-image",
        "submitPath": "images/generations",
        "apiKeyKeychain": credential,
    }

path.parent.mkdir(parents=True, exist_ok=True)
fd, temporary = tempfile.mkstemp(prefix=".providers.", suffix=".json", dir=path.parent)
try:
    os.fchmod(fd, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as stream:
        json.dump(config, stream, indent=2, sort_keys=True)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)
finally:
    try:
        os.unlink(temporary)
    except FileNotFoundError:
        pass
PY

chmod 600 "$PROVIDER_FILE"
echo "Configured OpenChatCut New API Agent provider in $PROVIDER_FILE"
if [[ "$ENABLE_VIDEO" == 1 ]]; then
  echo "Enabled the asynchronous New API video adapter."
fi
if [[ "$ENABLE_TTS" == 1 ]]; then
  echo "Enabled the synchronous private New API voice adapter."
fi
if [[ "$ENABLE_ASR" == 1 ]]; then
  echo "Enabled the private New API WhisperX transcription adapter."
fi
if [[ "$ENABLE_IMAGE" == 1 ]]; then
  echo "Enabled the private New API Qwen Image adapter."
fi
