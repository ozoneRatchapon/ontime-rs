#!/usr/bin/env bash
# Mutation testing of the code that decides who gets in (plan 019, item 7).
#
#   scripts/mutants.sh            # the security files, about 20 minutes
#   scripts/mutants.sh <file>...  # just these, relative to packages/ontime-core
#
# cargo-mutants changes the code one small way at a time (a `<` for a `<=`, a
# function returning a constant) and reruns the tests. A change no test notices
# is a gap in the tests. Reports land in target/mutants/<n>/, numbered.
#
# It runs in place, because some ontime-core tests read files from outside the
# crate, which a copied tree would not have. So: do not build, test or deploy
# anything that uses ontime-core while it runs, and commit or stash first.
# Every mutation is undone when it finishes; `git diff` should then be empty.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
core="$root/packages/ontime-core"

if ! command -v cargo-mutants >/dev/null; then
  echo "cargo-mutants is not installed: cargo install cargo-mutants" >&2
  exit 1
fi
if [ -n "$(git -C "$root" status --porcelain -- packages/ontime-core)" ]; then
  echo "packages/ontime-core has uncommitted changes; commit or stash them first" >&2
  exit 1
fi

files=("$@")
if [ ${#files[@]} -eq 0 ]; then
  files=(src/guard.rs src/api/access.rs src/identity.rs src/directory.rs)
fi
args=()
for file in "${files[@]}"; do args+=(-f "$file"); done

reports="$root/target/mutants"
mkdir -p "$reports"
n=$(( $(find "$reports" -mindepth 1 -maxdepth 1 -type d | wc -l) + 1 ))
out="$reports/$(printf '%03d' "$n")"

cd "$core"
cargo mutants --in-place "${args[@]}" --timeout 120 -o "$out"
# the token helpers live in a large file, so only they are mutated there
cargo mutants --in-place -f src/project.rs --re 'token_matches|hash_password' --timeout 300 -o "$out-project"

git -C "$root" diff --quiet -- packages/ontime-core || {
  echo "ontime-core is not back to its committed state; check git diff" >&2
  exit 1
}
echo "reports in $out and $out-project"
