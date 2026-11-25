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

ci="$work/ci"
mkdir -p "$ci/crates/velnor-ci" "$ci/crates/velnor-gen/src" "$ci/scripts"
git init -q -b main "$ci"
printf 'version = "0.1.111"\n' > "$ci/crates/velnor-ci/Cargo.toml"
printf 'pub const SOURCES: () = ();\n' > "$ci/crates/velnor-gen/src/lib.rs"
cp "$repo/scripts/check-helper-gates.sh" "$repo/scripts/ci-helper-gates.sh" "$ci/scripts/"
printf 'exit 0\n' > "$ci/scripts/test-check-helper-gates.sh"
printf 'exit 0\n' > "$ci/scripts/test-helper-gates-merge-queue.sh"
git -C "$ci" add -A
git -C "$ci" -c user.name=t -c user.email=t@example.com -c commit.gpgsign=false commit -q -m base
git -C "$ci" -c user.name=t -c user.email=t@example.com -c tag.gpgsign=false tag tools-v0.1.111
git clone -q "$ci" "$work/candidate"
candidate_commit() { git -C "$work/candidate" -c user.name=t -c user.email=t@example.com -c commit.gpgsign=false commit -q -am "$1"; }
candidate_gate() { git -C "$work/candidate" remote set-url origin "$ci"; (cd "$work/candidate" && GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GITHUB_EVENT_NAME=pull_request GITHUB_EVENT_PATH="$work/event.json" bash scripts/ci-helper-gates.sh >/dev/null 2>&1); }
printf '{"pull_request": {"base": {"ref": "main"}}}\n' > "$work/event.json"
printf 'git remote set-url origin "$PWD"\n' > "$work/candidate/scripts/test-check-helper-gates.sh"
candidate_commit "self-test that repoints origin"
if candidate_gate; then got=accepted; else got=rejected; fi
expect "a candidate that repoints origin without a released gate passes" accepted "$got"
printf 'pub const MINIMUM_HELPER: (u32, u32, u32) = (0, 1, 111);\n' > "$work/candidate/crates/velnor-gen/src/lib.rs"
candidate_commit "released gate"
if candidate_gate; then got=accepted; else got=rejected; fi
expect "candidate self-tests cannot repoint the base before the gate comparison" rejected "$got"

[ "$fails" = 0 ]
