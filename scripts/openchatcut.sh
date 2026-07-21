#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
HOME_DIR=${OPENCHATCUT_HOME:-"$HOME/.openchatcut"}
PID_FILE="$HOME_DIR/launcher.pid"
LOG_FILE="$HOME_DIR/openchatcutd.log"
DAEMON_BUILD_BIN=${OPENCHATCUT_DAEMON_BUILD_BIN:-"$ROOT/target/release/openchatcutd"}
DAEMON_BIN="$HOME_DIR/bin/openchatcutd"
MEDIA_WORKER=${OPENCHATCUT_MEDIA_WORKER:-"$HOME_DIR/runtime/media-worker/bin/openchatcut-media-worker"}
MG_RUNTIME_CLI=${OPENCHATCUT_MG_RUNTIME_CLI:-"$HOME_DIR/runtime/mg-runtime/src/cli.mjs"}
DAEMON_LABEL=io.openchatcut.daemon
VIDEO_ACCELERATION=${OPENCHATCUT_VIDEO_ACCELERATION:-auto}
WEB_PORT=${OPENCHATCUT_WEB_PORT:-3100}

case "$VIDEO_ACCELERATION" in
  auto|cpu|apple|nvidia) ;;
  *) echo "OPENCHATCUT_VIDEO_ACCELERATION must be auto, cpu, apple, or nvidia" >&2; exit 2 ;;
esac

case "$WEB_PORT" in
  ''|*[!0-9]*) echo "OPENCHATCUT_WEB_PORT must be an integer from 1 to 65535" >&2; exit 2 ;;
esac
if [ "$WEB_PORT" -lt 1 ] || [ "$WEB_PORT" -gt 65535 ] || [ "$WEB_PORT" -eq 3210 ]; then
  echo "OPENCHATCUT_WEB_PORT must be from 1 to 65535 and cannot be the daemon port 3210" >&2
  exit 2
fi

is_daemon_ready() {
  command -v curl >/dev/null 2>&1 && curl --noproxy '*' -fsS http://127.0.0.1:3210/health >/dev/null 2>&1
}

install_daemon_binary() {
  if [ ! -x "$DAEMON_BUILD_BIN" ]; then
    if [ -x "$DAEMON_BIN" ]; then
      return
    fi
    echo "Daemon is not built. Run ./scripts/setup.sh first." >&2
    exit 1
  fi
  if [ -x "$DAEMON_BIN" ] && cmp -s "$DAEMON_BUILD_BIN" "$DAEMON_BIN"; then
    return
  fi
  bin_dir=$(dirname "$DAEMON_BIN")
  mkdir -p "$bin_dir"
  temporary_bin="$bin_dir/.openchatcutd.$$"
  cp "$DAEMON_BUILD_BIN" "$temporary_bin"
  chmod 755 "$temporary_bin"
  mv -f "$temporary_bin" "$DAEMON_BIN"
}

start_daemon() {
  if [ "$(uname -s)" = Darwin ] && command -v launchctl >/dev/null 2>&1; then
    launchctl remove "$DAEMON_LABEL" >/dev/null 2>&1 || true
    launchctl submit \
      -l "$DAEMON_LABEL" \
      -o "$LOG_FILE" \
      -e "$LOG_FILE.err" \
      -- /usr/bin/env \
      "PATH=$PATH" \
      "OPENCHATCUT_HOME=$HOME_DIR" \
      "OPENCHATCUT_MEDIA_WORKER=$MEDIA_WORKER" \
      "OPENCHATCUT_MG_RUNTIME_CLI=$MG_RUNTIME_CLI" \
      "OPENCHATCUT_VIDEO_ACCELERATION=$VIDEO_ACCELERATION" \
      "OPENCHATCUT_EDITOR_URL=http://127.0.0.1:$WEB_PORT" \
      "$DAEMON_BIN"
    return
  fi

  OPENCHATCUT_HOME="$HOME_DIR" \
    OPENCHATCUT_MEDIA_WORKER="$MEDIA_WORKER" \
    OPENCHATCUT_MG_RUNTIME_CLI="$MG_RUNTIME_CLI" \
    OPENCHATCUT_VIDEO_ACCELERATION="$VIDEO_ACCELERATION" \
    OPENCHATCUT_EDITOR_URL="http://127.0.0.1:$WEB_PORT" \
    nohup "$DAEMON_BIN" >>"$LOG_FILE" 2>&1 &
  echo $! >"$PID_FILE"
}

start() {
  mkdir -p "$HOME_DIR"
  if [ ! -x "$MEDIA_WORKER" ] || [ ! -f "$MG_RUNTIME_CLI" ]; then
    echo "OpenChatCut host runtime is not installed. Run ./scripts/setup.sh first." >&2
    exit 1
  fi
  # Always atomically refresh the installed binary before checking readiness.
  # A previous daemon may be in the middle of failing its startup probe (for
  # example after a provider-schema upgrade), so gating installation on the
  # health endpoint can otherwise relaunch stale code indefinitely.
  install_daemon_binary
  if ! is_daemon_ready; then
    start_daemon
  fi

  attempt=0
  until is_daemon_ready; do
    attempt=$((attempt + 1))
    # Capability probing can be cold on CPU/ML workers. The installed daemon
    # lives outside Documents to avoid macOS delaying first execution of a
    # freshly linked repository build.
    if [ "$attempt" -gt 240 ]; then
      echo "Daemon did not become ready; see $LOG_FILE" >&2
      exit 1
    fi
    sleep 0.25
  done

  (cd "$ROOT" && OPENCHATCUT_WEB_PORT="$WEB_PORT" docker compose up -d web)
  echo "OpenChatCut daemon: http://127.0.0.1:3210"
  echo "OpenChatCut editor: http://127.0.0.1:$WEB_PORT/projects"
}

stop() {
  (cd "$ROOT" && docker compose stop web >/dev/null 2>&1) || true
  if [ "$(uname -s)" = Darwin ] && command -v launchctl >/dev/null 2>&1; then
    launchctl remove "$DAEMON_LABEL" >/dev/null 2>&1 || true
  fi
  if [ -f "$PID_FILE" ]; then
    pid=$(cat "$PID_FILE")
    kill "$pid" >/dev/null 2>&1 || true
    rm -f "$PID_FILE"
  fi
  # launchctl removal is asynchronous. Waiting here prevents `restart` from
  # observing the retiring process as healthy and skipping installation/start
  # of the newly built daemon.
  attempt=0
  offline_samples=0
  while [ "$offline_samples" -lt 4 ]; do
    attempt=$((attempt + 1))
    if is_daemon_ready; then
      # launchctl removal can briefly make the listener disappear before a
      # retiring job is observable again. Require one full second of stable
      # offline probes before installing and starting a replacement binary.
      offline_samples=0
    else
      offline_samples=$((offline_samples + 1))
    fi
    if [ "$attempt" -gt 80 ]; then
      echo "Daemon did not stop cleanly; see $LOG_FILE" >&2
      exit 1
    fi
    sleep 0.25
  done
  echo "OpenChatCut stopped"
}

status() {
  if is_daemon_ready; then
    echo "daemon: ready"
  else
    echo "daemon: offline"
  fi
  (cd "$ROOT" && docker compose ps web) || true
}

case "${1:-start}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status) status ;;
  logs) tail -f "$LOG_FILE" ;;
  *) echo "Usage: $0 {start|stop|restart|status|logs}" >&2; exit 2 ;;
esac
