#!/usr/bin/env bash
# Regenerates the teleprompter contract fixtures (plan 008) by running
# upstream ontime's own TypeScript over our case lists.
#
#   scripts/teleprompter-oracle/generate.sh [upstream-ref]   # default upstream/master
#
# Upstream's sources are extracted with `git archive` into a temporary folder,
# bundled with esbuild (already in node_modules) and run with node. The
# fixtures land in packages/ontime-core/tests/fixtures/teleprompter_*.json.
set -euo pipefail

ref="${1:-upstream/master}"
root="$(git rev-parse --show-toplevel)"
here="$root/scripts/teleprompter-oracle"
fixtures="$root/packages/ontime-core/tests/fixtures"
esbuild="$(ls -d "$root"/node_modules/.pnpm/esbuild@*/node_modules/esbuild/bin/esbuild | sort -V | tail -1)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/upstream" "$work/out"
git -C "$root" archive "$ref" tsconfig.common.json packages/types packages/utils apps/server/src/services/teleprompter-service apps/server/src/api-integration/integration.teleprompter.ts \
  | tar -x -C "$work/upstream"
# only the teleprompter part of ontime-utils, so the bundle needs none of its npm dependencies
cat > "$work/ontime-utils.ts" <<SHIM
export * from '$work/upstream/packages/utils/src/teleprompter/teleprompterSettings.ts';
export * from '$work/upstream/packages/utils/src/teleprompter/teleprompterPosition.ts';
export * from '$work/upstream/packages/utils/src/teleprompter/teleprompterTransport.ts';
SHIM
commit="$(git -C "$root" rev-parse --short "$ref")"

for name in wrap script position transport; do
  "$esbuild" "$here/gen-$name.ts" --bundle --platform=node --format=esm --log-level=error \
    --alias:@upstream="$work/upstream" \
    --alias:ontime-types="$work/upstream/packages/types/src/index.ts" \
    --alias:ontime-utils="$work/ontime-utils.ts" \
    --outfile="$work/out/gen-$name.mjs"
  node "$work/out/gen-$name.mjs" > "$work/out/$name.json"
  COMMIT="$commit" NAME="$name" node -e '
    const fs = require("fs");
    const cases = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const count = Array.isArray(cases) ? cases.length : Object.values(cases).reduce((n, list) => n + list.length, 0);
    const comment = `Generated from upstream ontime ${process.env.COMMIT} by scripts/teleprompter-oracle/generate.sh; do not edit by hand.`;
    fs.writeFileSync(process.argv[2], JSON.stringify({ _comment: comment, cases }, null, 1) + "\n");
  ' "$work/out/$name.json" "$fixtures/teleprompter_$name.json"
  echo "teleprompter_$name.json written"
done
