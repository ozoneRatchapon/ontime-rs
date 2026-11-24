# Sourced by the e2e and browser runners when they start their own worker.
#
# `wrangler dev` runs as npm -> node -> workerd. Killing the subshell that
# started it leaves workerd listening, and the next `--start` then cannot bind:
# its health check finds the old worker, and every suite tests the old build
# while reporting a pass. That happened on 2026-11-25. So a busy port is an
# error, and on exit everything listening on the port is stopped.

# Fails when something already listens on the port. The runner checked that
# nothing listened before it started, so whatever listens later is its own.
require_free_port() {
  local port="$1"
  local holders
  holders="$(lsof -nP -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)"
  if [ -n "$holders" ]; then
    echo "port $port is already in use (pid $(echo "$holders" | tr '\n' ' '))." >&2
    echo "A worker left from an earlier run would be tested instead of this build." >&2
    echo "Stop it first, e.g.: kill $(echo "$holders" | tr '\n' ' ')" >&2
    exit 1
  fi
}

# Stops whatever listens on the port, and the wrangler that started it.
stop_port() {
  local port="$1"
  pkill -f "wrangler@latest dev --port $port" 2>/dev/null || true
  local holders
  holders="$(lsof -nP -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)"
  if [ -n "$holders" ]; then
    # shellcheck disable=SC2086 # one pid per word
    kill $holders 2>/dev/null || true
  fi
}
