#!/usr/bin/env bash
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
VERSION=${1:?usage: package-unix.sh VERSION [TARGET] [OUTPUT_DIR]}
TARGET=${2:-$(rustc -vV | awk '/host:/ {print $2}')}
OUTPUT=${3:-$ROOT/dist}
NAME="openchatcut-${VERSION}-${TARGET}"
STAGE="$OUTPUT/$NAME"
ARCHIVE="$OUTPUT/$NAME.tar.gz"

case "$VERSION" in *[!0-9A-Za-z.+-]*|'') echo "invalid version" >&2; exit 2 ;; esac
for path in \
  "$ROOT/target/release/openchatcutd" \
  "$ROOT/apps/web/.next/standalone/apps/web/server.js" \
  "$ROOT/apps/web/.next/static" \
  "$ROOT/apps/web/public" \
  "$ROOT/packages/mg-runtime/node_modules/@babel/parser"; do
  [[ -e "$path" ]] || { echo "missing release input: $path" >&2; exit 1; }
done
JS_RUNTIME=${OPENCHATCUT_JS_RUNTIME:-}
if [[ -z "$JS_RUNTIME" ]]; then
  JS_RUNTIME=$(command -v bun || command -v node || true)
fi
[[ -n "$JS_RUNTIME" && -x "$JS_RUNTIME" ]] || {
  echo "missing JavaScript runtime: install Bun/Node or set OPENCHATCUT_JS_RUNTIME" >&2
  exit 1
}
rm -rf "$STAGE" "$ARCHIVE"
mkdir -p "$STAGE/bin" "$STAGE/web/apps/web/.next" "$STAGE/runtime" "$STAGE/scripts/release"
install -m 0755 "$ROOT/target/release/openchatcutd" "$STAGE/bin/openchatcutd"
install -m 0755 "$JS_RUNTIME" "$STAGE/bin/js-runtime"
"$STAGE/bin/js-runtime" --version >/dev/null 2>&1 || {
  echo "JavaScript runtime is not relocatable after copying: $JS_RUNTIME" >&2
  echo "Use a standalone Bun binary for portable release artifacts." >&2
  exit 1
}
cp -RL "$ROOT/apps/web/.next/standalone"/. "$STAGE/web"/
cp -RL "$ROOT/apps/web/.next/static" "$STAGE/web/apps/web/.next/static"
cp -RL "$ROOT/apps/web/public" "$STAGE/web/apps/web/public"
cp -RL "$ROOT/packages/mg-runtime" "$STAGE/runtime/mg-runtime"
cp -RL "$ROOT/services" "$STAGE/services"
find "$STAGE/services" -type d \( -name __pycache__ -o -name .pytest_cache \) -prune -exec rm -rf {} +
find "$STAGE/services" -type f \( -name '*.pyc' -o -name '*.pyo' \) -delete
cp -RL "$ROOT/plugins" "$STAGE/plugins"
rm -rf \
  "$STAGE/services/media-worker/tests" \
  "$STAGE/plugins/open-chat-cut/tests" \
  "$STAGE/runtime/mg-runtime/test"
mkdir -p "$STAGE/.agents/plugins"
cp "$ROOT/.agents/plugins/marketplace.json" "$STAGE/.agents/plugins/marketplace.json"
cp "$ROOT/scripts/install-ffmpeg.sh" "$ROOT/scripts/install-codex-plugin.sh" "$STAGE/scripts/"
cp "$ROOT/scripts/release/build-manifest.py" "$ROOT/scripts/release/verify-release-bundle.py" "$ROOT/scripts/release/openchatcut-installed.ps1" "$STAGE/scripts/release/"
cp "$ROOT/LICENSE" "$ROOT/NOTICE.md" "$STAGE/"
cp -RL "$ROOT/LICENSES" "$STAGE/LICENSES"
printf '%s\n' "$VERSION" >"$STAGE/VERSION"
cp "$ROOT/scripts/release/openchatcut-portable.sh" "$STAGE/openchatcut"
cp "$ROOT/scripts/release/install-release.sh" "$STAGE/install.sh"
chmod 0755 "$STAGE/openchatcut" "$STAGE/install.sh" "$STAGE/scripts"/*.sh "$STAGE/scripts/release"/*.py
python3 "$ROOT/scripts/release/build-manifest.py" "$STAGE" --version "$VERSION" --target "$TARGET"
python3 "$ROOT/scripts/release/verify-release-bundle.py" "$STAGE"
COPYFILE_DISABLE=1 tar -C "$OUTPUT" -czf "$ARCHIVE" "$NAME"
python3 "$ROOT/scripts/release/verify-release-bundle.py" "$ARCHIVE"
if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$ARCHIVE" >"$ARCHIVE.sha256"
else sha256sum "$ARCHIVE" >"$ARCHIVE.sha256"; fi
echo "$ARCHIVE"
