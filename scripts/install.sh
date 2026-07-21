#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SETUP_ARGS=
INSTALL_PLUGIN=1
START=1
for argument in "$@"; do
  case "$argument" in
    --without-ml) SETUP_ARGS="$SETUP_ARGS --without-ml" ;;
    --skip-plugin) INSTALL_PLUGIN=0 ;;
    --no-start) START=0 ;;
    *) echo "unknown option: $argument" >&2; exit 2 ;;
  esac
done
# shellcheck disable=SC2086
"$ROOT/scripts/setup.sh" $SETUP_ARGS
if [ "$INSTALL_PLUGIN" -eq 1 ]; then
  if command -v codex >/dev/null 2>&1; then
    "$ROOT/scripts/install-codex-plugin.sh"
  else
    echo "Codex CLI is not installed; plugin installation was skipped." >&2
  fi
fi
if [ "$START" -eq 1 ]; then "$ROOT/scripts/openchatcut.sh" start; fi
echo "OpenChatCut source installation is ready."
