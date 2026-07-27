#!/usr/bin/env bash
# Builds and deploys the Cloudflare stack: the Rust Worker and the React client.
#
# The ordering matters and is easy to get wrong by hand:
#
#   1. packages/ontime-wasm must be built before apps/client, because the client
#      resolves `ontime-timer-engine` to its pkg/ output through a vite alias and
#      pkg/ is a gitignored build artifact.
#   2. apps/client must be built with VITE_WORKER_URL pointing at the deployed
#      Worker. Without it the client defaults to window.location.origin, which on
#      Pages is the static host with no backend — every API call 404s and the
#      editor renders blank (see handover 006).
#
# Usage:
#   ./deploy-cloud.sh              # build and deploy both
#   ./deploy-cloud.sh --worker     # worker only
#   ./deploy-cloud.sh --client     # client only
#   ./deploy-cloud.sh --dry-run    # build everything, deploy nothing
#
# Troubleshooting: if `worker-build` fails with a wasm-bindgen "schema version"
# mismatch, worker-build's cached CLI got overwritten by a `cargo install
# wasm-bindgen-cli` of a different version (the ontime-wasm build pins its own).
# Fix: rm -rf ~/Library/Caches/worker-build/wasm-bindgen-* and rebuild — it
# re-fetches the version matching apps/server-rs's pin.
set -euo pipefail

WORKER_URL="${VITE_WORKER_URL:-https://ontime-server-rs.example.workers.dev}"
PAGES_PROJECT="${PAGES_PROJECT:-your-pages-project}"
PAGES_BRANCH="${PAGES_BRANCH:-master}"

root="$(cd "$(dirname "$0")" && pwd)"
do_worker=true
do_client=true
dry_run=false

for arg in "$@"; do
  case "$arg" in
    --worker) do_client=false ;;
    --client) do_worker=false ;;
    --dry-run) dry_run=true ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

step() { printf '\n\033[1m==> %s\033[0m\n' "$1"; }

if [ "$do_worker" = true ]; then
  step "Building the Worker"
  (cd "$root/apps/server-rs" && worker-build --release)

  if [ "$dry_run" = false ]; then
    step "Deploying the Worker"
    (cd "$root/apps/server-rs" && npx --yes wrangler@latest deploy)
  fi
fi

if [ "$do_client" = true ]; then
  step "Building the WASM timer engine"
  "$root/packages/ontime-wasm/build.sh"

  step "Building the client against $WORKER_URL"
  (cd "$root/apps/client" && VITE_WORKER_URL="$WORKER_URL" pnpm build)

  # guard against the two mistakes that produce a silently broken deploy
  step "Checking the build"
  # the URL can land in any chunk, so search them all rather than guessing one
  if ! grep -rqF "$WORKER_URL" "$root/apps/client/build/assets/"*.js; then
    echo "FAIL: no bundle references $WORKER_URL" >&2
    exit 1
  fi
  if grep -qE '127\.0\.0\.1:8787|localhost:8787' "$root/apps/client/build/assets/"*.js; then
    echo "FAIL: a localhost worker URL leaked into the bundle" >&2
    exit 1
  fi
  if ! ls "$root/apps/client/build/assets/"*.wasm >/dev/null 2>&1; then
    echo "FAIL: no wasm asset emitted — was packages/ontime-wasm built?" >&2
    exit 1
  fi
  echo "  worker url ok, no localhost leak, wasm asset present"

  if [ "$dry_run" = false ]; then
    step "Deploying the client to Pages"
    (cd "$root/apps/client" && npx --yes wrangler@latest pages deploy build \
      --project-name="$PAGES_PROJECT" --branch="$PAGES_BRANCH" --commit-dirty=true)
  fi
fi

step "Done"
