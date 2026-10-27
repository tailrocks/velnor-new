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

run_collector() {
  local name=$1 mode=$2 operation=$3 status=0
  local workspace workspace_base workspace_parent raw case_name case_dir source destination
  VELNOR_PLAN_DIAGNOSTICS_MODE="$mode" TMPDIR="$DIAGNOSTICS_TMPDIR" VELNOR_GOLDEN_DIR="$OUTPUT/oracle" \
    VELNOR_GOLDEN_KEEP_WORK=0 \
    bash "$ROOT/scripts/capture-opentofu-goldens.sh" \
      --archive-guard "$QUALIFICATION_GUARD_INPUT_PATH" \
      --archive-guard-sha256 "$QUALIFICATION_GUARD_INPUT_SHA256" \
      "$operation" "$OUTPUT/candidate" \
    >"$OUTPUT/$name.log" 2>&1 || status=$?
  printf '%s\n' "$status" >"$OUTPUT/$name.exit"
  workspace="$(sed -n -e 's/^retaining successful golden evidence workspace: //p' \
    -e 's/^retaining failed golden workspace: //p' \
    "$OUTPUT/$name.log" | tail -n 1)"
  if [ -z "$workspace" ]; then return 0; fi
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
  if [ ! -e "$raw" ]; then return 0; fi
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
    if [ ! -e "$case_dir" ]; then continue; fi
    if [ ! -d "$case_dir" ]; then
      printf 'retained diagnostics case path is not a directory: %s\n' "$case_dir" >&2
      return 1
    fi
    source="$case_dir/plan.stderr.raw.txt"
    if [ -L "$source" ]; then
      printf 'unsafe retained plan diagnostics file: %s\n' "$source" >&2
      return 1
    fi
    if [ ! -e "$source" ]; then continue; fi
    if [ ! -f "$source" ]; then
      printf 'retained plan diagnostics path is not a regular file: %s\n' "$source" >&2
      return 1
    fi
    destination="$OUTPUT/$name-$case_name-plan.stderr.raw.txt"
    if [ -e "$destination" ] || [ -L "$destination" ] || ! cp -p "$source" "$destination"; then
      printf 'could not safely preserve retained plan diagnostics: %s\n' "$source" >&2
      return 1
    fi
  done
}
run_collector capture control capture
[[ "$(cat "$OUTPUT/capture.exit")" = 0 ]] || { cat "$OUTPUT/capture.log" >&2; exit 1; }
run_collector control control check
[[ "$(cat "$OUTPUT/control.exit")" = 0 ]] || { cat "$OUTPUT/control.log" >&2; exit 1; }
run_collector fixture-warning fixture-warning check
run_collector dogfood-warning dogfood-warning check
final_status=0
for name in fixture-warning dogfood-warning; do
  if [[ "$(cat "$OUTPUT/$name.exit")" = 0 ]]; then
    printf 'FAIL: successful plan diagnostics were discarded: %s\n' "$name" >&2
    final_status=1
  fi
done
if ! grep -Fq 'golden mismatch: nested' "$OUTPUT/fixture-warning.log"; then final_status=1; fi
if ! grep -Fq 'golden mismatch: dogfood' "$OUTPUT/dogfood-warning.log"; then final_status=1; fi
printf 'capture=%s control=%s fixture_warning=%s dogfood_warning=%s result=%s\n' \
  "$(cat "$OUTPUT/capture.exit")" "$(cat "$OUTPUT/control.exit")" \
  "$(cat "$OUTPUT/fixture-warning.exit")" "$(cat "$OUTPUT/dogfood-warning.exit")" \
  "$final_status" | tee "$OUTPUT/result.txt"
exit "$final_status"
