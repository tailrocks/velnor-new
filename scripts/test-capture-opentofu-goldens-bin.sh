#!/usr/bin/env bash
# Exercise the collector's explicit CLI path without changing checked-in goldens.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/generator-release/qualification-goldens.sh"
SCRIPT="$ROOT/scripts/capture-opentofu-goldens.sh"
GOLDEN_DIR="$ROOT/docs/proposed/opentofu-goldens"
CLI_ARG="${1:-}"
if [ -z "$CLI_ARG" ]; then
  echo "FATAL: provide the CLI binary path"
  exit 2
fi
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
MISE_TRACE="$WORK/mise-release-fetches"
SEED_TRACE="$WORK/cargo-cache-seeds"
ORIGINAL_PATH="$PATH"
REAL_MISE_BIN="$(command -v mise)"
if [[ "$REAL_MISE_BIN" != /* || ! -f "$REAL_MISE_BIN" || ! -x "$REAL_MISE_BIN" ]]; then
  echo "FAIL: the test stub needs an absolute path to the mise executable." >&2
  exit 2
fi
export VELNOR_TEST_REAL_MISE_BIN="$REAL_MISE_BIN"
printf '%s\n' "$REAL_MISE_BIN" >"$WORK/real-mise-path"
MISE_STATE_DIR="$WORK/mise-state"
export MISE_STATE_DIR
mkdir -p "$MISE_STATE_DIR"
RUST_POLICY="$(awk -v key=rust -f "$ROOT/scripts/toml-tool-pin.awk" \
  "$ROOT/.velnor/version-policy.toml")"
RUST_INSTALL_PATH="$(MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
  "$REAL_MISE_BIN" --no-config --no-env --no-hooks ls --json rust \
  | jq -er --arg version "$RUST_POLICY" \
    '[.[] | select(.version == $version and .installed == true) | .install_path] | if length == 1 then .[0] else error("missing or duplicate installed Rust pin") end')"
CANONICAL_RUSTUP_HOME="$(cd "${RUSTUP_HOME:-$HOME/.rustup}" && pwd -P)"
mkdir -p "$STUB_BIN" "$WORK/tmp workspace"
ln -s "$WORK/tmp workspace" "$WORK/tmp symlink"
# Only `cargo build` is the collector's implicit debug build. `plan` runs
# `cargo metadata` through the real toolchain; poisoning that call makes
# the spaced-path check fail before it can prove the collector skipped build.
cat >"$STUB_BIN/cargo" <<'STUB'
#!/bin/sh
case "${CARGO_HOME-}" in
  */.cache/velnor/archive-guard/cargo-home)
    # The child's trusted-guard recheck uses an isolated archive-guard Cargo
    # home. Let its build use real Cargo even though the candidate child has
    # the collector fallback stub at the front of PATH.
    stub_dir="${0%/*}"
    filtered_path=''
    old_ifs="$IFS"
    IFS=:
    set -f
    for path_entry in $PATH; do
      [ "$path_entry" = "$stub_dir" ] && continue
      if [ -n "$filtered_path" ]; then
        filtered_path="$filtered_path:$path_entry"
      else
        filtered_path="$path_entry"
      fi
    done
    set +f
    IFS="$old_ifs"
    PATH="$filtered_path"
    export PATH
    exec cargo "$@"
    ;;
esac
if [ "${1-}" = "build" ]; then
  printf invoked >"$VELNOR_TEST_CARGO_MARKER"
  exit 79
fi
PATH="$VELNOR_TEST_REAL_CARGO_PATH"
export PATH
exec cargo "$@"
STUB
chmod u+x "$STUB_BIN/cargo"
cat >"$STUB_BIN/mise" <<'STUB'
#!/bin/sh
# The candidate child rechecks the trusted guard. Its builder has a separate
# Cargo/Mise home; let that isolated bootstrap use the real pinned tools.
case "${MISE_DATA_DIR-}" in
  */.cache/velnor/archive-guard/mise-data)
    real_mise_bin="$(cat "${0%/*}/../real-mise-path")" || exit 87
    exec "$real_mise_bin" "$@"
    ;;
esac
case " $* " in
  *" install "*) echo "unexpected Mise install" >&2; exit 80 ;;
  *" cargo fetch --locked --manifest-path "*)
    if [ "${MISE_AUTO_INSTALL-}" != false ] || [ "${MISE_EXEC_AUTO_INSTALL-}" != false ]; then
      echo "release cargo fetch did not disable Mise auto-install" >&2
      exit 81
    fi
    case "${CARGO_HOME-}" in
      "${VELNOR_TEST_PRIVATE_TMPDIR-}"/velnor-goldens-work.*/stage/dogfood/cargo-home) ;;
      *) echo "release cargo fetch did not use its private Cargo cache" >&2; exit 82 ;;
    esac
    if [ "${CARGO_NET_OFFLINE-}" = true ]; then
      case "${VELNOR_TEST_CARGO_CACHE_HOME-}" in
        /*) ;;
        *) echo "offline release cargo fetch has no absolute registry cache source" >&2; exit 84 ;;
      esac
      for part in index cache; do
        if [ ! -d "$VELNOR_TEST_CARGO_CACHE_HOME/registry/$part" ]; then
          echo "offline release cargo fetch registry $part source is missing" >&2
          exit 85
        fi
        mkdir -p "$CARGO_HOME/registry"
        if [ ! -e "$CARGO_HOME/registry/$part" ]; then
          cp -RL "$VELNOR_TEST_CARGO_CACHE_HOME/registry/$part" "$CARGO_HOME/registry/$part"
        fi
      done
      if [ -e "$CARGO_HOME/config" ] || [ -e "$CARGO_HOME/config.toml" ]; then
        echo "offline release cargo cache unexpectedly copied Cargo configuration" >&2
        exit 86
      fi
      printf '%s\t%s\n' "$CARGO_HOME" "$VELNOR_TEST_CARGO_CACHE_HOME" \
        >>"$VELNOR_TEST_MISE_SEED_TRACE"
    fi
    if [ "${RUSTUP_HOME-}" != "${VELNOR_TEST_CANONICAL_RUSTUP_HOME-}" ]; then
      echo "release cargo fetch did not use the canonical Rustup home" >&2
      exit 83
    fi
    printf '%s\n' "$*" >>"$VELNOR_TEST_MISE_TRACE"
    ;;
esac
exec "$VELNOR_TEST_REAL_MISE_BIN" "$@"
STUB
chmod u+x "$STUB_BIN/mise"

if ! bash "$ROOT/scripts/generator-release/test-release-rust-toolchain.sh"; then
  echo "FAIL: guarded release Rust toolchain regression failed" >&2
  exit 1
fi

test_file_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  else
    shasum -a 256 "$1" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  fi
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

rust_install_snapshot() {
  python3 - "$RUST_INSTALL_PATH" <<'PYTHON'
import os
import stat
import sys

path = sys.argv[1]
info = os.lstat(path)
target = os.readlink(path) if stat.S_ISLNK(info.st_mode) else ""
print(f"{info.st_dev}:{info.st_ino}:{info.st_mode:o}:{target}")
PYTHON
}

BEFORE="$(golden_fingerprint)" || exit 2
SOURCE_SHA="$(git -C "$ROOT" rev-parse HEAD)" || exit 2
prepare_qualification_guard || exit 2
verify_qualification_guard || exit 2
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
  local before
  before="$(rust_install_snapshot)" || exit 2
  TMPDIR="$WORK/tmp symlink" GITHUB_SHA="$SOURCE_SHA" GITHUB_REPOSITORY=tailrocks/velnor-new \
    PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    VELNOR_TEST_MISE_TRACE="$MISE_TRACE" \
    VELNOR_TEST_PRIVATE_TMPDIR="$WORK/tmp workspace" \
    VELNOR_TEST_CARGO_CACHE_HOME="$(cd "${CARGO_HOME:-$HOME/.cargo}" && pwd -P)" \
    VELNOR_TEST_MISE_SEED_TRACE="$SEED_TRACE" \
    VELNOR_TEST_CANONICAL_RUSTUP_HOME="$CANONICAL_RUSTUP_HOME" \
    "$SCRIPT" check-release "$CLI_BIN" "$manifest" \
      "$(test_file_sha256 "$manifest")" >"$log" 2>&1 || status=$?
  if [ "$status" -ne 0 ] || ! grep -Fq 'ALL RELEASE FIXTURE GOLDENS AND DOGFOOD PARITY MATCH' "$log"; then
    cat "$log" >&2
    echo "FAIL: valid candidate did not match release fixture goldens (exit $status)" >&2
    exit 1
  fi
  if [ -e "$MARKER" ]; then echo "FAIL: release qualification invoked cargo build" >&2; exit 1; fi
  if [ "$(wc -l <"$MISE_TRACE" | tr -d ' ')" -ne 2 ]; then
    echo "FAIL: release qualification did not use guarded Mise for both lockfiles" >&2
    exit 1
  fi
  if [ "${CARGO_NET_OFFLINE-}" = true ]; then
    local expected_cache_home
    expected_cache_home="$(cd "${CARGO_HOME:-$HOME/.cargo}" && pwd -P)"
    if [ "$(wc -l <"$SEED_TRACE" | tr -d ' ')" -ne 2 ] || \
      ! awk -F '\t' -v prefix="$WORK/tmp workspace/velnor-goldens-work." \
        -v source="$expected_cache_home" \
        '$1 !~ ("^" prefix "[^/]+/stage/dogfood/cargo-home$") || $2 != source { bad=1 } END { exit bad }' \
        "$SEED_TRACE"; then
      echo "FAIL: offline release Cargo cache records do not match the two expected fetches." >&2
      exit 1
    fi
  fi
  after="$(rust_install_snapshot)" || exit 2
  if [ "$before" != "$after" ]; then
    echo "FAIL: release qualification changed the installed Rust alias" >&2
    exit 1
  fi
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
    "$SCRIPT" check-release "$CLI_BIN" "$manifest" "$digest" >"$log" 2>&1 || status=$?
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
    "$SCRIPT" check-release "$CLI_BIN" "$manifest" "$digest" >"$log" 2>&1 || status=$?
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
    "$SCRIPT" "$@" >"$WORK/$label.log" 2>&1 || status=$?
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
if [ "${VELNOR_TEST_QUALIFICATION_OUTPUT_ONLY:-0}" = 1 ]; then
  run_candidate_output_binding_tests || exit $?
  exit 0
fi
expect_release_check_match "$WORK/tmp symlink/valid manifest.json"
run_candidate_output_binding_tests || exit $?
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

expect_execution_failure() {
  local status=0 log="$WORK/failing-binary.log"
  if (
    cd "$WORK" || exit 2
    TMPDIR="$WORK/tmp workspace" PATH="$STUB_BIN:$ORIGINAL_PATH" \
      VELNOR_TEST_CARGO_MARKER="$MARKER" \
      VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
      VELNOR_TEST_FAILING_BIN_MARKER="$WORK/failing binary invoked" \
      "$SCRIPT" capture "$FAILING_BIN"
  ) >"$log" 2>&1; then
    cat "$log" >&2
    echo "FAIL: executable that exits nonzero unexpectedly succeeded" >&2
    exit 1
  else
    status=$?
  fi
  if [ "$status" -ne 2 ] || ! grep -Fq 'FATAL: plan failed for nested' "$log"; then
    cat "$log" >&2
    echo "FAIL: nonzero executable status was not reported (exit $status)" >&2
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
  echo "passed executed-failure rejection without cargo fallback or golden changes"
}

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
    "$SCRIPT" check "space path/velnor-actions"
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
