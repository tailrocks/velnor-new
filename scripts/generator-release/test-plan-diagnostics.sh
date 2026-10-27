#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail

POSITIONAL=()
ARCHIVE_GUARD_INPUT_PATH=""
ARCHIVE_GUARD_INPUT_SHA256=""
DIAGNOSTICS_FIXTURE_INPUT_PATH=""
DIAGNOSTICS_FIXTURE_INPUT_SHA256=""
SEEN_ARCHIVE_GUARD=0
SEEN_ARCHIVE_GUARD_SHA256=0
SEEN_DIAGNOSTICS_FIXTURE=0
SEEN_DIAGNOSTICS_FIXTURE_SHA256=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --archive-guard)
      if [ "$SEEN_ARCHIVE_GUARD" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo 'FATAL: archive guard path and SHA-256 options must appear once' >&2
        exit 2
      fi
      SEEN_ARCHIVE_GUARD=1
      ARCHIVE_GUARD_INPUT_PATH="$2"
      shift 2
      ;;
    --archive-guard-sha256)
      if [ "$SEEN_ARCHIVE_GUARD_SHA256" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo 'FATAL: archive guard path and SHA-256 options must appear once' >&2
        exit 2
      fi
      SEEN_ARCHIVE_GUARD_SHA256=1
      ARCHIVE_GUARD_INPUT_SHA256="$2"
      shift 2
      ;;
    --diagnostics-fixture)
      if [ "$SEEN_DIAGNOSTICS_FIXTURE" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo 'FATAL: diagnostics fixture path and SHA-256 options must appear once' >&2
        exit 2
      fi
      SEEN_DIAGNOSTICS_FIXTURE=1
      DIAGNOSTICS_FIXTURE_INPUT_PATH="$2"
      shift 2
      ;;
    --diagnostics-fixture-sha256)
      if [ "$SEEN_DIAGNOSTICS_FIXTURE_SHA256" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo 'FATAL: diagnostics fixture path and SHA-256 options must appear once' >&2
        exit 2
      fi
      SEEN_DIAGNOSTICS_FIXTURE_SHA256=1
      DIAGNOSTICS_FIXTURE_INPUT_SHA256="$2"
      shift 2
      ;;
    --*)
      echo 'usage: test-plan-diagnostics.sh --archive-guard PATH --archive-guard-sha256 SHA --diagnostics-fixture PATH --diagnostics-fixture-sha256 SHA ABSOLUTE_CLI FRESH_ABSOLUTE_OUTPUT' >&2
      exit 2
      ;;
    *) POSITIONAL+=("$1"); shift ;;
  esac
done
if [ "$SEEN_ARCHIVE_GUARD" -ne 1 ] || [ "$SEEN_ARCHIVE_GUARD_SHA256" -ne 1 ] \
  || [ "$SEEN_DIAGNOSTICS_FIXTURE" -ne 1 ] || [ "$SEEN_DIAGNOSTICS_FIXTURE_SHA256" -ne 1 ] \
  || [ "${#POSITIONAL[@]}" -ne 2 ] \
  || [ -z "$ARCHIVE_GUARD_INPUT_PATH" ] || [ -z "$ARCHIVE_GUARD_INPUT_SHA256" ] \
  || [ -z "$DIAGNOSTICS_FIXTURE_INPUT_PATH" ] || [ -z "$DIAGNOSTICS_FIXTURE_INPUT_SHA256" ]; then
  echo 'FATAL: explicit archive guard, diagnostics fixture, CLI, and output arguments are required' >&2
  exit 2
fi
if [[ ! "$ARCHIVE_GUARD_INPUT_SHA256" =~ ^[0-9a-f]{64}$ \
  || ! "$DIAGNOSTICS_FIXTURE_INPUT_SHA256" =~ ^[0-9a-f]{64}$ \
  || "$DIAGNOSTICS_FIXTURE_INPUT_PATH" != /* ]]; then
  echo 'FATAL: archive guard and diagnostics fixture identities must use absolute paths and lowercase SHA-256 values' >&2
  exit 2
fi
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
CLI="${POSITIONAL[0]}"
OUTPUT="${POSITIONAL[1]}"
[[ "$CLI" = /* ]] || { echo 'FATAL: CLI path must be absolute' >&2; exit 2; }
cli_parent="$(cd "$(dirname "$CLI")" 2>/dev/null && pwd -P)" || { echo 'FATAL: CLI parent must exist' >&2; exit 2; }
CLI="$cli_parent/$(basename "$CLI")"
[[ -f "$CLI" && -x "$CLI" ]] || { echo 'FATAL: CLI must be an executable file' >&2; exit 2; }
[[ -f "$DIAGNOSTICS_FIXTURE_INPUT_PATH" && ! -L "$DIAGNOSTICS_FIXTURE_INPUT_PATH" \
  && -x "$DIAGNOSTICS_FIXTURE_INPUT_PATH" ]] \
  || { echo 'FATAL: diagnostics fixture must be a regular executable file' >&2; exit 2; }
fixture_parent="$(cd "$(dirname "$DIAGNOSTICS_FIXTURE_INPUT_PATH")" 2>/dev/null && pwd -P)" \
  || { echo 'FATAL: diagnostics fixture parent must exist' >&2; exit 2; }
FIXTURE="$fixture_parent/$(basename "$DIAGNOSTICS_FIXTURE_INPUT_PATH")"
[[ "$OUTPUT" = /* && ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] \
  || { echo 'FATAL: output must be a fresh absolute path' >&2; exit 2; }
output_parent="$(cd "$(dirname "$OUTPUT")" 2>/dev/null && pwd -P)" \
  || { echo 'FATAL: output parent must exist' >&2; exit 2; }
OUTPUT="$output_parent/$(basename "$OUTPUT")"
case "$OUTPUT" in "$ROOT"|"$ROOT"/*) echo 'FATAL: diagnostic evidence must remain outside the repository' >&2; exit 2 ;; esac
CALLER_DIR="$(pwd -P)"
mkdir -m 700 "$OUTPUT"
QUALIFICATION_GUARD_INPUT_PATH="$ARCHIVE_GUARD_INPUT_PATH"
QUALIFICATION_GUARD_INPUT_SHA256="$ARCHIVE_GUARD_INPUT_SHA256"
source "$ROOT/scripts/generator-release/qualification-goldens.sh"
cd "$ROOT"
command -v node >/dev/null 2>&1 || { echo 'FATAL: Node.js is required for plan diagnostic qualification' >&2; exit 2; }
prepare_qualification_guard || exit 2
verify_qualification_guard || exit 2
CLI_SHA="$(file_sha256 "$CLI")" || exit 2
FIXTURE_SHA256="$DIAGNOSTICS_FIXTURE_INPUT_SHA256"
[[ "$(file_sha256 "$FIXTURE")" = "$FIXTURE_SHA256" ]] \
  || { echo 'FATAL: diagnostics fixture SHA-256 does not match the supplied identity' >&2; exit 2; }
cp -p "$FIXTURE" "$OUTPUT/qualification-diagnostics fixture"
FIXTURE_COPY="$OUTPUT/qualification-diagnostics fixture"
[[ "$(file_sha256 "$FIXTURE_COPY")" = "$FIXTURE_SHA256" ]] \
  || { echo 'FATAL: retained diagnostics fixture copy changed identity' >&2; exit 2; }

if [ -n "${VELNOR_GOLDEN_DIR:-}" ]; then
  case "$VELNOR_GOLDEN_DIR" in
    /*) requested_oracle="$VELNOR_GOLDEN_DIR" ;;
    *) requested_oracle="$CALLER_DIR/$VELNOR_GOLDEN_DIR" ;;
  esac
  oracle_name="${requested_oracle##*/}"
  oracle_parent="${requested_oracle%/*}"
  [[ -n "$oracle_name" && "$oracle_name" != . && "$oracle_name" != .. ]] \
    || { echo 'FATAL: selected external oracle path is invalid' >&2; exit 2; }
  oracle_parent="$(cd "$oracle_parent" 2>/dev/null && pwd -P)" \
    || { echo 'FATAL: selected external oracle parent must exist' >&2; exit 2; }
  ORACLE="$oracle_parent/$oracle_name"
  case "$ORACLE" in "$ROOT"|"$ROOT"/*) echo 'FATAL: selected external oracle must remain outside the repository' >&2; exit 2 ;; esac
else
  ORACLE="$ROOT/docs/proposed/opentofu-goldens"
fi
[[ -d "$ORACLE" && ! -L "$ORACLE" && -d "$ORACLE/cases" && ! -L "$ORACLE/cases" ]] \
  || { echo 'FATAL: selected golden oracle must be an existing regular directory tree' >&2; exit 2; }
printf '%s\n' "$ORACLE" >"$OUTPUT/selected-oracle.path.txt"

stdin_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  else
    shasum -a 256 | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  fi
}

golden_fingerprint() {
  node - "$1" <<'JS'
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const root = process.argv[2];
const rows = [];
function digest(bytes) { return crypto.createHash('sha256').update(bytes).digest('hex'); }
function walk(directory, relative) {
  for (const name of fs.readdirSync(directory).sort()) {
    const absolute = path.join(directory, name);
    const next = relative ? `${relative}/${name}` : name;
    const info = fs.lstatSync(absolute);
    if (info.isSymbolicLink()) rows.push(`link ./${next} -> ${fs.readlinkSync(absolute)}`);
    else if (info.isDirectory()) walk(absolute, next);
    else if (info.isFile()) rows.push(`${digest(fs.readFileSync(absolute))}  ./${next}`);
    else throw new Error(`unsupported oracle entry: ${next}`);
  }
}
walk(root, '');
process.stdout.write(`${digest(Buffer.from(`${rows.sort().join('\n')}\n`))}\n`);
JS
}

rebuild_oracle_manifest() {
  local directory="$1/cases" output="$1/MANIFEST.sha256"
  (
    cd "$directory" || exit 2
    files="$(find . -type f -print | LC_ALL=C sort)" || exit 2
    if [ -n "$files" ]; then
      if command -v sha256sum >/dev/null 2>&1; then
        printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 sha256sum || exit 2
      else
        printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 shasum -a 256 || exit 2
      fi
    fi
    find . -type l -print | LC_ALL=C sort | while IFS= read -r link; do
      printf 'link %s -> %s\n' "$link" "$(readlink "$link")"
    done
  ) >"$output"
}

ORACLE_BEFORE="$(golden_fingerprint "$ORACLE")" || exit 2
printf '%s\n' "$ORACLE_BEFORE" >"$OUTPUT/selected-oracle.fingerprint.before.txt"
for case_name in nested mbx-nextest empty-suite minimal-cargo dogfood; do
  plan_stderr="$ORACLE/cases/$case_name/plan.stderr.txt"
  [[ -f "$plan_stderr" && ! -L "$plan_stderr" && ! -s "$plan_stderr" ]] \
    || { printf 'FATAL: selected external plan stderr oracle must be empty for %s\n' "$case_name" >&2; exit 2; }
done
EMPTY_ORACLE="$OUTPUT/oracle-empty"
CONTROL_ORACLE="$OUTPUT/oracle-control"
EXPECTED_ORACLE="$OUTPUT/oracle-warning"
mkdir -m 700 "$EMPTY_ORACLE" "$CONTROL_ORACLE"
cp -RP "$ORACLE/." "$EMPTY_ORACLE/"
cp -RP "$ORACLE/." "$CONTROL_ORACLE/"
[[ "$(golden_fingerprint "$EMPTY_ORACLE")" = "$ORACLE_BEFORE" ]] \
  || { echo 'FATAL: copied empty oracle differs from the selected oracle' >&2; exit 2; }

ORIGINAL_PATH="$PATH"
REAL_MISE="$(command -v mise 2>/dev/null)" || REAL_MISE=''
case "$REAL_MISE" in
  /*) ;;
  '') echo 'FATAL: real Mise executable is unavailable' >&2; exit 2 ;;
  *) REAL_MISE="$(cd "$(dirname "$REAL_MISE")" 2>/dev/null && pwd -P)/$(basename "$REAL_MISE")" || exit 2 ;;
esac
[[ -x "$REAL_MISE" ]] || { echo "FATAL: real Mise executable is unavailable: $REAL_MISE" >&2; exit 2; }
RUST_VERSION="$(test_mise_rust_version)" || { echo 'FATAL: could not resolve the pinned Rust version' >&2; exit 2; }
CARGO_GUARD="${VELNOR_TEST_CARGO_GUARD:-${VELNOR_TEST_RUST_BUILD_GUARD:-}}"
if [[ "$(uname -s)" = Darwin && "${GITHUB_ACTIONS:-}" != true && -z "$CARGO_GUARD" ]]; then
  echo 'FATAL: set VELNOR_TEST_CARGO_GUARD to the required Rust build guard' >&2
  exit 2
fi
if [[ -n "$CARGO_GUARD" && ( "$CARGO_GUARD" != /* || ! -f "$CARGO_GUARD" || ! -x "$CARGO_GUARD" ) ]]; then
  echo "FATAL: the configured Rust build guard is unavailable: $CARGO_GUARD" >&2
  exit 2
fi
STUB_BIN="$OUTPUT/stub bin"
MISE_ADAPTER_BIN="$OUTPUT/mise adapter"
CARGO_BUILD_MARKER="$OUTPUT/implicit cargo build invoked"
mkdir -m 700 "$STUB_BIN"
cat >"$STUB_BIN/cargo" <<'STUB'
#!/bin/sh
if [ "${1-}" = build ]; then
  printf invoked >"$VELNOR_TEST_CARGO_MARKER"
  exit 79
fi
printf 'cargo-invocation=%s\n' "$*" >>"$VELNOR_TEST_CARGO_RECEIPT"
PATH="$VELNOR_TEST_REAL_CARGO_PATH"
export PATH
if [ -n "${VELNOR_TEST_CARGO_GUARD:-}" ]; then
  "$VELNOR_TEST_CARGO_GUARD" -- mise exec -- cargo "$@"
  guard_rc=$?
  printf 'guard-exit=%s\n' "$guard_rc" >>"$VELNOR_TEST_CARGO_RECEIPT"
  exit "$guard_rc"
fi
exit 79
STUB
chmod 755 "$STUB_BIN/cargo"
test_mise_resolve_cargo "$REAL_MISE" "$RUST_VERSION" "$OUTPUT/mise-cargo-resolution" "$STUB_BIN/cargo"
MISE_CARGO_PATH="$TEST_MISE_CARGO_PATH"
test_mise_adapter_create "$MISE_ADAPTER_BIN" "$REAL_MISE" "$ORIGINAL_PATH" "$CARGO_GUARD" "$RUST_VERSION"
MISE_ADAPTER_BIN="$TEST_MISE_ADAPTER_BIN"
export VELNOR_TEST_CARGO_GUARD="$CARGO_GUARD"

TMP_ROOT="$OUTPUT/tmp workspace"
SPACE_TMP_TARGET="$OUTPUT/physical tmp target"
SPACE_TMP_LINK="$OUTPUT/symlink tmpdir"
mkdir -m 700 "$TMP_ROOT" "$SPACE_TMP_TARGET"
ln -s "$SPACE_TMP_TARGET" "$SPACE_TMP_LINK"
[[ -L "$SPACE_TMP_LINK" && "$(cd "$SPACE_TMP_LINK" && pwd -P)" = "$(cd "$SPACE_TMP_TARGET" && pwd -P)" ]] \
  || { echo 'FATAL: spaced TMPDIR symlink fixture was not created' >&2; exit 2; }

ARCHIVE_GUARD_ARGS=(--archive-guard "$ARCHIVE_GUARD_INPUT_PATH" --archive-guard-sha256 "$ARCHIVE_GUARD_INPUT_SHA256")
CASES=(nested mbx-nextest empty-suite minimal-cargo dogfood)

verify_direct_cargo_receipt() {
  local label="$1" receipt="$2"
  if [ ! -s "$receipt" ]; then return 0; fi
  if [ -z "$CARGO_GUARD" ] || ! awk '
    /^cargo-invocation=/ { requested++ }
    /^guard-exit=/ { completed++; if ($0 != "guard-exit=0") failed=1 }
    END { if (requested == 0 || requested != completed || failed) exit 1 }
  ' "$receipt"; then
    cat "$receipt" >&2
    echo "FAIL: direct collector Cargo request did not complete through the configured guard: $label" >&2
    return 1
  fi
}

workspace_from_log() {
  local label="$1" workspace physical_tmp
  workspace="$(sed -n \
    -e 's/^retaining successful golden evidence workspace: //p' \
    -e 's/^retaining failed golden workspace: //p' "$OUTPUT/$label.log" | tail -n 1)"
  [[ -n "$workspace" && ! -L "$workspace" && -d "$workspace" ]] || {
    echo "FAIL: collector did not retain its workspace for $label" >&2
    return 1
  }
  [[ "${workspace##*/}" =~ ^velnor-goldens-work\.[A-Za-z0-9]+$ ]] || {
    echo "FAIL: retained workspace has an unexpected name for $label: $workspace" >&2
    return 1
  }
  physical_tmp="$(cd "$(cat "$OUTPUT/$label.tmpdir.txt")" 2>/dev/null && pwd -P)" || return 1
  [[ "$(cd "$workspace/.." 2>/dev/null && pwd -P)" = "$physical_tmp" \
    && "$(cd "$workspace" 2>/dev/null && pwd -P)" = "$workspace" ]] || {
    echo "FAIL: retained workspace is outside the selected physical TMPDIR for $label" >&2
    return 1
  }
  printf '%s\n' "$workspace" >"$OUTPUT/$label.workspace.txt"
  printf '%s\n' "$workspace"
}

preserve_plan_evidence() {
  local label="$1" workspace="$2" case_name="$3" required="$4"
  local raw_case="$workspace/raw/$case_name" stage_case="$workspace/stage/$case_name"
  local raw_source="$raw_case/plan.stderr.raw.txt" staged_source="$stage_case/plan.stderr.txt" exit_source="$stage_case/plan.exit"
  if [[ ! -d "$raw_case" || -L "$raw_case" || ! -d "$stage_case" || -L "$stage_case" ]]; then
    if [ "$required" = 1 ]; then
      echo "FAIL: collector did not retain plan evidence for $label/$case_name" >&2
      return 1
    fi
    return 0
  fi
  for source in "$raw_source" "$staged_source" "$exit_source"; do
    [[ -f "$source" && ! -L "$source" ]] || {
      if [ "$required" = 1 ]; then
        echo "FAIL: collector plan evidence is missing or unsafe for $label/$case_name" >&2
        return 1
      fi
      return 0
    }
  done
  cp -p "$raw_source" "$OUTPUT/$label-$case_name-plan.stderr.raw.txt"
  cp -p "$staged_source" "$OUTPUT/$label-$case_name-plan.stderr.txt"
  cp -p "$exit_source" "$OUTPUT/$label-$case_name-plan.exit"
}

run_collector() {
  local label="$1" mode="$2" oracle="$3" operation="$4" candidate="$5" tmpdir="$6" cases="$7"
  local mise_receipt="$OUTPUT/$label.mise.receipt" direct_receipt="$OUTPUT/$label.direct-cargo.receipt"
  local status=0 workspace case_name
  : >"$mise_receipt"
  : >"$direct_receipt"
  rm -f "$CARGO_BUILD_MARKER"
  printf 'operation=%s\nmode=%s\ncandidate=%s\nTMPDIR=%s\noracle=%s\n' \
    "$operation" "$mode" "$candidate" "$tmpdir" "$oracle" >"$OUTPUT/$label.command.txt"
  if (
    unset CARGO_TARGET_DIR CARGO_BUILD_TARGET
    export PATH="$STUB_BIN:$MISE_ADAPTER_BIN:$ORIGINAL_PATH"
    export MBX_GC_AUTO=0
    export TMPDIR="$tmpdir"
    export VELNOR_GOLDEN_DIR="$oracle"
    export VELNOR_GOLDEN_KEEP_WORK=1
    export VELNOR_TEST_CARGO_MARKER="$CARGO_BUILD_MARKER"
    export VELNOR_TEST_CARGO_RECEIPT="$direct_receipt"
    export VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH"
    export VELNOR_TEST_CARGO_GUARD="$CARGO_GUARD"
    export VELNOR_TEST_REAL_MISE="$REAL_MISE"
    export VELNOR_TEST_ORIGINAL_PATH="$ORIGINAL_PATH"
    export VELNOR_TEST_RUST_VERSION="$RUST_VERSION"
    export VELNOR_TEST_MISE_CARGO_PATH="$MISE_CARGO_PATH"
    export VELNOR_TEST_MISE_RECEIPT="$mise_receipt"
    export VELNOR_PLAN_DIAGNOSTICS_ROOT="$ROOT"
    if [ -n "$mode" ]; then export VELNOR_PLAN_DIAGNOSTICS_MODE="$mode"; else unset VELNOR_PLAN_DIAGNOSTICS_MODE; fi
    bash "$ROOT/scripts/capture-opentofu-goldens.sh" "${ARCHIVE_GUARD_ARGS[@]}" "$operation" "$candidate"
  ) >"$OUTPUT/$label.log" 2>&1; then
    status=0
  else
    status=$?
  fi
  printf '%s\n' "$status" >"$OUTPUT/$label.exit"
  test_mise_adapter_verify "$label" "$mise_receipt" "$RUST_VERSION" "$CARGO_GUARD" || return 1
  verify_direct_cargo_receipt "$label" "$direct_receipt" || return 1
  verify_qualification_guard || return 1
  if [ -e "$CARGO_BUILD_MARKER" ]; then
    echo "FAIL: collector invoked an implicit Cargo build for $label" >&2
    return 1
  fi
  printf '%s\n' "$tmpdir" >"$OUTPUT/$label.tmpdir.txt"
  workspace="$(workspace_from_log "$label")" || return 1
  for case_name in $cases; do
    preserve_plan_evidence "$label" "$workspace" "$case_name" 1 || return 1
  done
}

assert_plan_bytes() {
  node - "$OUTPUT" "$1" "$2" "$3" "$4" "$ROOT" <<'JS'
const fs = require('node:fs');
const [output, label, mode, caseName, repo, root] = process.argv.slice(2);
const base = `${output}/${label}-${caseName}`;
const raw = fs.readFileSync(`${base}-plan.stderr.raw.txt`);
const staged = fs.readFileSync(`${base}-plan.stderr.txt`);
const exit = fs.readFileSync(`${base}-plan.exit`, 'utf8');
let expectedRaw;
let expectedStaged;
let warning = '';
const warn = mode === 'warning-all'
  || ((mode === 'nested-only' || mode === 'fail57') && caseName === 'nested')
  || (mode === 'dogfood-only' && caseName === 'dogfood');
if (warn) warning = 'warning: plan-diagnostic-regression\n';
if (mode === 'baseline' || mode === 'quiet') {
  expectedRaw = Buffer.alloc(0);
  expectedStaged = Buffer.alloc(0);
} else {
  const lines = raw.toString('utf8').split('\n');
  if (lines.length !== (warn ? 5 : 4) || lines[0] !== `Repository: ${repo}`
      || lines[1] !== `Preview: ${repo}/diagnostic-preview`
      || lines[2] !== `Plan context: repository=${repo} source-root=${root}`) {
    throw new Error(`raw plan diagnostics have an unexpected shape for ${label}/${caseName}`);
  }
  expectedRaw = Buffer.from(`Repository: ${repo}\nPreview: ${repo}/diagnostic-preview\nPlan context: repository=${repo} source-root=${root}\n${warning}`);
  const normalizedRepo = caseName === 'dogfood' ? '<root>' : '<repo>';
  expectedStaged = Buffer.from(`Repository: ${normalizedRepo}\nPreview: ${normalizedRepo}/diagnostic-preview\nPlan context: repository=${normalizedRepo} source-root=<root>\n${warning}`);
}
const expectedExit = mode === 'fail57' && caseName === 'nested' ? '57\n' : '0\n';
if (!raw.equals(expectedRaw) || !staged.equals(expectedStaged) || exit !== expectedExit) {
  throw new Error(`raw, staged, or exit evidence differs for ${label}/${caseName}`);
}
JS
}

assert_mismatch_scope() {
  local label="$1" expected_case="$2" oracle="$3" workspace="$4" case_name difference
  if [ "$(grep -Fc 'FAIL: golden mismatch:' "$OUTPUT/$label.log" || true)" != 1 ] \
    || ! grep -Fq "FAIL: golden mismatch: $expected_case " "$OUTPUT/$label.log"; then
    echo "FAIL: expected one isolated golden mismatch for $label/$expected_case" >&2
    return 1
  fi
  for case_name in "${CASES[@]}"; do
    difference="$workspace/check-diff/$case_name.diff"
    if [ "$case_name" = "$expected_case" ]; then
      if [ "$(grep -c '^diff -r ' "$difference" || true)" != 1 ] \
        || ! grep -Fq '/plan.stderr.txt' "$difference" \
        || ! diff -r -x plan.stderr.txt "$oracle/cases/$case_name" "$workspace/stage/$case_name" \
          >"$OUTPUT/$label-$case_name.nonstderr.diff"; then
        echo "FAIL: $label changed files beyond the expected plan stderr" >&2
        return 1
      fi
    elif [ -s "$difference" ] \
      || ! diff -r "$oracle/cases/$case_name" "$workspace/stage/$case_name" \
        >"$OUTPUT/$label-$case_name.unaffected.diff"; then
      echo "FAIL: $label changed an unaffected scenario: $case_name" >&2
      return 1
    fi
  done
  cp -p "$workspace/check-diff/$expected_case.diff" "$OUTPUT/$label.diff" \
    || { echo "FATAL: could not retain mismatch evidence for $label/$expected_case" >&2; exit 2; }
}

behavior_failures=0
record_behavior_failure() {
  printf 'FAIL: behavior assertion: %s\n' "$1" >&2
  behavior_failures=$((behavior_failures + 1))
}

assert_all_diagnostics_differ_only_in_stderr() {
  local label="$1" oracle="$2" workspace="$3" case_name difference
  if [ "$(grep -Fc 'FAIL: golden mismatch:' "$OUTPUT/$label.log" || true)" != 5 ]; then
    echo "FAIL: $label did not produce exactly five plan stderr mismatches" >&2
    return 1
  fi
  for case_name in "${CASES[@]}"; do
    difference="$workspace/check-diff/$case_name.diff"
    if [ "$(grep -c '^diff -r ' "$difference" || true)" != 1 ] \
      || ! grep -Fq '/plan.stderr.txt' "$difference" \
      || ! grep -Fq "FAIL: golden mismatch: $case_name " "$OUTPUT/$label.log" \
      || ! diff -r -x plan.stderr.txt "$oracle/cases/$case_name" "$workspace/stage/$case_name" \
        >"$OUTPUT/$label-$case_name.nonstderr.diff"; then
      echo "FAIL: $label mismatch scope was broader than plan stderr for $case_name" >&2
      return 1
    fi
  done
}

assert_collector_status() {
  local label="$1" expected="$2" actual
  actual="$(cat "$OUTPUT/$label.exit")"
  if [ "$actual" != "$expected" ]; then
    cat "$OUTPUT/$label.log" >&2
    printf 'FAIL: collector exit for %s was %s, expected %s\n' "$label" "$actual" "$expected" >&2
    return 1
  fi
}

mkdir -m 700 "$OUTPUT/oracle-warning"
cp -RP "$CONTROL_ORACLE/." "$EXPECTED_ORACLE/"
for case_name in "${CASES[@]}"; do
  if [ "$case_name" = dogfood ]; then normalized_repo='<root>'; else normalized_repo='<repo>'; fi
  printf 'Repository: %s\nPreview: %s/diagnostic-preview\nPlan context: repository=%s source-root=<root>\nwarning: plan-diagnostic-regression\n' \
    "$normalized_repo" "$normalized_repo" "$normalized_repo" >"$EXPECTED_ORACLE/cases/$case_name/plan.stderr.txt"
done
rebuild_oracle_manifest "$EXPECTED_ORACLE"

NORMAL_TMPDIR="$TMP_ROOT"
run_collector baseline '' "$EMPTY_ORACLE" check "$CLI" "$NORMAL_TMPDIR" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status baseline 0 || record_behavior_failure 'baseline collector exit'
[[ "$(grep -Fc 'ALL GOLDENS MATCH' "$OUTPUT/baseline.log")" = 1 ]] \
  || record_behavior_failure 'baseline all-goldens-match output'
for case_name in "${CASES[@]}"; do
  repo="$OUTPUT/baseline.workspace.txt"
  repo="$(cat "$repo")/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes baseline baseline "$case_name" "$repo" \
    || record_behavior_failure "baseline plan bytes for $case_name"
done

run_collector quiet quiet "$EMPTY_ORACLE" check "$FIXTURE_COPY" "$NORMAL_TMPDIR" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status quiet 0 || record_behavior_failure 'quiet collector exit'
[[ "$(grep -Fc 'ALL GOLDENS MATCH' "$OUTPUT/quiet.log")" = 1 ]] \
  || record_behavior_failure 'quiet all-goldens-match output'
for case_name in "${CASES[@]}"; do
  workspace="$(cat "$OUTPUT/quiet.workspace.txt")"
  repo="$workspace/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes quiet quiet "$case_name" "$repo" \
    || record_behavior_failure "quiet plan bytes for $case_name"
done

run_collector control-capture control "$CONTROL_ORACLE" capture "$FIXTURE_COPY" "$NORMAL_TMPDIR" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status control-capture 0 || record_behavior_failure 'control capture collector exit'
for case_name in "${CASES[@]}"; do
  workspace="$(cat "$OUTPUT/control-capture.workspace.txt")"
  repo="$workspace/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes control-capture control "$case_name" "$repo" \
    || record_behavior_failure "control capture plan bytes for $case_name"
done
run_collector control-check control "$CONTROL_ORACLE" check "$FIXTURE_COPY" "$NORMAL_TMPDIR" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status control-check 0 || record_behavior_failure 'control check collector exit'
[[ "$(grep -Fc 'ALL GOLDENS MATCH' "$OUTPUT/control-check.log")" = 1 ]] \
  || record_behavior_failure 'control check all-goldens-match output'
for case_name in "${CASES[@]}"; do
  workspace="$(cat "$OUTPUT/control-check.workspace.txt")"
  repo="$workspace/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes control-check control "$case_name" "$repo" \
    || record_behavior_failure "control check plan bytes for $case_name"
done

run_collector warning-empty warning-all "$EMPTY_ORACLE" check "$FIXTURE_COPY" "$NORMAL_TMPDIR" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status warning-empty 1 || record_behavior_failure 'warning-empty collector exit'
workspace="$(cat "$OUTPUT/warning-empty.workspace.txt")"
for case_name in "${CASES[@]}"; do
  repo="$workspace/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes warning-empty warning-all "$case_name" "$repo" \
    || record_behavior_failure "warning-empty plan bytes for $case_name"
done
assert_all_diagnostics_differ_only_in_stderr warning-empty "$EMPTY_ORACLE" "$workspace" \
  || record_behavior_failure 'warning-empty mismatch scope'

run_collector warning-expected warning-all "$EXPECTED_ORACLE" check "$FIXTURE_COPY" "$NORMAL_TMPDIR" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status warning-expected 0 || record_behavior_failure 'warning-expected collector exit'
[[ "$(grep -Fc 'ALL GOLDENS MATCH' "$OUTPUT/warning-expected.log")" = 1 ]] \
  || record_behavior_failure 'warning-expected all-goldens-match output'
workspace="$(cat "$OUTPUT/warning-expected.workspace.txt")"
for case_name in "${CASES[@]}"; do
  repo="$workspace/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes warning-expected warning-all "$case_name" "$repo" \
    || record_behavior_failure "warning-expected plan bytes for $case_name"
done

run_collector nested-only nested-only "$CONTROL_ORACLE" check "$FIXTURE_COPY" "$NORMAL_TMPDIR" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status nested-only 1 || record_behavior_failure 'nested-only collector exit'
workspace="$(cat "$OUTPUT/nested-only.workspace.txt")"
for case_name in "${CASES[@]}"; do
  repo="$workspace/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes nested-only nested-only "$case_name" "$repo" \
    || record_behavior_failure "nested-only plan bytes for $case_name"
done
assert_mismatch_scope nested-only nested "$CONTROL_ORACLE" "$workspace" \
  || record_behavior_failure 'nested-only mismatch scope'

run_collector dogfood-only dogfood-only "$CONTROL_ORACLE" check "$FIXTURE_COPY" "$NORMAL_TMPDIR" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status dogfood-only 1 || record_behavior_failure 'dogfood-only collector exit'
workspace="$(cat "$OUTPUT/dogfood-only.workspace.txt")"
for case_name in "${CASES[@]}"; do
  repo="$workspace/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes dogfood-only dogfood-only "$case_name" "$repo" \
    || record_behavior_failure "dogfood-only plan bytes for $case_name"
done
assert_mismatch_scope dogfood-only dogfood "$CONTROL_ORACLE" "$workspace" \
  || record_behavior_failure 'dogfood-only mismatch scope'

SPACED_FIXTURE_DIR="$OUTPUT/space path"
mkdir -m 700 "$SPACED_FIXTURE_DIR"
SPACED_FIXTURE="$SPACED_FIXTURE_DIR/qualification-diagnostics"
cp -p "$FIXTURE_COPY" "$SPACED_FIXTURE"
[[ "$(file_sha256 "$SPACED_FIXTURE")" = "$FIXTURE_SHA256" ]] \
  || { echo 'FATAL: spaced diagnostics fixture copy changed identity' >&2; exit 2; }
run_collector warning-spaced warning-all "$EXPECTED_ORACLE" check "$SPACED_FIXTURE" "$SPACE_TMP_LINK" 'nested mbx-nextest empty-suite minimal-cargo dogfood'
assert_collector_status warning-spaced 0 || record_behavior_failure 'warning-spaced collector exit'
[[ "$(grep -Fc 'ALL GOLDENS MATCH' "$OUTPUT/warning-spaced.log")" = 1 ]] \
  || record_behavior_failure 'warning-spaced all-goldens-match output'
workspace="$(cat "$OUTPUT/warning-spaced.workspace.txt")"
for case_name in "${CASES[@]}"; do
  repo="$workspace/$case_name"
  if [ "$case_name" = dogfood ]; then repo="$ROOT"; fi
  assert_plan_bytes warning-spaced warning-all "$case_name" "$repo" \
    || record_behavior_failure "warning-spaced plan bytes for $case_name"
done

run_collector fail57 fail57 "$CONTROL_ORACLE" check "$FIXTURE_COPY" "$NORMAL_TMPDIR" nested
assert_collector_status fail57 2 || record_behavior_failure 'fail57 collector exit'
workspace="$(cat "$OUTPUT/fail57.workspace.txt")"
repo="$workspace/nested"
assert_plan_bytes fail57 fail57 nested "$repo" \
  || record_behavior_failure 'fail57 plan bytes'
if ! grep -Fq 'FATAL: plan failed for nested' "$OUTPUT/fail57.log" \
  || [ -e "$workspace/stage/nested/generate.exit" ] \
  || [ -d "$workspace/stage/nested/preview/.github" ]; then
  cat "$OUTPUT/fail57.log" >&2
  echo 'FAIL: injected plan failure did not follow a successful plan without running generate' >&2
  record_behavior_failure 'fail57 did not preserve the post-plan failure boundary'
fi
node - "$OUTPUT/fail57.log" "$OUTPUT/fail57-nested-plan.stderr.raw.txt" <<'JS' \
  || record_behavior_failure 'fail57 raw stderr was not surfaced byte-for-byte'
const fs = require('node:fs');
const [logPath, rawPath] = process.argv.slice(2);
const log = fs.readFileSync(logPath);
const raw = fs.readFileSync(rawPath);
if (raw.length === 0 || log.indexOf(raw) < 0) {
  throw new Error('failed plan stderr was not surfaced byte-for-byte in the collector log');
}
JS

ORACLE_AFTER="$(golden_fingerprint "$ORACLE")" || exit 2
[[ "$ORACLE_AFTER" = "$ORACLE_BEFORE" ]] \
  || { echo 'FATAL: diagnostic qualification changed the selected oracle' >&2; exit 2; }
[[ "$(file_sha256 "$CLI")" = "$CLI_SHA" \
  && "$(file_sha256 "$FIXTURE")" = "$FIXTURE_SHA256" ]] \
  || { echo 'FATAL: diagnostic qualification changed the CLI or fixture identity' >&2; exit 2; }
verify_qualification_guard || exit 2
printf 'selected_oracle_sha256=%s\ncontrol_oracle_sha256=%s\nexpected_warning_oracle_sha256=%s\ncli_sha256=%s\nfixture_sha256=%s\narchive_guard_sha256=%s\n' \
  "$ORACLE_AFTER" "$(golden_fingerprint "$CONTROL_ORACLE")" \
  "$(golden_fingerprint "$EXPECTED_ORACLE")" "$CLI_SHA" "$FIXTURE_SHA256" \
  "$QUALIFICATION_GUARD_SHA256" >"$OUTPUT/result.txt"
printf 'baseline=0 quiet=0 control_capture=0 control_check=0 warning_empty=1 warning_expected=0 nested_only=1 dogfood_only=1 warning_spaced=0 fail57=2\n'
printf 'diagnostic evidence: %s\n' "$OUTPUT"
if [ "$behavior_failures" -ne 0 ]; then
  printf 'diagnostic behavior failures=%s\n' "$behavior_failures" >&2
  exit 1
fi
