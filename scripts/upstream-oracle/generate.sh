#!/usr/bin/env bash
# Regenerates the contract fixtures shared by ontime-core and the browser, by
# running upstream ontime's own TypeScript over our case lists:
# - teleprompter_*.json (plan 008)
# - rundown_process.json: times, gaps, delays and days of a rundown (RIIR 1)
# - runtime_offset.json: the offset and the expected flag, group and rundown
#   end, after a script of operator actions (RIIR 3)
#
#   scripts/upstream-oracle/generate.sh [upstream-ref]   # default upstream/master
#
# Upstream's sources are extracted with `git archive` into a temporary folder,
# bundled with esbuild (already in node_modules) and run with node. The
# fixtures land in packages/ontime-core/tests/fixtures/.
set -euo pipefail

ref="${1:-upstream/master}"
root="$(git rev-parse --show-toplevel)"
here="$root/scripts/upstream-oracle"
fixtures="$root/packages/ontime-core/tests/fixtures"
esbuild="$(ls -d "$root"/node_modules/.pnpm/esbuild@*/node_modules/esbuild/bin/esbuild | sort -V | tail -1)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/upstream" "$work/out"
git -C "$root" archive "$ref" tsconfig.common.json packages/types packages/utils apps/server/src \
  | tar -x -C "$work/upstream"
# a file upstream generates at install time, which git does not carry
echo "export const ONTIME_VERSION = '0.0.0-oracle';" > "$work/upstream/apps/server/src/ONTIME_VERSION.js"
# npm packages the bundles need are taken from this repository's install
export NODE_PATH="$root/node_modules/.pnpm/node_modules"
commit="$(git -C "$root" rev-parse --short "$ref")"

for name in teleprompter_wrap teleprompter_script teleprompter_position teleprompter_transport rundown_process runtime_offset; do
  "$esbuild" "$here/gen-$name.ts" --bundle --platform=node --format=esm --log-level=error \
    --alias:@upstream="$work/upstream" \
    --alias:ontime-types="$work/upstream/packages/types/src/index.ts" \
    --alias:ontime-utils="$work/upstream/packages/utils/index.ts" \
    --outfile="$work/out/gen-$name.mjs"
  # UTC, so the runtime cases can read a time of day as epoch mod 24h
  TZ=UTC node "$work/out/gen-$name.mjs" > "$work/out/$name.json"
  COMMIT="$commit" NAME="$name" node -e '
    const fs = require("fs");
    const cases = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const count = Array.isArray(cases) ? cases.length : Object.values(cases).reduce((n, list) => n + list.length, 0);
    const comment = `Generated from upstream ontime ${process.env.COMMIT} by scripts/upstream-oracle/generate.sh; do not edit by hand.`;
    fs.writeFileSync(process.argv[2], JSON.stringify({ _comment: comment, cases }, null, 1) + "\n");
  ' "$work/out/$name.json" "$fixtures/$name.json"
  echo "$name.json written"
done
