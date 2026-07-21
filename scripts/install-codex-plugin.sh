#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
MARKETPLACE=openchatcut-local
SELECTOR="open-chat-cut@$MARKETPLACE"

if ! command -v codex >/dev/null 2>&1; then
  echo "Codex CLI is required. Install Codex and run 'codex login' first." >&2
  exit 1
fi
if ! command -v node >/dev/null 2>&1; then
  echo "Node.js 20+ is required by the bundled dependency-free STDIO bridge." >&2
  exit 1
fi

node --test "$ROOT"/plugins/open-chat-cut/tests/*.test.mjs
node "$ROOT/plugins/open-chat-cut/mcp/check-runtime.mjs"

# Local marketplace installs are cached copies. Remove the previous copy first so
# an update cannot silently keep an old bridge or skill bundle.
codex plugin remove "$SELECTOR" --json >/dev/null 2>&1 || true
codex plugin marketplace remove "$MARKETPLACE" >/dev/null 2>&1 || true
codex plugin marketplace add "$ROOT" --json
codex plugin add "$SELECTOR" --json

echo
echo "OpenChatCut Codex plugin installed. Open a new Codex task before using it."
