#!/usr/bin/env bash
set -euo pipefail

LMS=${LMS_BIN:-$HOME/.lmstudio/bin/lms}
LOG_ROOT=${OPENCHATCUT_MODEL_FARM_LOG_ROOT:-$HOME/Library/Logs/OpenChatCut}
LOAD_TIMEOUT_SECONDS=${OPENCHATCUT_LMS_LOAD_TIMEOUT_SECONDS:-600}

if [[ ! -x "$LMS" ]]; then
  echo "LM Studio CLI is required at $LMS" >&2
  exit 2
fi
mkdir -p "$LOG_ROOT"

model_status() {
  local identifier=$1
  "$LMS" ps --json | /usr/bin/python3 -c '
import json, sys
identifier = sys.argv[1]
for model in json.load(sys.stdin):
    if model.get("identifier") == identifier:
        print(model.get("status", "unknown"))
        raise SystemExit(0)
raise SystemExit(1)
' "$identifier"
}

load_model() {
  local model=$1
  local identifier=$2
  local context=$3
  local status
  if status=$(model_status "$identifier" 2>/dev/null); then
    echo "present: $identifier status=$status"
    return
  fi

  local log="$LOG_ROOT/$identifier.load.log"
  : >"$log"
  "$LMS" load "$model" \
    --identifier "$identifier" \
    --context-length "$context" \
    --parallel 1 \
    --gpu max \
    --yes \
    >"$log" 2>&1 &
  local loader=$!
  local deadline=$((SECONDS + LOAD_TIMEOUT_SECONDS))
  while (( SECONDS < deadline )); do
    if status=$(model_status "$identifier" 2>/dev/null); then
      # LM Studio 0.4.7 can leave its CLI spinner alive after the engine has
      # reached idle. Stopping that CLI process does not unload the engine.
      kill "$loader" >/dev/null 2>&1 || true
      wait "$loader" 2>/dev/null || true
      echo "loaded: $identifier status=$status"
      return
    fi
    if ! kill -0 "$loader" 2>/dev/null; then
      wait "$loader" 2>/dev/null || true
      echo "LM Studio failed to load $identifier" >&2
      tail -80 "$log" >&2 || true
      exit 1
    fi
    sleep 2
  done
  kill "$loader" >/dev/null 2>&1 || true
  wait "$loader" 2>/dev/null || true
  echo "Timed out loading $identifier" >&2
  exit 1
}

load_model qwen/qwen3-coder-next occ-edit-planner 32768
load_model zai-org/glm-4.7-flash occ-edit-fast 32768
load_model text-embedding-nomic-embed-text-v1.5 occ-embedding 8192

echo "LM Studio hot planning and embedding models are loaded."
