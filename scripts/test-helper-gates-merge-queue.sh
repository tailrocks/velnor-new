#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
workflow="${1:-$repo/.github/workflows/repository-checks.yml}"
work="$(mktemp -d "${TMPDIR:-/tmp}/helper-gates-queue.XXXXXX")"
trap 'rm -rf "$work"' EXIT

job_if() {
  awk '/^  helper-gates:/ {on = 1; next} on && /^  [a-z-]+:/ {exit} on && /^    if: / {sub(/^    if: /, ""); print; exit}' "$workflow"
}

runs_on() {
  local event=$1 head_repo=$2 expr
  expr=$(job_if)
  [ -n "$expr" ] || { echo "helper-gates has no if condition in $workflow" >&2; return 2; }
  python3 -I - "$expr" "$event" "$head_repo" <<'PY'
import sys
expr, event, head_repo = sys.argv[1:4]
for old, new in (
    ("github.event.pull_request.head.repo.full_name", "head_repo"),
    ("github.event_name", "event"),
    ("github.repository", "repository"),
    ("&&", " and "),
    ("||", " or "),
):
    expr = expr.replace(old, new)
env = {"event": event, "head_repo": head_repo, "repository": "o/r"}
sys.exit(0 if eval(expr, {"__builtins__": {}}, env) else 1)
PY
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

if runs_on merge_group ""; then got=runs; else got=skipped; fi
expect "helper-gates runs in the merge queue" runs "$got"
if runs_on pull_request o/r; then got=runs; else got=skipped; fi
expect "helper-gates runs on a same-repository pull request" runs "$got"
if runs_on pull_request fork/r; then got=runs; else got=skipped; fi
expect "helper-gates skips a fork pull request" skipped "$got"

git_in() { git -C "$work/repo" -c user.name=t -c user.email=t@example.com "$@"; }
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

gate_check() { (cd "$work/repo" && bash "$repo/scripts/check-helper-gates.sh" "$base" >/dev/null 2>&1); }

if gate_check; then got=accepted; else got=rejected; fi
expect "an unreleased gate passes at queue time" accepted "$got"
git_in tag tools-v0.1.112 "$base"
if gate_check; then got=accepted; else got=rejected; fi
expect "a gate released between PR CI and the queue entry is rejected" rejected "$got"

[ "$fails" = 0 ]
