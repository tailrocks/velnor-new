#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/private-names-policy-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT

fixture="$work/repo"
mkdir -p "$fixture/scripts"
cd "$fixture"
git init -q
git config user.email t@example.invalid
git config user.name t
cp "$repo/scripts/private-names-guard.py" "$repo/scripts/run-private-names-guard.sh" scripts/
python3 -I -c '
import importlib.util, sys
spec = importlib.util.spec_from_file_location("guard", sys.argv[1])
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
print(guard.entry("zzplanted"))
' "$repo/scripts/private-names-guard.py" > scripts/private-names-hashes.txt
echo clean > clean.md
git add -A
git commit -q -m base
base="$(git rev-parse HEAD)"
git remote add origin "$fixture"

printf 'import sys\nsys.exit(0)\n' > scripts/private-names-guard.py
: > scripts/private-names-hashes.txt
echo zzplanted > leak.md
git add -A
git commit -q -m head
head="$(git rev-parse HEAD)"

printf '{"pull_request": {"base": {"sha": "%s"}}}\n' "$base" > "$work/event.json"

fails=0
expect() {
  local name="$1" want="$2"
  shift 2
  local got=0
  "$@" > /dev/null 2>&1 || got=$?
  if { [ "$want" = fail ] && [ "$got" -ne 0 ]; } || { [ "$want" = pass ] && [ "$got" -eq 0 ]; }; then
    echo "ok   $name ($want)"
  else
    echo "FAIL $name: expected $want, exit $got"
    fails=$((fails + 1))
  fi
}

expect pull_request_uses_the_base_policy fail env GITHUB_EVENT_NAME=pull_request GITHUB_EVENT_PATH="$work/event.json" bash scripts/run-private-names-guard.sh
git reset -q --hard "$base"
: > scripts/private-names-hashes.txt
git commit -q -am emptied
base_empty="$(git rev-parse HEAD)"
python3 -I -c '
import importlib.util, sys
spec = importlib.util.spec_from_file_location("guard", sys.argv[1])
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
print(guard.entry("zzplanted"))
' "$repo/scripts/private-names-guard.py" > scripts/private-names-hashes.txt
echo zzplanted > leak.md
git add -A
git commit -q -m "head adds a protected name"
printf '{"pull_request": {"base": {"sha": "%s"}}}\n' "$base_empty" > "$work/event-new-name.json"
expect new_name_in_the_head_list_applies_to_the_same_pull_request fail env GITHUB_EVENT_NAME=pull_request GITHUB_EVENT_PATH="$work/event-new-name.json" bash scripts/run-private-names-guard.sh
git reset -q --hard "$head"
expect weakened_head_policy_alone_is_silent pass env GITHUB_EVENT_NAME=push python3 -I scripts/private-names-guard.py
expect push_uses_the_checked_out_policy pass env GITHUB_EVENT_NAME=push bash scripts/run-private-names-guard.sh

if [ "$fails" -ne 0 ]; then
  echo "private-names policy fixtures: $fails failing"
  exit 1
fi
echo "private-names policy fixtures: OK"
