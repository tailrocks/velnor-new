#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
config="${1:-$repo/.alint.yml}"
alint_bin="${ALINT:-alint}"
work="$(mktemp -d "${TMPDIR:-/tmp}/no-local-paths.XXXXXX")"
trap 'rm -rf "$work"' EXIT

rule="$work/rule.yml"
{
  printf 'version: 1\nrules:\n'
  awk '/^  - id: no-absolute-local-paths$/ {on = 1; print; next} on && /^  [^ ]/ {exit} on {print}' "$config" \
    | awk '/^      exclude:$/ {skip = 1; next} skip && /^        - / {next} {skip = 0; print}'
} > "$rule"
grep -q 'id: no-absolute-local-paths' "$rule" || { echo "no-absolute-local-paths rule not found in $config" >&2; exit 2; }

mac="/Us""ers"
priv="/priv""ate/tmp"
root="/ro""ot"
home="/ho""me"

fails=0
check() {
  local name="$1" expect="$2" dir="$work/$1"
  mkdir -p "$dir/docs"
  printf '%s\n' "$3" > "$dir/docs/fixture.md"
  if "$alint_bin" check -c "$rule" "$dir" >/dev/null 2>&1; then got=pass; else got=fail; fi
  if [ "$got" != "$expect" ]; then
    echo "FAIL $name: expected $expect, got $got"
    fails=$((fails + 1))
  else
    echo "ok   $name ($got)"
  fi
}

check macos_home        fail "see ${mac}/someone/project/file.rs for details"
check macos_line_start  fail "${mac}/someone/project"
check macos_link        fail "[doc](${mac}/someone/repo/docs/a.md:3)"
check macos_quoted      fail "path=\"${mac}/someone/.cache/x\""
check linux_home        fail "see ${home}/someone/work/file.rs"
check linux_runner_like fail "see ${home}/runner2/work/file.rs"
check linux_runner_pref fail "see ${home}/runnerx/work/file.rs"
check linux_short_name  fail "see ${home}/ru/work/file.rs"
check linux_r_name      fail "see ${home}/r/work/file.rs"
check agent_scratch     fail "see ${priv}/claude-501/session/out.txt"
check root_home         fail "kept under ${root}/.cache/artifacts/log.txt"
check root_line_start   fail "${root}/work/log.txt"
check gh_runner_work    pass "log at ${home}/runner/work/repo/repo/out.txt"
check gh_runner_config  pass "wrote ${home}/runner/.docker/config.json"
check gh_runner_bare    pass "home is ${home}/runner"
check placeholder       pass "kept under <retained-cache>/artifacts/log.txt"
check relative_path     pass "see ../../goals.md and docs/implemented/a.md"
check url_segment       pass "https://example.com${mac}/someone/page and https://example.com${root}/page"
check dotted_prefix     pass "pkg.${root}/x and a-${mac}/someone/y"

if [ "$fails" -ne 0 ]; then
  echo "no-absolute-local-paths fixtures: $fails failing"
  exit 1
fi
echo "no-absolute-local-paths fixtures: OK"
