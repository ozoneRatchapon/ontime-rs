#!/usr/bin/env bash
# The client in a real browser against the real Rust Worker.
#
#   ./run.sh           # against a worker on :8788 and the client on :4199 already running
#   ./run.sh --start   # build the client, start both, run, then stop them
#
# The worker gets a master password (ADMIN_PASSWORD, default admin-e2e), so the
# admin, people and members-only paths are covered too. Needs the wasm engine
# built (packages/ontime-wasm/build.sh), which the client build includes.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../../.." && pwd)"
export ADMIN_PASSWORD="${ADMIN_PASSWORD:-admin-e2e}"
export BROWSER_API="${BROWSER_API:-http://127.0.0.1:8788}"
export BROWSER_APP="${BROWSER_APP:-http://localhost:4199}"

# shellcheck source=../ports.sh
. "$here/../ports.sh"

pids=()
started=false
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  if [ "$started" = true ]; then
    stop_port 8788
    stop_port 4199
  fi
}
trap cleanup EXIT

# any HTTP answer means it is up; a new room on a worker with a password answers 403
wait_for() {
  for _ in $(seq 1 180); do
    [ "$(curl -s -o /dev/null -w '%{http_code}' "$1")" != "000" ] && return 0
    sleep 1
  done
  echo "timed out waiting for $1" >&2
  exit 1
}

if [ "${1:-}" = "--start" ]; then
  require_free_port 8788
  require_free_port 4199
  started=true
  # a fresh, empty state every run, as on CI: rooms left by earlier runs would
  # hide whatever only happens on a new deployment
  state="$(mktemp -d)"
  echo "==> starting the worker (state in $state)"
  (cd "$root/apps/server-rs" && npx --yes wrangler@latest dev --port 8788 --local \
    --persist-to "$state" --var "SESSION_PASSWORD:$ADMIN_PASSWORD" >/tmp/ontime-browser-wrangler.log 2>&1) &
  pids+=($!)
  echo "==> building the client against $BROWSER_API"
  (cd "$root/apps/client" && VITE_WORKER_URL="$BROWSER_API" pnpm build >/tmp/ontime-browser-build.log 2>&1)
  (cd "$root/apps/client" && npx vite preview --port 4199 --strictPort >/tmp/ontime-browser-preview.log 2>&1) &
  pids+=($!)
  wait_for "$BROWSER_API/r/browser-probe/data/session/info"
  wait_for "$BROWSER_APP/"
fi

failures=()
for check in people members activity teleprompter offline custom_view; do
  printf '\033[1m── %s\033[0m\n' "$check"
  if node "$here/$check.mjs"; then :; else failures+=("$check"); fi
  echo
done

if [ ${#failures[@]} -eq 0 ]; then
  printf '\033[32mall browser checks passed\033[0m\n'
  exit 0
fi
printf '\033[31mfailed: %s\033[0m\n' "${failures[*]}"
exit 1
