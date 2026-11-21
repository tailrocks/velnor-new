#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
config="${1:-$repo/.alint.yml}"
alint_bin="${ALINT:-alint}"
work="$(mktemp -d "${TMPDIR:-/tmp}/no-local-paths.XXXXXX")"
trap 'rm -rf "$work"' EXIT

# extract_rule <rule-id> <out-file>: the named rule from $config, without its exclude list.
extract_rule() {
  local id="$1" out="$2"
  {
    printf 'version: 1\nrules:\n'
    awk -v id="$id" '$0 == "  - id: " id {on = 1; print; next} on && /^  [^ ]/ {exit} on {print}' "$config" \
      | awk '/^      exclude:$/ {skip = 1; next} skip && /^        - / {next} {skip = 0; print}'
  } > "$out"
  grep -q "id: $id" "$out" || { echo "$id rule not found in $config" >&2; exit 2; }
}

mac="/Us""ers"
priv="/priv""ate/tmp"
root="/ro""ot"
home="/ho""me"
tmp="/t""mp"

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

rule="$work/rule.yml"
extract_rule no-absolute-local-paths "$rule"

check macos_home        fail "see ${mac}/someone/project/file.rs for details"
check macos_line_start  fail "${mac}/someone/project"
check macos_link        fail "[doc](${mac}/someone/repo/docs/a.md:3)"
check macos_quoted      fail "path=\"${mac}/someone/.cache/x\""
check macos_bare         fail "owner is ${mac}/someone"
check macos_bare_quoted  fail "home: \"${mac}/someone\""
check macos_bare_short   fail "owner is ${mac}/x"
check macos_bare_eol     fail "${mac}/x"
check linux_bare_short   fail "HOME=${home}/x"
check linux_home        fail "see ${home}/someone/work/file.rs"
check linux_runner_like fail "see ${home}/runner2/work/file.rs"
check linux_runner_pref fail "see ${home}/runnerx/work/file.rs"
check linux_short_name  fail "see ${home}/ru/work/file.rs"
check linux_r_name      fail "see ${home}/r/work/file.rs"
check linux_bare         fail "HOME=${home}/ci"
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

# no-local-temp-paths covers doc and evidence files only (.md, .json, .txt).
rule="$work/rule-temp.yml"
extract_rule no-local-temp-paths "$rule"
check temp_private_claude  fail "see ${priv}/claude-501/session/out.md"
check temp_private_plain   fail "log at ${priv}/velnor-jit.abc/out.txt"
check temp_private_var     fail "cache at /priv""ate/var/folders/ab/T/x.json"
check temp_bare_named      fail "wrote ${tmp}/velnor-audit-gap/report.json"
check temp_bare_file       fail "see ${tmp}/new.json"
check temp_bare_line_start fail "${tmp}/runner25-proof.log"
check temp_example_dir     pass "use ${tmp}/example/file.md as the sample"
check temp_example_plain   pass "use ${tmp}/foo for the sample"
check temp_placeholder     pass "wrote <tmp>/velnor-jit.abc/out.md"
check temp_repo_relative   pass "see docs/implemented/a.md and ../goals.md"
check temp_url_segment     pass "https://example.com${tmp}/velnor-x/page"
check temp_dotted_prefix   pass "pkg.${tmp}/velnor-x and a-${tmp}/velnor-y"

if [ "$fails" -ne 0 ]; then
  echo "no-local-temp-paths fixtures: $fails failing"
  exit 1
fi
echo "no-local-temp-paths fixtures: OK"
