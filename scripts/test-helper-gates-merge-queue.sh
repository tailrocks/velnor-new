#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
workflow="${1:-$repo/.github/workflows/ci.yml}"
work="$(mktemp -d "${TMPDIR:-/tmp}/helper-gates-queue.XXXXXX")"
trap 'rm -rf "$work"' EXIT

triggers_on_merge_group() {
  awk '/^"?on"?:/ {on = 1; next} on && /^[^ ]/ {exit} on && /^  merge_group:/ {found = 1} END {exit !found}' "$workflow"
}

required_condition() {
  grep -o -- '--job lint-helper-gates=[^ '"'"']*' "$workflow" | head -n1
}

fails=0
expect() {
  local name=$1 want=$2 got=$3
  if [ "$got" != "$want" ]; then
    echo "FAIL $name: expected $want, got $got"
    fails=$((fails + 1))
  else
    echo "ok   $name ($got)"
  fi
}

if triggers_on_merge_group; then got=runs; else got=skipped; fi
expect "the generated workflow runs on the merge_group event" runs "$got"

condition="$(required_condition)"
if [ -n "$condition" ]; then got=required; else got=missing; fi
expect "the Required verdict waits for the helper gates job" required "$got"
case "$condition" in
  *lint:*) got=plan-gated ;;
  *) got=always ;;
esac
expect "the helper gates job is not skipped by the plan" always "$got"

git_in() { git -C "$work/repo" -c user.name=t -c user.email=t@example.com -c commit.gpgsign=false -c tag.gpgsign=false "$@"; }
mkdir -p "$work/repo/crates/velnor-ci" "$work/repo/crates/velnor-gen/src"
git init -q "$work/repo"
printf 'version = "0.1.111"\n' > "$work/repo/crates/velnor-ci/Cargo.toml"
printf 'pub const SOURCES: () = ();\n' > "$work/repo/crates/velnor-gen/src/lib.rs"
git_in add -A
git_in commit -q -m base
git_in tag tools-v0.1.111
base=$(git_in rev-parse HEAD)
printf 'pub const MINIMUM_HELPER: (u32, u32, u32) = (0, 1, 112);\n' > "$work/repo/crates/velnor-gen/src/lib.rs"
git_in commit -q -am "queue entry with an unreleased gate"
printf 'pub const OTHER: () = ();\n' > "$work/repo/crates/velnor-gen/src/other.rs"
git_in add -A
git_in commit -q -m "later queue entry without a gate"

gate_check() { (cd "$work/repo" && bash "$repo/scripts/check-helper-gates.sh" "$base" >/dev/null 2>&1); }

if gate_check; then got=accepted; else got=rejected; fi
expect "an unreleased gate passes at queue time" accepted "$got"
git_in tag tools-v0.1.112 "$base"
if gate_check; then got=accepted; else got=rejected; fi
expect "a gate released between PR CI and the queue is rejected whichever entry added it" rejected "$got"

[ "$fails" = 0 ]
