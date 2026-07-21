#!/usr/bin/env sh
set -eu

SOURCE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
VERSION=$(cat "$SOURCE/VERSION")
case "$VERSION" in *[!0-9A-Za-z._+-]*|'') echo "Release VERSION is invalid" >&2; exit 2 ;; esac
DATA_HOME=${XDG_DATA_HOME:-"$HOME/.local/share"}
INSTALL_ROOT=${OPENCHATCUT_INSTALL_ROOT:-"$DATA_HOME/openchatcut"}
DESTINATION="$INSTALL_ROOT/versions/$VERSION"
HOME_DIR=${OPENCHATCUT_HOME:-"$HOME/.openchatcut"}
WITH_ML=1
INSTALL_PLUGIN=1
START=1

for argument in "$@"; do
  case "$argument" in
    --without-ml) WITH_ML=0 ;;
    --no-plugin) INSTALL_PLUGIN=0 ;;
    --no-start) START=0 ;;
    *) echo "unknown option: $argument" >&2; exit 2 ;;
  esac
done
for command in python3 curl; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 1; }
done

has_system_chromium() {
  for candidate in \
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
    "/Applications/Chromium.app/Contents/MacOS/Chromium" \
    /usr/bin/google-chrome /usr/bin/google-chrome-stable \
    /usr/bin/chromium /usr/bin/chromium-browser; do
    [ -x "$candidate" ] && return 0
  done
  return 1
}

python3 "$SOURCE/scripts/release/verify-release-bundle.py" "$SOURCE"
mkdir -p "$INSTALL_ROOT/versions" "$HOME_DIR/runtime" "$HOME/.local/bin"
temporary="$INSTALL_ROOT/versions/.install-$VERSION-$$"
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
rm -rf "$temporary"
mkdir -p "$temporary"
cp -R "$SOURCE"/. "$temporary"/
rm -rf "$DESTINATION"
mv "$temporary" "$DESTINATION"
ln -sfn "$DESTINATION" "$INSTALL_ROOT/current"
ln -sfn "$INSTALL_ROOT/current/openchatcut" "$HOME/.local/bin/openchatcut"

"$DESTINATION/scripts/install-ffmpeg.sh"
VENV="$HOME_DIR/runtime/media-worker"
if [ ! -x "$VENV/bin/python" ]; then python3 -m venv "$VENV"; fi
"$VENV/bin/python" -m pip install --disable-pip-version-check --upgrade pip
if [ "$WITH_ML" -eq 1 ]; then
  extras='[transcription,diarization,denoise,render]'
else
  extras='[render]'
fi
"$VENV/bin/python" -m pip install --disable-pip-version-check "$DESTINATION/services/media-worker$extras"
if has_system_chromium; then
  echo "Using the installed Chrome/Chromium browser for headless rendering"
else
  "$VENV/bin/python" -m playwright install chromium
fi

if [ "$INSTALL_PLUGIN" -eq 1 ]; then
  if command -v codex >/dev/null 2>&1 && command -v node >/dev/null 2>&1; then
    "$DESTINATION/scripts/install-codex-plugin.sh"
  else
    echo "Codex plugin skipped; install Codex + Node, run 'codex login', then:" >&2
    echo "  $DESTINATION/scripts/install-codex-plugin.sh" >&2
  fi
fi
echo "Installed OpenChatCut $VERSION at $DESTINATION"
echo "Ensure $HOME/.local/bin is on PATH, then run: openchatcut open"
if [ "$START" -eq 1 ]; then "$DESTINATION/openchatcut" open; fi
