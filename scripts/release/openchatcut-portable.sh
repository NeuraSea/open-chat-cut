#!/usr/bin/env sh
set -eu

APP_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
HOME_DIR=${OPENCHATCUT_HOME:-"$HOME/.openchatcut"}
WEB_PORT=${OPENCHATCUT_WEB_PORT:-3100}
DAEMON_PORT=3210
DAEMON_PID="$HOME_DIR/portable-daemon.pid"
WEB_PID="$HOME_DIR/portable-web.pid"
DAEMON_LOG="$HOME_DIR/openchatcutd.log"
WEB_LOG="$HOME_DIR/web.log"
DAEMON="$APP_ROOT/bin/openchatcutd"
JS_RUNTIME="$APP_ROOT/bin/js-runtime"
WORKER="$HOME_DIR/runtime/media-worker/bin/openchatcut-media-worker"
MG_RUNTIME="$APP_ROOT/runtime/mg-runtime/src/cli.mjs"

valid_port() {
  case "$1" in ''|*[!0-9]*) return 1 ;; esac
  [ "$1" -ge 1 ] && [ "$1" -le 65535 ]
}
valid_port "$WEB_PORT" || { echo "OPENCHATCUT_WEB_PORT is invalid" >&2; exit 2; }
[ "$WEB_PORT" -ne "$DAEMON_PORT" ] || { echo "Web and daemon ports must differ" >&2; exit 2; }

ready() { curl --noproxy '*' -fsS "$1" >/dev/null 2>&1; }
pid_matches() {
  pid_file=$1 pattern=$2
  [ -f "$pid_file" ] || return 1
  pid=$(cat "$pid_file" 2>/dev/null || true)
  case "$pid" in ''|*[!0-9]*) return 1 ;; esac
  kill -0 "$pid" 2>/dev/null || return 1
  command=$(ps -p "$pid" -o command= 2>/dev/null || true)
  case "$command" in *"$pattern"*) return 0 ;; *) return 1 ;; esac
}
wait_ready() {
  url=$1 label=$2 count=0
  until ready "$url"; do
    count=$((count + 1))
    [ "$count" -le 240 ] || {
      echo "$label did not become ready; inspect $HOME_DIR/${label}.log.err" >&2
      exit 1
    }
    sleep 0.25
  done
}
start() {
  mkdir -p "$HOME_DIR"
  [ -x "$DAEMON" ] && [ -x "$JS_RUNTIME" ] && [ -x "$WORKER" ] && [ -f "$MG_RUNTIME" ] || {
    echo "Portable runtime is incomplete. Run $APP_ROOT/install.sh first." >&2; exit 1;
  }
  if ready "http://127.0.0.1:$DAEMON_PORT/health"; then
    pid_matches "$DAEMON_PID" "openchatcutd" || {
      echo "Port $DAEMON_PORT is already served by another daemon; stop it before starting this portable installation." >&2
      exit 1
    }
  else
    OPENCHATCUT_HOME="$HOME_DIR" \
      OPENCHATCUT_BIND="127.0.0.1:$DAEMON_PORT" \
      OPENCHATCUT_EDITOR_URL="http://127.0.0.1:$WEB_PORT" \
      OPENCHATCUT_MEDIA_WORKER="$WORKER" \
      OPENCHATCUT_MG_RUNTIME_CLI="$MG_RUNTIME" \
      OPENCHATCUT_NODE_COMMAND="$JS_RUNTIME" \
      nohup "$DAEMON" >>"$DAEMON_LOG" 2>"$DAEMON_LOG.err" </dev/null &
    echo $! >"$DAEMON_PID"
  fi
  wait_ready "http://127.0.0.1:$DAEMON_PORT/health" daemon
  if ready "http://127.0.0.1:$WEB_PORT/api/health"; then
    pid_matches "$WEB_PID" "apps/web/server.js" || {
      echo "Port $WEB_PORT is already served by another Web process; choose OPENCHATCUT_WEB_PORT." >&2
      exit 1
    }
  else
    (cd "$APP_ROOT/web"
      PORT="$WEB_PORT" HOSTNAME=127.0.0.1 NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 \
        nohup "$JS_RUNTIME" apps/web/server.js >>"$WEB_LOG" 2>"$WEB_LOG.err" </dev/null &
      echo $! >"$WEB_PID")
  fi
  wait_ready "http://127.0.0.1:$WEB_PORT/api/health" web
  echo "OpenChatCut: http://127.0.0.1:$WEB_PORT/projects"
}
stop_one() {
  file=$1 pattern=$2
  if pid_matches "$file" "$pattern"; then
    target_pid=$(cat "$file")
    kill "$target_pid" 2>/dev/null || true
    attempts=0
    while kill -0 "$target_pid" 2>/dev/null; do
      attempts=$((attempts + 1))
      [ "$attempts" -le 40 ] || {
        echo "$pattern did not stop cleanly (pid $target_pid)" >&2
        return 1
      }
      sleep 0.25
    done
  fi
  rm -f "$file"
}
stop() {
  stop_one "$WEB_PID" "apps/web/server.js"
  stop_one "$DAEMON_PID" "openchatcutd"
  echo "OpenChatCut stopped"
}
open_editor() {
  url="http://127.0.0.1:$WEB_PORT/projects"
  if command -v open >/dev/null 2>&1; then open "$url"
  elif command -v xdg-open >/dev/null 2>&1; then xdg-open "$url"
  else echo "$url"; fi
}
case "${1:-start}" in
  start) start ;;
  stop) stop ;;
  restart) stop; start ;;
  status)
    ready "http://127.0.0.1:$DAEMON_PORT/health" && echo "daemon: ready" || echo "daemon: offline"
    ready "http://127.0.0.1:$WEB_PORT/api/health" && echo "web: ready" || echo "web: offline"
    ;;
  open) start; open_editor ;;
  logs) tail -f "$DAEMON_LOG" "$WEB_LOG" ;;
  *) echo "usage: $0 {start|stop|restart|status|open|logs}" >&2; exit 2 ;;
esac
