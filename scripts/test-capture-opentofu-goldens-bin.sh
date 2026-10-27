#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Exercise the collector's explicit CLI path without changing checked-in goldens.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$ROOT/scripts/capture-opentofu-goldens.sh"
GOLDEN_DIR="$ROOT/docs/proposed/opentofu-goldens"
ARCHIVE_GUARD_INPUT_PATH=""
ARCHIVE_GUARD_INPUT_SHA256=""
SEEN_ARCHIVE_GUARD=0
SEEN_ARCHIVE_GUARD_SHA256=0
CLI_ARG=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --archive-guard)
      if [ "$SEEN_ARCHIVE_GUARD" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo "FATAL: archive guard options must appear once as a path and SHA-256 pair"
        exit 2
      fi
      SEEN_ARCHIVE_GUARD=1
      ARCHIVE_GUARD_INPUT_PATH="$2"
      shift 2
      ;;
    --archive-guard-sha256)
      if [ "$SEEN_ARCHIVE_GUARD_SHA256" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo "FATAL: archive guard options must appear once as a path and SHA-256 pair"
        exit 2
      fi
      SEEN_ARCHIVE_GUARD_SHA256=1
      ARCHIVE_GUARD_INPUT_SHA256="$2"
      shift 2
      ;;
    --*)
      echo "FATAL: usage: $0 --archive-guard PATH --archive-guard-sha256 SHA256 CLI_BINARY"
      exit 2
      ;;
    *)
      if [ -n "$CLI_ARG" ]; then
        echo "FATAL: usage: $0 --archive-guard PATH --archive-guard-sha256 SHA256 CLI_BINARY"
        exit 2
      fi
      CLI_ARG="$1"
      shift
      ;;
  esac
done
if [ "$SEEN_ARCHIVE_GUARD" -ne "$SEEN_ARCHIVE_GUARD_SHA256" ] || [ -z "$CLI_ARG" ]; then
  echo "FATAL: archive guard path and SHA-256 must be supplied together with a CLI binary"
  exit 2
fi
if [ "$SEEN_ARCHIVE_GUARD" -eq 1 ] \
  && { [ -z "$ARCHIVE_GUARD_INPUT_PATH" ] || [ -z "$ARCHIVE_GUARD_INPUT_SHA256" ]; }; then
  echo "FATAL: archive guard path and SHA-256 must be non-empty"
  exit 2
fi
QUALIFICATION_GUARD_INPUT_PATH="$ARCHIVE_GUARD_INPUT_PATH"
QUALIFICATION_GUARD_INPUT_SHA256="$ARCHIVE_GUARD_INPUT_SHA256"
source "$ROOT/scripts/generator-release/qualification-goldens.sh"
ARCHIVE_GUARD_ARGS=()
if ! CALLER_DIR="$(pwd -P)"; then
  echo "FATAL: could not resolve caller directory"
  exit 2
fi
case "$CLI_ARG" in
  /*) CLI_BIN="$CLI_ARG" ;;
  *) CLI_BIN="$CALLER_DIR/$CLI_ARG" ;;
esac
if [ ! -f "$CLI_BIN" ] || [ ! -x "$CLI_BIN" ]; then
  echo "FATAL: CLI must be a regular executable file: $CLI_BIN"
  exit 2
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/velnor-goldens-bin-test.XXXXXX")" || exit 2
WORK="$(cd "$WORK" && pwd -P)" || exit 2
cleanup_work() {
  local status=$?
  trap - EXIT
  if [ "$status" -eq 0 ]; then
    rm -rf "$WORK"
  else
    printf 'retaining failed golden harness workspace: %s\n' "$WORK" >&2
  fi
  exit "$status"
}
trap cleanup_work EXIT
STUB_BIN="$WORK/stub bin"
MARKER="$WORK/cargo-invoked"
ORIGINAL_PATH="$PATH"
mkdir -p "$STUB_BIN" "$WORK/tmp workspace"
ln -s "$WORK/tmp workspace" "$WORK/tmp symlink"
# Only `cargo build` is the collector's implicit debug build. `plan` runs
# `cargo metadata` through the real toolchain; poisoning that call makes
# the spaced-path check fail before it can prove the collector skipped build.
cat >"$STUB_BIN/cargo" <<'STUB'
#!/bin/sh
if [ "${1-}" = "build" ]; then
  printf invoked >"$VELNOR_TEST_CARGO_MARKER"
  exit 79
fi
PATH="$VELNOR_TEST_REAL_CARGO_PATH"
export PATH
cargo "$@"
STUB
chmod u+x "$STUB_BIN/cargo"

REAL_MISE="$(command -v mise)"
if [ -z "$REAL_MISE" ]; then
  echo 'FATAL: mise is required for Cargo metadata fault tests' >&2
  exit 2
fi
case "$REAL_MISE" in
  /*) ;;
  *) REAL_MISE="$(cd "$(dirname "$REAL_MISE")" && pwd -P)/$(basename "$REAL_MISE")" || exit 2 ;;
esac
FAULT_MISE_TEMPLATE="$WORK/mise fault template"
mkdir -m 700 "$FAULT_MISE_TEMPLATE" || exit 2
cat >"$FAULT_MISE_TEMPLATE/mise" <<'MISE'
#!/usr/bin/env bash
set -u
base="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)" || exit 88
real_mise="$(cat "$base/realpath")" || exit 88
mode="$(cat "$base/mode")" || exit 88
args=("$@")
metadata=0
if [ "${#args[@]}" -ge 12 ] \
  && [ "${args[0]}" = --no-config ] \
  && [ "${args[1]}" = --no-env ] \
  && [ "${args[2]}" = --no-hooks ] \
  && [ "${args[3]}" = exec ]; then
  separator=-1
  for ((index = 4; index < ${#args[@]}; index++)); do
    if [ "${args[index]}" = -- ]; then
      separator=$index
      break
    fi
  done
  if [ "$separator" -ge 5 ] \
    && [ "${#args[@]}" -eq "$((separator + 8))" ] \
    && [ "${args[separator + 1]}" = cargo ] \
    && [ "${args[separator + 2]}" = metadata ] \
    && [ "${args[separator + 3]}" = --format-version ] \
    && [ "${args[separator + 4]}" = 1 ] \
    && [ "${args[separator + 5]}" = --no-deps ] \
    && [ "${args[separator + 6]}" = --manifest-path ] \
    && [ -n "${args[separator + 7]}" ]; then
    metadata=1
  fi
fi
"$real_mise" "${args[@]}"
status=$?
if [ "$status" -ne 0 ] || [ "$metadata" -ne 1 ]; then
  exit "$status"
fi
case "$mode" in
  guard-mutation|metadata-failure)
    current="$(pwd -P)" || exit 88
    [ "${current##*/}" = nested ] || exit 0
    ;;
  dogfood-guard-mutation)
    current="$(pwd -P)" || exit 88
    [ "${current##*/}" = dogfood-repo ] || exit 0
    ;;
  *) exit 88 ;;
esac
mkdir "$base/claim" 2>/dev/null || exit 0
printf invoked >"$base/metadata-succeeded" || exit 88
if [ "$mode" = metadata-failure ]; then
  printf 'real-mise-status=%s\nmode=%s\nfixture=%s\n' "$status" "$mode" "${current##*/}" >"$base/receipt" || exit 88
  exit 57
fi
guard_copy="$(cat "$base/guard-copy")" || exit 88
case "$guard_copy" in
  /*) ;;
  *) exit 88 ;;
esac
printf changed >>"$guard_copy" || exit 88
if command -v sha256sum >/dev/null 2>&1; then
  changed_sha="$(sha256sum "$guard_copy" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')" || exit 88
else
  changed_sha="$(shasum -a 256 "$guard_copy" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')" || exit 88
fi
printf 'real-mise-status=%s\nmode=%s\nfixture=%s\nguard-copy=%s\nguard-sha256=%s\n' \
  "$status" "$mode" "${current##*/}" "$guard_copy" "$changed_sha" >"$base/receipt" || exit 88
exit "$status"
MISE
chmod 700 "$FAULT_MISE_TEMPLATE/mise"

prepare_fault_mise() {
  local mode="$1" guard_copy="${2-}" fault_dir="$WORK/mise fault $1"
  mkdir -m 700 "$fault_dir" || return 2
  cp "$FAULT_MISE_TEMPLATE/mise" "$fault_dir/mise" || return 2
  chmod 700 "$fault_dir/mise" || return 2
  printf '%s\n' "$REAL_MISE" >"$fault_dir/realpath" || return 2
  printf '%s\n' "$mode" >"$fault_dir/mode" || return 2
  printf '%s\n' "$guard_copy" >"$fault_dir/guard-copy" || return 2
  printf '%s\n' "$fault_dir"
}

REAL_BASH="$(command -v bash)"
export VELNOR_TEST_REAL_BASH="$REAL_BASH"
export VELNOR_TEST_GUARD_BUILDER_PATH="$ROOT/scripts/build-owned-archive-guard.sh"
export VELNOR_TEST_GUARD_BUILDER_MARKER="$WORK/guard builder invoked"
cat >"$STUB_BIN/bash" <<'BASH'
#!/bin/sh
if [ "${1-}" = "${VELNOR_TEST_GUARD_BUILDER_PATH-}" ]; then
  printf invoked >"$VELNOR_TEST_GUARD_BUILDER_MARKER"
  exit 89
fi
exec "$VELNOR_TEST_REAL_BASH" "$@"
BASH
chmod 755 "$STUB_BIN/bash"

test_file_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  else
    shasum -a 256 "$1" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  fi
}

run_collector() {
  local -a guard_args
  if [ "${1-}" = --guard-args ]; then
    shift
    guard_args=()
    while [ "$#" -gt 0 ] && [ "$1" != -- ]; do
      guard_args+=("$1")
      shift
    done
    if [ "$#" -eq 0 ]; then
      echo 'FATAL: run_collector guard options require a -- separator' >&2
      return 2
    fi
    shift
  else
    guard_args=("${ARCHIVE_GUARD_ARGS[@]}")
  fi
  "$SCRIPT" "${guard_args[@]}" "$@"
}

test_stdin_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  else
    shasum -a 256 | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  fi
}

golden_fingerprint() {
  (
    cd "$GOLDEN_DIR" || exit 2
    {
      files="$(find . -type f -print | LC_ALL=C sort)" || exit 2
      if [ -n "$files" ]; then
        if command -v sha256sum >/dev/null 2>&1; then
          printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 sha256sum || exit 2
        else
          printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 shasum -a 256 || exit 2
        fi
      fi
      find . -type l -print | LC_ALL=C sort | while IFS= read -r path; do
        printf 'link %s -> %s\n' "$path" "$(readlink "$path")"
      done
    } | test_stdin_sha256
  )
}

BEFORE="$(golden_fingerprint)" || exit 2
SOURCE_SHA="$(git -C "$ROOT" rev-parse HEAD)" || exit 2
prepare_qualification_guard || exit 2
verify_qualification_guard || exit 2
if [ "$SEEN_ARCHIVE_GUARD" -eq 0 ]; then
  ARCHIVE_GUARD_INPUT_PATH="$QUALIFICATION_GUARD_BIN"
  ARCHIVE_GUARD_INPUT_SHA256="$QUALIFICATION_GUARD_SHA256"
fi
ARCHIVE_GUARD_ARGS=(--archive-guard "$ARCHIVE_GUARD_INPUT_PATH" --archive-guard-sha256 "$ARCHIVE_GUARD_INPUT_SHA256")
PATH="$STUB_BIN:$ORIGINAL_PATH"
export PATH
CLI_VERSION="$("$CLI_BIN" --version | awk 'NR == 1 && NF == 2 && $1 == "velnor-actions" { print $2; next } { exit 1 } END { if (NR != 1) exit 1 }')" || exit 2
verify_qualification_guard || exit 2
if command -v sha256sum >/dev/null 2>&1; then
  CLI_SHA="$(sha256sum "$CLI_BIN" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')" || exit 2
else
  CLI_SHA="$(shasum -a 256 "$CLI_BIN" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')" || exit 2
fi
case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64) HOST_TARGET="x86_64-unknown-linux-gnu" ;;
  Darwin:arm64|Darwin:aarch64) HOST_TARGET="aarch64-apple-darwin" ;;
  Darwin:x86_64|Darwin:amd64) HOST_TARGET="x86_64-apple-darwin" ;;
  *) echo "FATAL: unsupported test host $(uname -s)/$(uname -m)" >&2; exit 2 ;;
esac
case "$HOST_TARGET" in
  x86_64-unknown-linux-gnu)
    HOST_TARGET_INDEX=0
    OTHER_TARGET_INDEX=1
    TEST_LINUX_SHA="$CLI_SHA"
    TEST_ARM_SHA="$(printf '%064d' 0 | tr '0' 'b')"
    TEST_INTEL_SHA="$(printf '%064d' 0 | tr '0' 'c')"
    ;;
  aarch64-apple-darwin)
    HOST_TARGET_INDEX=1
    OTHER_TARGET_INDEX=0
    TEST_LINUX_SHA="$(printf '%064d' 0 | tr '0' 'b')"
    TEST_ARM_SHA="$CLI_SHA"
    TEST_INTEL_SHA="$(printf '%064d' 0 | tr '0' 'c')"
    ;;
  x86_64-apple-darwin)
    HOST_TARGET_INDEX=2
    OTHER_TARGET_INDEX=0
    TEST_LINUX_SHA="$(printf '%064d' 0 | tr '0' 'b')"
    TEST_ARM_SHA="$(printf '%064d' 0 | tr '0' 'c')"
    TEST_INTEL_SHA="$CLI_SHA"
    ;;
esac

write_test_manifest() {
  local manifest="$1" commit="$2" linux_sha="$3" arm_sha="$4" intel_sha="$5"
  jq -n --arg version "$CLI_VERSION" --arg commit "$commit" \
    --arg linux "$linux_sha" --arg arm "$arm_sha" --arg intel "$intel_sha" '
    {schema:1, version:$version, repository:"tailrocks/velnor-new", commit:$commit,
     targets:[
       {target:"x86_64-unknown-linux-gnu",
        artifact:("https://github.com/tailrocks/velnor-new/releases/download/v" + $version + "/velnor-actions-" + $version + "-x86_64-unknown-linux-gnu"),
        sha256:$linux},
       {target:"aarch64-apple-darwin",
        artifact:("https://github.com/tailrocks/velnor-new/releases/download/v" + $version + "/velnor-actions-" + $version + "-aarch64-apple-darwin"),
        sha256:$arm},
       {target:"x86_64-apple-darwin",
        artifact:("https://github.com/tailrocks/velnor-new/releases/download/v" + $version + "/velnor-actions-" + $version + "-x86_64-apple-darwin"),
        sha256:$intel}
     ]}' >"$manifest"
}

source "$ROOT/scripts/generator-release/test-fixture-git.sh"
test_fixture_git_isolation || exit 1
source "$ROOT/scripts/generator-release/test-qualification-output-bindings.sh"

test_candidate_stderr_diagnostic() {
  local source="$WORK/candidate stderr source.txt"
  local rendered="$WORK/candidate stderr diagnostic.txt"
  {
    printf 'safe line\n::error::candidate text\n##[error]candidate text\n##[add-mask::secret]\nescape\033[31mred\033[0m\rline\n'
    awk 'BEGIN { for (i = 0; i < 20000; i++) printf "x" }'
  } >"$source"
  emit_candidate_stderr "$source" 2>"$rendered"
  if [ "$(wc -c <"$rendered" | tr -d '[:space:]')" -gt 16384 ] \
    || ! grep -Fq '[candidate stderr] safe line' "$rendered" \
    || ! grep -Fq '[candidate stderr] ::error::candidate text' "$rendered" \
    || ! grep -Fq '[candidate stderr] # #[error]candidate text' "$rendered" \
    || ! grep -Fq '[candidate stderr] # #[add-mask::secret]' "$rendered" \
    || grep -Fq '##[' "$rendered" \
    || ! grep -Fq '[candidate stderr] escape[31mred[0mline' "$rendered" \
    || grep -Fq $'\033' "$rendered" || grep -Fq $'\r' "$rendered"; then
    cat "$rendered" >&2
    echo 'FAIL: candidate stderr diagnostic was not bounded and sanitized' >&2
    exit 1
  fi
  if ! LC_ALL=C awk '
      !/^\[candidate stderr\] / { exit 1 }
      END { if (NR == 0) exit 1 }
    ' "$rendered"; then
    cat "$rendered" >&2
    echo 'FAIL: candidate stderr diagnostic contains an unprefixed line' >&2
    exit 1
  fi
  if LC_ALL=C tr -d '\n' <"$rendered" | LC_ALL=C grep '[[:cntrl:]]' >/dev/null; then
    echo 'FAIL: candidate stderr diagnostic retained terminal control bytes' >&2
    exit 1
  fi
  echo 'passed bounded, prefixed, sanitized candidate stderr diagnostic'
}

test_candidate_stderr_diagnostic

expect_release_check_match() {
  local status=0 log="$WORK/release-check.log" after
  local manifest="${1:-$WORK/valid manifest.json}"
  TMPDIR="$WORK/tmp symlink" GITHUB_SHA="$SOURCE_SHA" GITHUB_REPOSITORY=tailrocks/velnor-new \
    PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    run_collector check-release "$CLI_BIN" "$manifest" \
      "$(test_file_sha256 "$manifest")" >"$log" 2>&1 || status=$?
  if [ "$status" -ne 0 ] || ! grep -Fq 'ALL RELEASE FIXTURE GOLDENS AND DOGFOOD PARITY MATCH' "$log"; then
    cat "$log" >&2
    echo "FAIL: valid candidate did not match release fixture goldens (exit $status)" >&2
    exit 1
  fi
  if [ -e "$MARKER" ]; then echo "FAIL: release qualification invoked cargo build" >&2; exit 1; fi
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then echo "FAIL: successful release check changed goldens" >&2; exit 1; fi
  echo "passed positive release candidate fixture oracle"
}

expect_release_rejected() {
  local label="$1" expected="$2" manifest="$3" digest="$4" source="$5"
  local status=0 log="$WORK/$label.log"
  GITHUB_SHA="$source" GITHUB_REPOSITORY=tailrocks/velnor-new \
    PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    run_collector check-release "$CLI_BIN" "$manifest" "$digest" >"$log" 2>&1 || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" "$log"; then
    cat "$log" >&2
    echo "FAIL: release manifest rejection $label exit=$status did not report $expected" >&2
    exit 1
  fi
  if [ -e "$MARKER" ]; then
    echo "FAIL: release manifest rejection $label unexpectedly invoked cargo" >&2
    exit 1
  fi
  local after
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: release manifest rejection $label changed goldens" >&2
    exit 1
  fi
  echo "passed release manifest rejection: $label"
}

expect_release_rejected_without_message() {
  local label="$1" manifest="$2" digest="$3" source="$4"
  local status=0 log="$WORK/$label.log"
  GITHUB_SHA="$source" GITHUB_REPOSITORY=tailrocks/velnor-new \
    PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    run_collector check-release "$CLI_BIN" "$manifest" "$digest" >"$log" 2>&1 || status=$?
  if [ "$status" -eq 0 ]; then
    cat "$log" >&2
    echo "FAIL: release manifest rejection $label unexpectedly succeeded" >&2
    exit 1
  fi
  if [ -e "$MARKER" ]; then
    echo "FAIL: release manifest rejection $label unexpectedly invoked cargo" >&2
    exit 1
  fi
  local after
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: release manifest rejection $label changed goldens" >&2
    exit 1
  fi
  echo "passed release manifest rejection: $label"
}

expect_rejected() {
  local label="$1" expected="$2" status=0
  shift 2
  PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    run_collector "$@" >"$WORK/$label.log" 2>&1 || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" "$WORK/$label.log"; then
    cat "$WORK/$label.log" >&2
    echo "FAIL: $label exit=$status did not report $expected" >&2
    exit 1
  fi
  if [ -e "$MARKER" ]; then
    echo "FAIL: $label unexpectedly invoked cargo" >&2
    exit 1
  fi
  local after
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: $label changed goldens" >&2
    exit 1
  fi
echo "passed rejection: $label"
}

expect_guard_options_rejected() {
  local label="$1" expected="$2" status=0
  local log="$WORK/$label.log"
  shift 2
  PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    VELNOR_TEST_REAL_BASH="$REAL_BASH" \
    VELNOR_TEST_GUARD_BUILDER_PATH="$ROOT/scripts/build-owned-archive-guard.sh" \
    VELNOR_TEST_GUARD_BUILDER_MARKER="$WORK/$label builder invoked" \
    VELNOR_TEST_PARSER_CANDIDATE_MARKER="$WORK/$label candidate invoked" \
    run_collector --guard-args "$@" -- check "$PARSER_SENTINEL_BIN" >"$log" 2>&1 || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" "$log"; then
    cat "$log" >&2
    echo "FAIL: $label exit=$status did not report $expected" >&2
    exit 1
  fi
  if [ -e "$MARKER" ]; then
    echo "FAIL: $label unexpectedly invoked Cargo build" >&2
    exit 1
  fi
  if [ -e "$WORK/$label builder invoked" ] || [ -e "$WORK/$label candidate invoked" ]; then
    echo "FAIL: $label executed the archive guard builder or candidate CLI" >&2
    exit 1
  fi
  local after
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: $label changed goldens" >&2
    exit 1
  fi
  echo "passed rejection: $label"
}

expect_rejected missing-manifest-argument 'usage:' check-release "$CLI_BIN"
expect_rejected missing-manifest-sha-argument 'usage:' \
  check-release "$CLI_BIN" "$WORK/missing candidate manifest"
expect_release_rejected missing-manifest 'candidate manifest must be a regular non-symlink file' \
  "$WORK/missing candidate manifest" "$(printf '%064d' 0)" "$SOURCE_SHA"
mkdir "$WORK/directory manifest"
expect_release_rejected directory-manifest 'candidate manifest must be a regular non-symlink file' \
  "$WORK/directory manifest" "$(printf '%064d' 0)" "$SOURCE_SHA"
if ! mkfifo "$WORK/fifo manifest"; then
  echo "FATAL: could not create FIFO manifest fixture" >&2
  exit 2
fi
expect_release_rejected fifo-manifest 'candidate manifest must be a regular non-symlink file' \
  "$WORK/fifo manifest" "$(printf '%064d' 0)" "$SOURCE_SHA"
dd if=/dev/zero of="$WORK/oversized manifest.json" bs=8388609 count=1 2>/dev/null
expect_release_rejected oversized-manifest 'candidate manifest must not exceed 8388608 bytes' \
  "$WORK/oversized manifest.json" "$(printf '%064d' 0)" "$SOURCE_SHA"
printf 'not json\n' >"$WORK/malformed manifest.json"
expect_release_rejected malformed-manifest 'candidate manifest is malformed JSON' \
  "$WORK/malformed manifest.json" "$(test_file_sha256 "$WORK/malformed manifest.json")" "$SOURCE_SHA"

write_test_manifest "$WORK/valid manifest.json" "$SOURCE_SHA" \
  "$TEST_LINUX_SHA" "$TEST_ARM_SHA" "$TEST_INTEL_SHA"
cp "$WORK/valid manifest.json" "$WORK/tmp workspace/valid manifest.json"
OUTPUT_BINDING_SCRIPT="$WORK/run_collector"
cat >"$OUTPUT_BINDING_SCRIPT" <<'WRAPPER'
#!/usr/bin/env bash
exec "$VELNOR_TEST_CAPTURE_SCRIPT" --archive-guard "$VELNOR_TEST_ARCHIVE_GUARD_PATH" \
  --archive-guard-sha256 "$VELNOR_TEST_ARCHIVE_GUARD_SHA256" "$@"
WRAPPER
chmod u+x "$OUTPUT_BINDING_SCRIPT"
SCRIPT_WITH_GUARD="$SCRIPT"
export VELNOR_TEST_CAPTURE_SCRIPT="$SCRIPT_WITH_GUARD"
export VELNOR_TEST_ARCHIVE_GUARD_PATH="$ARCHIVE_GUARD_INPUT_PATH"
export VELNOR_TEST_ARCHIVE_GUARD_SHA256="$ARCHIVE_GUARD_INPUT_SHA256"
if [ "${VELNOR_TEST_QUALIFICATION_OUTPUT_ONLY:-0}" = 1 ]; then
  SCRIPT="$OUTPUT_BINDING_SCRIPT"
  run_candidate_output_binding_tests || exit $?
  exit 0
fi
expect_release_check_match "$WORK/tmp symlink/valid manifest.json"
SCRIPT="$OUTPUT_BINDING_SCRIPT"
run_candidate_output_binding_tests || exit $?
SCRIPT="$SCRIPT_WITH_GUARD"
unset VELNOR_TEST_CAPTURE_SCRIPT VELNOR_TEST_ARCHIVE_GUARD_PATH VELNOR_TEST_ARCHIVE_GUARD_SHA256
ln -s "$WORK/valid manifest.json" "$WORK/symlink manifest.json"
expect_release_rejected symlink-manifest 'candidate manifest must be a regular non-symlink file' \
  "$WORK/symlink manifest.json" "$(test_file_sha256 "$WORK/valid manifest.json")" "$SOURCE_SHA"
expect_release_rejected malformed-github-sha 'GITHUB_SHA must be a lowercase 40-character source SHA' \
  "$WORK/valid manifest.json" "$(test_file_sha256 "$WORK/valid manifest.json")" 'invalid-source-sha'
sed 's/"schema": 1/"schema": 1, "schema": 1/' \
  "$WORK/valid manifest.json" >"$WORK/duplicate keys.json"
expect_release_rejected_without_message duplicate-manifest-keys \
  "$WORK/duplicate keys.json" "$(test_file_sha256 "$WORK/duplicate keys.json")" "$SOURCE_SHA"
expect_release_rejected wrong-manifest-sha 'candidate manifest bytes do not match expected SHA-256' \
  "$WORK/valid manifest.json" "$(printf '%064d' 0)" "$SOURCE_SHA"
jq --argjson index "$HOST_TARGET_INDEX" \
  '.targets[$index].sha256 = "0000000000000000000000000000000000000000000000000000000000000000"' \
  "$WORK/valid manifest.json" >"$WORK/wrong digest.json"
expect_release_rejected wrong-digest 'candidate manifest digest does not match candidate CLI' \
  "$WORK/wrong digest.json" "$(test_file_sha256 "$WORK/wrong digest.json")" "$SOURCE_SHA"
jq --argjson host "$HOST_TARGET_INDEX" --argjson other "$OTHER_TARGET_INDEX" \
  --arg digest "$CLI_SHA" \
  '.targets[$host].sha256 = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd" |
   .targets[$other].sha256 = $digest' \
  "$WORK/valid manifest.json" >"$WORK/wrong host target.json"
expect_release_rejected wrong-host-target 'candidate manifest digest does not match candidate CLI' \
  "$WORK/wrong host target.json" "$(test_file_sha256 "$WORK/wrong host target.json")" "$SOURCE_SHA"
write_test_manifest "$WORK/wrong source.json" "$(printf '%040d' 0 | tr '0' 'b')" \
  "$TEST_LINUX_SHA" "$TEST_ARM_SHA" "$TEST_INTEL_SHA"
expect_release_rejected wrong-manifest-source 'candidate manifest source does not match GITHUB_SHA' \
  "$WORK/wrong source.json" "$(test_file_sha256 "$WORK/wrong source.json")" "$SOURCE_SHA"
expect_release_rejected wrong-checkout-source 'checked-out source does not match GITHUB_SHA' \
  "$WORK/valid manifest.json" "$(test_file_sha256 "$WORK/valid manifest.json")" \
  "$(printf '%040d' 0 | tr '0' 'f')"

expect_unsupported_binary() {
  local status=0 log="$WORK/failing-binary.log"
  if (
    cd "$WORK" || exit 2
    TMPDIR="$WORK/tmp workspace" PATH="$STUB_BIN:$ORIGINAL_PATH" \
      VELNOR_TEST_CARGO_MARKER="$MARKER" \
      VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
      VELNOR_TEST_FAILING_BIN_MARKER="$WORK/failing binary invoked" \
      run_collector capture "$FAILING_BIN"
  ) >"$log" 2>&1; then
    cat "$log" >&2
    echo "FAIL: executable that exits nonzero unexpectedly succeeded" >&2
    exit 1
  else
    status=$?
  fi
  if [ "$status" -ne 2 ] || ! grep -Fq 'FATAL: unsupported or ambiguous fixture binary target:' "$log"; then
    cat "$log" >&2
    echo "FAIL: unsupported executable target was not reported (exit $status)" >&2
    exit 1
  fi
  if [ ! -f "$WORK/failing binary invoked" ] || [ -e "$MARKER" ]; then
    echo "FAIL: explicit failing binary was not run or cargo fallback occurred" >&2
    exit 1
  fi
  if ! grep -Fq 'retaining failed golden workspace:' "$log"; then
    cat "$log" >&2
    echo "FAIL: failure workspace was not retained for diagnosis" >&2
    exit 1
  fi
  local retained_work
  retained_work="$(sed -n 's/^retaining failed golden workspace: //p' "$log")"
  if [[ -z "$retained_work" \
    || "$retained_work" != "$WORK/tmp workspace"/velnor-goldens-work.* \
    || ! -d "$retained_work" ]]; then
    echo "FAIL: reported failure workspace is absent or outside the test TMPDIR" >&2
    exit 1
  fi
  local after
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: failed explicit binary changed goldens" >&2
    exit 1
  fi
  echo "passed unsupported executable rejection without cargo fallback or golden changes"
}

expect_execution_failure() {
  local status=0 log="$WORK/executed-plan-failure.log"
  local mise_dir="$METADATA_FAILURE_MISE_DIR"
  if (
    cd "$WORK" || exit 2
    TMPDIR="$WORK/tmp workspace" PATH="$mise_dir:$STUB_BIN:$ORIGINAL_PATH" \
      VELNOR_TEST_CARGO_MARKER="$MARKER" \
      VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
      run_collector capture "$CLI_BIN"
  ) >"$log" 2>&1; then
    cat "$log" >&2
    echo "FAIL: native CLI unexpectedly passed a forced Cargo metadata failure" >&2
    exit 1
  else
    status=$?
  fi
  if [ "$status" -ne 2 ] || ! grep -Fq 'FATAL: plan failed for nested' "$log"; then
    cat "$log" >&2
    echo "FAIL: native CLI plan failure was not preserved (exit $status)" >&2
    exit 1
  fi
  if [ ! -f "$mise_dir/metadata-succeeded" ] \
    || ! grep -Fq 'real-mise-status=0' "$mise_dir/receipt" \
    || ! grep -Fq 'mode=metadata-failure' "$mise_dir/receipt" \
    || ! grep -Fq 'fixture=nested' "$mise_dir/receipt" \
    || [ -e "$MARKER" ]; then
    echo "FAIL: Cargo metadata failure did not run or implicit cargo build occurred" >&2
    exit 1
  fi
  if ! grep -Fq 'retaining failed golden workspace:' "$log"; then
    cat "$log" >&2
    echo "FAIL: plan failure workspace was not retained" >&2
    exit 1
  fi
  local retained_work after
  retained_work="$(sed -n 's/^retaining failed golden workspace: //p' "$log")"
  if [[ -z "$retained_work" \
    || "$retained_work" != "$WORK/tmp workspace"/velnor-goldens-work.* \
    || ! -d "$retained_work" ]]; then
    echo "FAIL: reported plan failure workspace is absent or outside test TMPDIR" >&2
    exit 1
  fi
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: executed plan failure changed goldens" >&2
    exit 1
  fi
  echo "passed native CLI executed-plan failure without cargo fallback or golden changes"
}
METADATA_FAILURE_MISE_DIR="$(prepare_fault_mise metadata-failure)" || exit 2

mkdir -p "$WORK/guard test"
chmod 700 "$WORK/guard test"
expect_guard_rejected() {
  local label="$1" expected="$2" path="$3" digest="$4" status=0
  local log="$WORK/guard-$label.log"
  PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    VELNOR_TEST_REAL_BASH="$REAL_BASH" \
    VELNOR_TEST_GUARD_BUILDER_PATH="$ROOT/scripts/build-owned-archive-guard.sh" \
    VELNOR_TEST_GUARD_BUILDER_MARKER="$WORK/guard builder invoked" \
    run_collector --guard-args --archive-guard "$path" --archive-guard-sha256 "$digest" \
      -- check "$CLI_BIN" \
      >"$log" 2>&1 || status=$?
  if [ "$status" -ne 2 ] || ! grep -Fq "$expected" "$log"; then
    cat "$log" >&2
    echo "FAIL: archive guard rejection $label exit=$status did not report $expected" >&2
    exit 1
  fi
  if [ -e "$MARKER" ] || [ -e "$WORK/guard builder invoked" ]; then
    echo "FAIL: archive guard rejection $label invoked a builder or Cargo" >&2
    exit 1
  fi
  local after
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: archive guard rejection $label changed goldens" >&2
    exit 1
  fi
}
expect_guard_rejected missing 'explicit archive guard must be a regular executable in a private directory' "$WORK/missing guard" "$CLI_SHA"
expect_guard_rejected malformed 'explicit archive guard path and lowercase SHA-256 are required' \
  "$ARCHIVE_GUARD_INPUT_PATH" malformed
expect_guard_rejected missing-file 'explicit archive guard must be a regular executable in a private directory' \
  "$WORK/guard test/missing" "$ARCHIVE_GUARD_INPUT_SHA256"
expect_guard_rejected directory 'explicit archive guard must be a regular executable in a private directory' \
  "$WORK/guard test" "$ARCHIVE_GUARD_INPUT_SHA256"
ln -s "$ARCHIVE_GUARD_INPUT_PATH" "$WORK/guard test/symlink guard"
expect_guard_rejected symlink 'explicit archive guard must be a regular executable in a private directory' \
  "$WORK/guard test/symlink guard" "$ARCHIVE_GUARD_INPUT_SHA256"
cp "$ARCHIVE_GUARD_INPUT_PATH" "$WORK/guard test/unexecutable guard"
chmod 600 "$WORK/guard test/unexecutable guard"
expect_guard_rejected permissions 'explicit archive guard must be a regular executable in a private directory' \
  "$WORK/guard test/unexecutable guard" "$ARCHIVE_GUARD_INPUT_SHA256"
cp "$ARCHIVE_GUARD_INPUT_PATH" "$WORK/guard test/linked guard"
ln "$WORK/guard test/linked guard" "$WORK/guard test/multiple links"
expect_guard_rejected links 'explicit archive guard ownership, permissions, or link count are unsafe' \
  "$WORK/guard test/multiple links" "$ARCHIVE_GUARD_INPUT_SHA256"
expect_guard_rejected digest 'explicit archive guard SHA-256 does not match supplied value' \
  "$ARCHIVE_GUARD_INPUT_PATH" "$(printf '%064d' 0)"
cat >"$WORK/guard test/wrong digest guard" <<'GUARD'
#!/bin/sh
printf invoked >"$VELNOR_TEST_HOSTILE_GUARD_MARKER"
exit 0
GUARD
chmod 700 "$WORK/guard test/wrong digest guard"
PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_HOSTILE_GUARD_MARKER="$WORK/hostile guard invoked" \
  expect_guard_rejected malicious-digest 'explicit archive guard SHA-256 does not match supplied value' \
    "$WORK/guard test/wrong digest guard" "$(printf '%064d' 0)"
if [ -e "$WORK/hostile guard invoked" ]; then
  echo "FAIL: digest-mismatched archive guard was executed" >&2
  exit 1
fi
STALE_SOURCE_ROOT="$WORK/stale source"
mkdir -p "$STALE_SOURCE_ROOT"
git -C "$ROOT" archive HEAD | tar -x -C "$STALE_SOURCE_ROOT" || exit 2
printf '\n' >>"$STALE_SOURCE_ROOT/crates/velnor-archive-guard/src/lib.rs"
STALE_SOURCE_HELPER="$ROOT/scripts/generator-release/qualification-goldens.sh"
if (
  ROOT="$STALE_SOURCE_ROOT" \
    QUALIFICATION_GUARD_INPUT_PATH="$ARCHIVE_GUARD_INPUT_PATH" \
    QUALIFICATION_GUARD_INPUT_SHA256="$ARCHIVE_GUARD_INPUT_SHA256" \
    REAL_HELPER="$STALE_SOURCE_HELPER" \
    bash -c 'source "$REAL_HELPER" && prepare_qualification_guard'
) >"$WORK/stale-guard-source.log" 2>&1; then
  cat "$WORK/stale-guard-source.log" >&2
  echo "FAIL: archive guard accepted a modified source closure" >&2
  exit 1
fi
if ! grep -Fq 'trusted qualification archive guard source fingerprint is stale' "$WORK/stale-guard-source.log" \
  || [ -e "$STALE_SOURCE_ROOT/.velnor/archive-guard/bin/velnor-archive-guard" ]; then
  cat "$WORK/stale-guard-source.log" >&2
  echo "FAIL: stale archive guard did not fail before builder acquisition" >&2
  exit 1
fi
MUTATED_GUARD_PATH="$WORK/guard test/mutable guard"
cp "$ARCHIVE_GUARD_INPUT_PATH" "$MUTATED_GUARD_PATH"
MUTATED_GUARD_SHA="$(test_file_sha256 "$MUTATED_GUARD_PATH")"
expect_guard_mutation_rejected() {
  local status=0 log="$WORK/guard-mutation.log"
  local mise_dir="$GUARD_MUTATION_MISE_DIR"
  if (
    cd "$WORK" || exit 2
    TMPDIR="$WORK/tmp workspace" PATH="$mise_dir:$STUB_BIN:$ORIGINAL_PATH" \
      VELNOR_TEST_CARGO_MARKER="$MARKER" \
      VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
      run_collector --guard-args --archive-guard "$MUTATED_GUARD_PATH" \
        --archive-guard-sha256 "$MUTATED_GUARD_SHA" -- capture "$CLI_BIN"
  ) >"$log" 2>&1; then
    cat "$log" >&2
    echo "FAIL: candidate execution continued after archive guard mutation" >&2
    exit 1
  else
    status=$?
  fi
  local retained plan_stderr
  retained="$(sed -n 's/^retaining failed golden workspace: //p' "$log")"
  plan_stderr="$retained/stage/nested/plan.stderr.txt"
  if [ "$status" -ne 2 ] || ! grep -Fq 'FATAL: plan failed for nested' "$log" \
    || [[ -z "$retained" || ! -f "$plan_stderr" ]] \
    || ! grep -Fq 'trusted qualification archive guard changed during candidate execution' "$plan_stderr"; then
    cat "$log" >&2
    [ -f "$plan_stderr" ] && cat "$plan_stderr" >&2
    echo "FAIL: mutated archive guard was not rejected (exit $status)" >&2
    exit 1
  fi
  if [ -e "$MARKER" ]; then
    echo "FAIL: archive guard mutation invoked Cargo build" >&2
    exit 1
  fi
  if [ ! -f "$mise_dir/metadata-succeeded" ] \
    || ! grep -Fq 'real-mise-status=0' "$mise_dir/receipt" \
    || ! grep -Fq 'mode=guard-mutation' "$mise_dir/receipt" \
    || ! grep -Fq 'fixture=nested' "$mise_dir/receipt" \
    || ! grep -Fq "guard-copy=$MUTATED_GUARD_PATH" "$mise_dir/receipt" \
    || ! grep -Fq "guard-sha256=$(test_file_sha256 "$MUTATED_GUARD_PATH")" "$mise_dir/receipt" \
    || [ "$(test_file_sha256 "$ARCHIVE_GUARD_INPUT_PATH")" != "$ARCHIVE_GUARD_INPUT_SHA256" ]; then
    echo "FAIL: archive guard mutation did not run real Cargo metadata" >&2
    exit 1
  fi
  local after
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: archive guard mutation changed goldens" >&2
    exit 1
  fi
  echo "passed continuous archive guard identity check"
}
GUARD_MUTATION_MISE_DIR="$(prepare_fault_mise guard-mutation "$MUTATED_GUARD_PATH")" || exit 2
expect_guard_mutation_rejected

expect_dogfood_guard_mutation_rejected() {
  local mutated="$WORK/guard test/dogfood mutable guard" digest status=0 log="$WORK/dogfood-guard-mutation.log"
  local mise_dir="$DOGFOOD_GUARD_MUTATION_MISE_DIR"
  cp "$ARCHIVE_GUARD_INPUT_PATH" "$mutated" || exit 2
  digest="$(test_file_sha256 "$mutated")" || exit 2
  if (
    cd "$WORK" || exit 2
    TMPDIR="$WORK/tmp workspace" PATH="$mise_dir:$STUB_BIN:$ORIGINAL_PATH" \
      VELNOR_TEST_CARGO_MARKER="$MARKER" \
      VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
      GITHUB_SHA="$SOURCE_SHA" GITHUB_REPOSITORY=tailrocks/velnor-new \
      run_collector --guard-args --archive-guard "$mutated" \
        --archive-guard-sha256 "$digest" -- check-release "$CLI_BIN" \
        "$WORK/tmp symlink/valid manifest.json" \
        "$(test_file_sha256 "$WORK/tmp symlink/valid manifest.json")"
  ) >"$log" 2>&1; then
    cat "$log" >&2
    echo "FAIL: release dogfood continued after archive guard mutation" >&2
    exit 1
  else
    status=$?
  fi
  if [ "$status" -ne 2 ] || [ ! -f "$mise_dir/metadata-succeeded" ] \
    || ! grep -Fq 'real-mise-status=0' "$mise_dir/receipt" \
    || ! grep -Fq 'mode=dogfood-guard-mutation' "$mise_dir/receipt" \
    || ! grep -Fq 'fixture=dogfood-repo' "$mise_dir/receipt" \
    || ! grep -Fq "guard-copy=$mutated" "$mise_dir/receipt" \
    || ! grep -Fq "guard-sha256=$(test_file_sha256 "$mutated")" "$mise_dir/receipt" \
    || [ "$(test_file_sha256 "$ARCHIVE_GUARD_INPUT_PATH")" != "$ARCHIVE_GUARD_INPUT_SHA256" ]; then
    cat "$log" >&2
    echo "FAIL: dogfood mutation did not run real Cargo metadata and fail closed (exit $status)" >&2
    exit 1
  fi
  local retained dogfood_stderr after
  retained="$(sed -n 's/^retaining failed golden workspace: //p' "$log")"
  dogfood_stderr="$retained/stage/dogfood/stderr.txt"
  if [[ -z "$retained" || ! -f "$dogfood_stderr" ]] \
    || ! grep -Fq 'trusted qualification archive guard changed during candidate execution' "$dogfood_stderr"; then
    cat "$log" >&2
    [ -f "$dogfood_stderr" ] && cat "$dogfood_stderr" >&2
    echo "FAIL: release dogfood did not report the post-execution guard mutation" >&2
    exit 1
  fi
  if [ -e "$MARKER" ]; then
    echo "FAIL: dogfood guard mutation invoked Cargo build" >&2
    exit 1
  fi
  after="$(golden_fingerprint)" || exit 2
  if [ "$after" != "$BEFORE" ]; then
    echo "FAIL: dogfood guard mutation changed goldens" >&2
    exit 1
  fi
  rm -rf "$retained"
  echo "passed final release dogfood candidate guard check"
}
DOGFOOD_GUARD_MUTATION_MISE_DIR="$(prepare_fault_mise dogfood-guard-mutation "$WORK/guard test/dogfood mutable guard")" || exit 2
expect_dogfood_guard_mutation_rejected
PARSER_SENTINEL_BIN="$WORK/parser sentinel candidate"
cat >"$PARSER_SENTINEL_BIN" <<'SENTINEL'
#!/bin/sh
printf invoked >"$VELNOR_TEST_PARSER_CANDIDATE_MARKER"
exit 0
SENTINEL
chmod u+x "$PARSER_SENTINEL_BIN"
expect_guard_options_rejected missing-guard-pair 'archive guard path and SHA-256 must be supplied together' \
  --archive-guard "$ARCHIVE_GUARD_INPUT_PATH"
expect_guard_options_rejected missing-guard-path 'archive guard path and SHA-256 must be supplied together' \
  --archive-guard-sha256 "$ARCHIVE_GUARD_INPUT_SHA256"
expect_guard_options_rejected duplicate-guard-pair 'archive guard options must appear once as a path and SHA-256 pair' \
  --archive-guard "$ARCHIVE_GUARD_INPUT_PATH" --archive-guard "$ARCHIVE_GUARD_INPUT_PATH" \
  --archive-guard-sha256 "$ARCHIVE_GUARD_INPUT_SHA256"
expect_guard_options_rejected empty-guard-path 'archive guard path and SHA-256 must be non-empty' \
  --archive-guard '' --archive-guard-sha256 "$ARCHIVE_GUARD_INPUT_SHA256"
expect_guard_options_rejected empty-guard-sha 'archive guard path and SHA-256 must be non-empty' \
  --archive-guard "$ARCHIVE_GUARD_INPUT_PATH" --archive-guard-sha256 ''
expect_guard_options_rejected both-empty-guard-pair 'archive guard path and SHA-256 must be non-empty' \
  --archive-guard '' --archive-guard-sha256 ''
expect_guard_options_rejected unknown-guard-option 'usage:' --archive-guard-unknown x

mkdir -p "$WORK/not a binary directory"
printf 'not executable\n' >"$WORK/not executable"
chmod 600 "$WORK/not executable"
expect_rejected invalid-mode 'usage:' unknown
expect_rejected extra-argument 'usage:' check "$CLI_BIN" extra
expect_rejected missing-bin 'explicit CLI binary must be a regular executable file' \
  capture "$WORK/missing binary"
expect_rejected non-executable-bin 'explicit CLI binary must be a regular executable file' \
  check "$WORK/not executable"
expect_rejected directory-bin 'explicit CLI binary must be a regular executable file' \
  check "$WORK/not a binary directory"

FAILING_BIN="$WORK/executable but failing"
cat >"$FAILING_BIN" <<FAILING
#!/bin/sh
printf invoked >"\$VELNOR_TEST_FAILING_BIN_MARKER"
if [ "\${1-}" = "--version" ]; then
  printf '%s\n' 'velnor-actions $CLI_VERSION'
  exit 0
fi
exit 57
FAILING
chmod u+x "$FAILING_BIN"
expect_unsupported_binary
expect_execution_failure

SPACED_BIN="$WORK/space path/velnor-actions"
mkdir -p "$(dirname "$SPACED_BIN")"
cp "$CLI_BIN" "$SPACED_BIN"
chmod u+x "$SPACED_BIN"
source_digest="$(test_file_sha256 "$CLI_BIN")"
copy_digest="$(test_file_sha256 "$SPACED_BIN")"
if [ "$source_digest" != "$copy_digest" ]; then
  echo "FAIL: spaced CLI copy differs from supplied binary" >&2
  exit 1
fi
WORKSPACES_BEFORE="$(find "$WORK/tmp workspace" -mindepth 1 -maxdepth 1 -type d \
  -name 'velnor-goldens-work.*' -print | LC_ALL=C sort)"
if (
  cd "$WORK" || exit 2
  TMPDIR="$WORK/tmp workspace" PATH="$STUB_BIN:$ORIGINAL_PATH" \
    VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    run_collector check "space path/velnor-actions"
) >"$WORK/spaced-path.log" 2>&1; then
  cat "$WORK/spaced-path.log"
else
  status=$?
  cat "$WORK/spaced-path.log" >&2
  echo "FAIL: spaced relative CLI check exited $status" >&2
  exit 1
fi
if [ -e "$MARKER" ]; then
  echo "FAIL: explicit CLI path invoked the default cargo build" >&2
  exit 1
fi
WORKSPACES_AFTER="$(find "$WORK/tmp workspace" -mindepth 1 -maxdepth 1 -type d \
  -name 'velnor-goldens-work.*' -print | LC_ALL=C sort)"
if [ "$WORKSPACES_AFTER" != "$WORKSPACES_BEFORE" ]; then
  echo "FAIL: successful check left behind a private golden workspace" >&2
  exit 1
fi
AFTER="$(golden_fingerprint)" || exit 2
if [ "$AFTER" != "$BEFORE" ]; then
  echo "FAIL: successful check changed goldens" >&2
  exit 1
fi
echo "passed explicit CLI path, relative path with spaces, TMPDIR with spaces, and no cargo fallback"
