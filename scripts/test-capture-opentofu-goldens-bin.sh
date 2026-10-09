#!/usr/bin/env bash
# Exercise the collector's explicit CLI path without changing checked-in goldens.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
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
trap 'rm -rf "$WORK"' EXIT
STUB_BIN="$WORK/stub bin"
MARKER="$WORK/cargo-invoked"
ORIGINAL_PATH="$PATH"
mkdir -p "$STUB_BIN" "$WORK/tmp workspace"
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
exec cargo "$@"
STUB
chmod u+x "$STUB_BIN/cargo"

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
      find . -type f -print | LC_ALL=C sort | while IFS= read -r path; do
        digest="$(test_file_sha256 "$path")" || exit 2
        printf 'file %s %s\n' "$path" "$digest"
      done
      find . -type l -print | LC_ALL=C sort | while IFS= read -r path; do
        printf 'link %s -> %s\n' "$path" "$(readlink "$path")"
      done
    } | test_stdin_sha256
  )
}

BEFORE="$(golden_fingerprint)" || exit 2
SOURCE_SHA="$(git -C "$ROOT" rev-parse HEAD)" || exit 2
CLI_VERSION="$("$CLI_BIN" --version | awk 'NR == 1 && NF == 2 && $1 == "velnor-actions" { print $2; next } { exit 1 } END { if (NR != 1) exit 1 }')" || exit 2
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

source "$ROOT/scripts/generator-release/test-qualification-output-bindings.sh"

expect_release_check_match() {
  local status=0 log="$WORK/release-check.log" after
  GITHUB_SHA="$SOURCE_SHA" GITHUB_REPOSITORY=tailrocks/velnor-new \
    PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
    VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH" \
    "$SCRIPT" check-release "$CLI_BIN" "$WORK/valid manifest.json" \
      "$(test_file_sha256 "$WORK/valid manifest.json")" >"$log" 2>&1 || status=$?
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
expect_release_check_match
run_candidate_output_binding_tests || exit 1
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
