#!/usr/bin/env bash
# Runs every end-to-end suite against a running worker.
#
#   ./run.sh                          # against a local `wrangler dev` on :8787
#   ./run.sh https://…workers.dev     # against a deployment
#   ./run.sh --start                  # start wrangler dev, run, then stop it
#
# These are behavioural tests: they drive the real HTTP and websocket surface the
# client uses, and several of them have caught bugs that unit tests could not —
# wrong response shapes, rooms leaking into each other, the timer not ticking.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
server_dir="$(cd "$here/../.." && pwd)"
repo_root="$(cd "$server_dir/../.." && pwd)"

base="http://127.0.0.1:8787"
start_server=false

for arg in "$@"; do
  case "$arg" in
    --start) start_server=true ;;
    http*) base="$arg" ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

# shellcheck source=../ports.sh
. "$here/../ports.sh"

wrangler_pid=""
cleanup() {
  if [ -n "$wrangler_pid" ]; then
    kill "$wrangler_pid" 2>/dev/null || true
    stop_port 8787
  fi
}
trap cleanup EXIT

if [ "$start_server" = true ]; then
  require_free_port 8787
  echo "==> building the worker"
  (cd "$server_dir" && worker-build --release >/dev/null)

  echo "==> starting wrangler dev"
  (cd "$server_dir" && npx --yes wrangler@latest dev --port 8787 --local >/tmp/ontime-e2e-wrangler.log 2>&1) &
  wrangler_pid=$!

  printf '    waiting'
  for _ in $(seq 1 60); do
    if curl -s -m 2 "$base/api/health" >/dev/null 2>&1; then break; fi
    printf '.'
    sleep 1
  done
  echo
fi

if ! curl -s -m 5 "$base/api/health" >/dev/null 2>&1; then
  echo "no worker responding at $base — start one, or pass --start" >&2
  exit 1
fi

# engine.mjs loads the real wasm the browser loads
if [ ! -f "$repo_root/packages/ontime-wasm/pkg/ontime_timer_bg.wasm" ]; then
  echo "==> building the wasm engine (engine.mjs needs it)"
  "$repo_root/packages/ontime-wasm/build.sh" >/dev/null
fi

suites=(
  "rest.sh:bash"      # CRUD round trips through storage
  "duplicate.mjs:node" # a duplicated rundown is a fully independent copy
  "shapes.mjs:node"   # response shapes against the client's types
  "ws.mjs:node"       # websocket handshake and refetch broadcast
  "timer.mjs:node"    # playback over the websocket
  "engine.mjs:node"   # browser wasm engine agrees with the server
  "rooms.mjs:node"    # project isolation
  "auth.mjs:node"     # per-room password
  "operator.mjs:node" # aux timers, message overlay, report, client list
  "offset.mjs:node"   # ahead/behind schedule against a live rundown
  "roll.mjs:node"     # the rundown following the wall clock
  "report-archive.mjs:node" # runs preserved permanently
  "rooms-cost.mjs:node" # what an unwatched room costs, and who may create one
  "upstream.mjs:node" # behaviour distilled from upstream v4.15 (plan 003)
  "admin.mjs:node"    # room list, owners, live clients (needs ADMIN_PASSWORD)
  "people.mjs:node"   # people, invite links, room owners (needs ADMIN_PASSWORD)
  "members.mjs:node"  # operators, members-only rooms (needs ADMIN_PASSWORD, ~1 min)
  "activity.mjs:node" # the audit log, who may read it (needs ADMIN_PASSWORD)
  "teleprompter.mjs:node" # the shared teleprompter transport (plan 008)
)

echo
echo "running against $base"
echo

failures=()
for entry in "${suites[@]}"; do
  file="${entry%%:*}"
  runner="${entry##*:}"

  printf '\033[1m── %s\033[0m\n' "$file"
  if "$runner" "$here/$file" "$base"; then
    :
  else
    failures+=("$file")
  fi
  echo
done

if [ ${#failures[@]} -eq 0 ]; then
  printf '\033[32mall suites passed\033[0m\n'
  exit 0
fi

printf '\033[31mfailed: %s\033[0m\n' "${failures[*]}"
exit 1
