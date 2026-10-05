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
cat >"$STUB_BIN/cargo" <<'STUB'
#!/bin/sh
printf invoked >"$VELNOR_TEST_CARGO_MARKER"
exit 79
STUB
chmod u+x "$STUB_BIN/cargo"

golden_fingerprint() {
  (
    cd "$GOLDEN_DIR" || exit 2
    {
      find . -type f -print | LC_ALL=C sort | while IFS= read -r path; do
        printf 'file %s ' "$path"
        sha256sum "$path"
      done
      find . -type l -print | LC_ALL=C sort | while IFS= read -r path; do
        printf 'link %s -> %s\n' "$path" "$(readlink "$path")"
      done
    } | sha256sum | awk '{print $1}'
  )
}

BEFORE="$(golden_fingerprint)" || exit 2
expect_rejected() {
  local label="$1" expected="$2" status=0
  shift 2
  PATH="$STUB_BIN:$ORIGINAL_PATH" VELNOR_TEST_CARGO_MARKER="$MARKER" \
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

expect_execution_failure() {
  local status=0 log="$WORK/failing-binary.log"
  if (
    cd "$WORK" || exit 2
    TMPDIR="$WORK/tmp workspace" PATH="$STUB_BIN:$ORIGINAL_PATH" \
      VELNOR_TEST_CARGO_MARKER="$MARKER" \
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
cat >"$FAILING_BIN" <<'FAILING'
#!/bin/sh
printf invoked >"$VELNOR_TEST_FAILING_BIN_MARKER"
exit 57
FAILING
chmod u+x "$FAILING_BIN"
expect_execution_failure

SPACED_BIN="$WORK/space path/velnor-actions"
mkdir -p "$(dirname "$SPACED_BIN")"
cp "$CLI_BIN" "$SPACED_BIN"
chmod u+x "$SPACED_BIN"
source_digest="$(sha256sum "$CLI_BIN" | awk '{print $1}')"
copy_digest="$(sha256sum "$SPACED_BIN" | awk '{print $1}')"
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
