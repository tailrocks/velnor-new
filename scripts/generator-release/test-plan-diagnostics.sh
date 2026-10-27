#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

[[ $# -eq 9 && "$1" = --archive-guard && "$3" = --archive-guard-sha256 \
  && "$5" = --diagnostics-fixture && "$7" = --diagnostics-fixture-sha256 ]] \
  || { echo 'usage: test-plan-diagnostics.sh --archive-guard PATH --archive-guard-sha256 SHA --diagnostics-fixture PATH --diagnostics-fixture-sha256 SHA FRESH_ABSOLUTE_OUTPUT' >&2; exit 2; }
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
QUALIFICATION_GUARD_INPUT_PATH=$2
QUALIFICATION_GUARD_INPUT_SHA256=$4
FIXTURE=$6
FIXTURE_SHA256=$8
OUTPUT=$9
[[ "$FIXTURE" = /* && -f "$FIXTURE" && ! -L "$FIXTURE" && -x "$FIXTURE" \
  && "$FIXTURE_SHA256" =~ ^[0-9a-f]{64}$ ]] || exit 2
[[ "$OUTPUT" = /* && ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || exit 2
parent="$(cd "$(dirname "$OUTPUT")" && pwd -P)"
OUTPUT="$parent/$(basename "$OUTPUT")"
case "$OUTPUT" in "$ROOT"|"$ROOT"/*) echo 'diagnostic evidence must remain outside the repository' >&2; exit 2 ;; esac
mkdir -m 700 "$OUTPUT"
DIAGNOSTICS_TMPDIR="$(cd "${TMPDIR:-/tmp}" && pwd -P)"
cd "$ROOT"
source scripts/generator-release/qualification-goldens.sh
command -v node >/dev/null
prepare_qualification_guard
[[ "$(file_sha256 "$FIXTURE")" = "$FIXTURE_SHA256" ]] || exit 2
cp -p "$FIXTURE" "$OUTPUT/candidate"
[[ "$(file_sha256 "$OUTPUT/candidate")" = "$FIXTURE_SHA256" ]] || exit 2
export VELNOR_PLAN_DIAGNOSTICS_ROOT="$ROOT"

assert_plan_bytes() {
  node - "$OUTPUT" "$1" "$2" "$3" "$4" "$ROOT" "$DIAGNOSTICS_TMPDIR" <<'JS'
const fs = require('node:fs');
const [output, name, mode, caseName, repo, root, temporary] = process.argv.slice(2);
const base = `${output}/${name}-${caseName}`;
const raw = fs.readFileSync(`${base}-plan.stderr.raw.txt`);
const staged = fs.readFileSync(`${base}-plan.stderr.txt`);
const previewPrefix = `Preview: ${temporary}/diagnostic-preview-`;
const preview = raw.toString('utf8').split('\n')[1] || '';
const pid = preview.slice(previewPrefix.length);
if (!preview.startsWith(previewPrefix) || !/^[0-9]+$/.test(pid)) {
  throw new Error(`unexpected raw preview identity for ${name}/${caseName}`);
}
let warning = '';
if (mode === 'fixture-warning' && caseName === 'nested') warning = 'warning: fixture-plan-diagnostic-regression\n';
if (mode === 'dogfood-warning' && caseName === 'dogfood') warning = 'warning: dogfood-plan-diagnostic-regression\n';
const expectedRaw = Buffer.from(`Repository: ${repo}\n${previewPrefix}${pid}\nPlan context: repository=${repo} source-root=${root}\n${warning}`);
const normalizedRepo = caseName === 'dogfood' ? '<root>' : '<repo>';
const expectedStaged = Buffer.from(`Repository: <repo>\nPreview: <preview>\nPlan context: repository=${normalizedRepo} source-root=<root>\n${warning}`);
if (!raw.equals(expectedRaw) || !staged.equals(expectedStaged)
    || !fs.readFileSync(`${base}-plan.exit`).equals(Buffer.from('0\n'))) {
  throw new Error(`raw, staged, or exit evidence differs for ${name}/${caseName}`);
}
JS
}

run_collector() {
  local name=$1 mode=$2 operation=$3 status=0
  local workspace workspace_base workspace_parent raw case_name case_dir source destination stage_case repo leaf
  VELNOR_PLAN_DIAGNOSTICS_MODE="$mode" TMPDIR="$DIAGNOSTICS_TMPDIR" VELNOR_GOLDEN_DIR="$OUTPUT/oracle" \
    VELNOR_GOLDEN_KEEP_WORK=1 \
    bash "$ROOT/scripts/capture-opentofu-goldens.sh" \
      --archive-guard "$QUALIFICATION_GUARD_INPUT_PATH" \
      --archive-guard-sha256 "$QUALIFICATION_GUARD_INPUT_SHA256" \
      "$operation" "$OUTPUT/candidate" \
    >"$OUTPUT/$name.log" 2>&1 || status=$?
  printf '%s\n' "$status" >"$OUTPUT/$name.exit"
  workspace="$(sed -n -e 's/^retaining successful golden evidence workspace: //p' \
    -e 's/^retaining failed golden workspace: //p' \
    "$OUTPUT/$name.log" | tail -n 1)"
  if [ -z "$workspace" ]; then return 1; fi
  workspace_base="${workspace##*/}"
  if [[ ! "$workspace_base" =~ ^velnor-goldens-work\.[A-Za-z0-9]+$ ]] \
    || [ -L "$workspace" ] || [ ! -d "$workspace" ]; then
    printf 'unsafe retained golden workspace: %s\n' "$workspace" >&2
    return 1
  fi
  workspace_parent="$(cd "$workspace/.." 2>/dev/null && pwd -P)" || return 1
  if [ "$workspace_parent" != "$DIAGNOSTICS_TMPDIR" ] \
    || [ "$(cd "$workspace" && pwd -P)" != "$workspace" ]; then
    printf 'retained golden workspace is outside physical TMPDIR: %s\n' "$workspace" >&2
    return 1
  fi
  raw="$workspace/raw"
  if [ -L "$raw" ]; then
    printf 'unsafe retained raw diagnostics directory: %s\n' "$raw" >&2
    return 1
  fi
  if [ ! -d "$raw" ]; then
    printf 'retained raw diagnostics path is not a directory: %s\n' "$raw" >&2
    return 1
  fi
  for case_name in nested mbx-nextest empty-suite minimal-cargo dogfood; do
    case_dir="$raw/$case_name"
    if [ -L "$case_dir" ]; then
      printf 'unsafe retained diagnostics case directory: %s\n' "$case_dir" >&2
      return 1
    fi
    if [ ! -d "$case_dir" ]; then
      printf 'retained diagnostics case path is not a directory: %s\n' "$case_dir" >&2
      return 1
    fi
    source="$case_dir/plan.stderr.raw.txt"
    if [ -L "$source" ]; then
      printf 'unsafe retained plan diagnostics file: %s\n' "$source" >&2
      return 1
    fi
    if [ ! -f "$source" ]; then
      printf 'retained plan diagnostics path is not a regular file: %s\n' "$source" >&2
      return 1
    fi
    destination="$OUTPUT/$name-$case_name-plan.stderr.raw.txt"
    if [ -e "$destination" ] || [ -L "$destination" ] || ! cp -p "$source" "$destination"; then
      printf 'could not safely preserve retained plan diagnostics: %s\n' "$source" >&2
      return 1
    fi
    stage_case="$workspace/stage/$case_name"
    if [ -L "$workspace/stage" ] || [ -L "$stage_case" ] || [ ! -d "$stage_case" ]; then return 1; fi
    for leaf in plan.stderr.txt plan.exit; do
      source="$stage_case/$leaf"
      destination="$OUTPUT/$name-$case_name-$leaf"
      if [ -L "$source" ] || [ ! -f "$source" ] || [ -e "$destination" ] \
        || [ -L "$destination" ] || ! cp -p "$source" "$destination"; then return 1; fi
    done
    repo="$workspace/$case_name"
    if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
    assert_plan_bytes "$name" "$mode" "$case_name" "$repo" || return 1
  done
  printf '%s\n' "$workspace" >"$OUTPUT/$name.workspace.txt"
}
run_collector capture control capture
[[ "$(cat "$OUTPUT/capture.exit")" = 0 ]] || { cat "$OUTPUT/capture.log" >&2; exit 1; }
run_collector control control check
[[ "$(cat "$OUTPUT/control.exit")" = 0 ]] || { cat "$OUTPUT/control.log" >&2; exit 1; }
run_collector fixture-warning fixture-warning check
run_collector dogfood-warning dogfood-warning check
final_status=0
for name in fixture-warning dogfood-warning; do
  if [[ "$(cat "$OUTPUT/$name.exit")" != 1 ]]; then
    printf 'FAIL: expected exactly one golden mismatch exit for %s\n' "$name" >&2
    final_status=1
  fi
  expected_case=nested
  if [ "$name" = dogfood-warning ]; then expected_case=dogfood; fi
  workspace="$(cat "$OUTPUT/$name.workspace.txt")"
  difference="$workspace/check-diff/$expected_case.diff"
  if [ "$(grep -Fc 'FAIL: golden mismatch:' "$OUTPUT/$name.log" || true)" != 1 ] \
    || ! grep -Fq "FAIL: golden mismatch: $expected_case " "$OUTPUT/$name.log" \
    || [ -L "$difference" ] || [ ! -f "$difference" ] \
    || [ "$(grep -c '^diff -r ' "$difference" || true)" != 1 ] \
    || ! grep -Fq '/plan.stderr.txt' "$difference" \
    || grep -q '^Only in ' "$difference" \
    || ! diff -r -x plan.stderr.txt "$OUTPUT/oracle/cases/$expected_case" \
      "$workspace/stage/$expected_case" >"$OUTPUT/$name.other-output.diff"; then
    printf 'FAIL: diagnostic mismatch scope differs for %s\n' "$name" >&2
    final_status=1
  fi
  cp -p "$difference" "$OUTPUT/$name.diff" || final_status=1
done
printf 'capture=%s control=%s fixture_warning=%s dogfood_warning=%s result=%s\n' \
  "$(cat "$OUTPUT/capture.exit")" "$(cat "$OUTPUT/control.exit")" \
  "$(cat "$OUTPUT/fixture-warning.exit")" "$(cat "$OUTPUT/dogfood-warning.exit")" \
  "$final_status" | tee "$OUTPUT/result.txt"
exit "$final_status"
