#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

[[ $# -eq 2 ]] || { echo 'usage: test-plan-diagnostics.sh ABSOLUTE_CLI FRESH_ABSOLUTE_OUTPUT' >&2; exit 2; }
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
CLI=$1
OUTPUT=$2
[[ "$CLI" = /* && -f "$CLI" && ! -L "$CLI" && -x "$CLI" ]] || exit 2
[[ "$OUTPUT" = /* && ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || exit 2
parent="$(cd "$(dirname "$OUTPUT")" && pwd -P)"
OUTPUT="$parent/$(basename "$OUTPUT")"
case "$OUTPUT" in "$ROOT"|"$ROOT"/*) echo 'diagnostic evidence must remain outside the repository' >&2; exit 2 ;; esac
mkdir -m 700 "$OUTPUT"
mkdir "$OUTPUT/bindings"
cd "$ROOT"
source scripts/generator-release/qualification-goldens.sh
command -v node >/dev/null
cat >"$OUTPUT/candidate" <<'WRAPPER'
#!/bin/sh
if [ "${1-}" = plan ]; then
  physical_root=$(pwd -P) || exit 2
  printf 'Repository: %s\nPreview: %s/diagnostic-preview-%s\n' "$physical_root" "$TMPDIR" "$$" >&2
  case "${VELNOR_PLAN_DIAGNOSTICS_MODE-}:$physical_root" in
    fixture-warning:*/nested) printf '%s\n' 'warning: fixture-plan-diagnostic-regression' >&2 ;;
    dogfood-warning:*)
      if [ "$physical_root" = "$VELNOR_PLAN_DIAGNOSTICS_ROOT" ]; then
        printf '%s\n' 'warning: dogfood-plan-diagnostic-regression' >&2
      fi
      ;;
  esac
fi
if [ "${1-}" != generate ] || [ "$(pwd -P)" = "$VELNOR_PLAN_DIAGNOSTICS_ROOT" ]; then
  exec "$VELNOR_TEST_REAL_CLI_BIN" "$@"
fi
backup=$(mktemp -d "$VELNOR_PLAN_DIAGNOSTICS_BINDINGS/manifest.XXXXXX") || exit 2
manifest=.velnor/release-manifest.json
[ -f "$manifest" ] && [ ! -L "$manifest" ] && [ ! -L .velnor ] || exit 2
cp -p "$manifest" "$backup/original.json" || exit 2
restore() {
  status=$?
  trap - EXIT HUP INT TERM
  if ! cp -p "$backup/original.json" "$manifest" || ! cmp -s "$backup/original.json" "$manifest"; then status=1; fi
  exit "$status"
}
trap restore EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
node - "$manifest" "$VELNOR_TEST_HOST_TARGET" "$VELNOR_TEST_CANDIDATE_SHA256" "$VELNOR_TEST_REAL_CLI_SHA256" <<'JS'
const fs = require('node:fs');
const [file, target, candidate, real] = process.argv.slice(2);
const original = fs.readFileSync(file);
const document = JSON.parse(original);
const rows = document.targets.filter(row => row.target === target);
if (rows.length !== 1 || rows[0].sha256 !== candidate) throw new Error('fixture host binding differs');
const token = Buffer.from(`"sha256":"${candidate}"`);
const index = original.indexOf(token);
if (index < 0 || original.indexOf(token, index + 1) >= 0) throw new Error('fixture digest token is not unique');
const changed = Buffer.concat([original.subarray(0, index), Buffer.from(`"sha256":"${real}"`), original.subarray(index + token.length)]);
const expected = structuredClone(document);
expected.targets.find(row => row.target === target).sha256 = real;
if (!require('node:util').isDeepStrictEqual(JSON.parse(changed), expected)) throw new Error('fixture changed beyond host digest');
fs.writeFileSync(file, changed);
JS
[ "$?" -eq 0 ] || exit 2
"$VELNOR_TEST_REAL_CLI_BIN" "$@"
exit "$?"
WRAPPER
chmod u+x "$OUTPUT/candidate"
export VELNOR_TEST_REAL_CLI_BIN="$CLI"
VELNOR_TEST_REAL_CLI_SHA256="$(file_sha256 "$CLI")"
VELNOR_TEST_CANDIDATE_SHA256="$(file_sha256 "$OUTPUT/candidate")"
VELNOR_TEST_HOST_TARGET="$(host_target)"
export VELNOR_TEST_REAL_CLI_SHA256 VELNOR_TEST_CANDIDATE_SHA256 VELNOR_TEST_HOST_TARGET
export VELNOR_PLAN_DIAGNOSTICS_ROOT="$ROOT"
export VELNOR_PLAN_DIAGNOSTICS_BINDINGS="$OUTPUT/bindings"

run_collector() {
  local name=$1 mode=$2 operation=$3 status=0
  VELNOR_PLAN_DIAGNOSTICS_MODE="$mode" VELNOR_GOLDEN_DIR="$OUTPUT/oracle" \
    VELNOR_GOLDEN_KEEP_WORK=0 \
    bash "$ROOT/scripts/capture-opentofu-goldens.sh" "$operation" "$OUTPUT/candidate" \
    >"$OUTPUT/$name.log" 2>&1 || status=$?
  printf '%s\n' "$status" >"$OUTPUT/$name.exit"
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
