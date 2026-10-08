#!/usr/bin/env bash
# Exercise local-cargo routing with fake Rustup and Cargo executables.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
HELPER="$ROOT/scripts/local-cargo.sh"
TEST_ROOT="$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/velnor-local-cargo-test.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

FAKE_BIN="$TEST_ROOT/bin"
FAKE_RUSTUP_HOME="$TEST_ROOT/rustup"
FAKE_TOOLCHAIN="$FAKE_RUSTUP_HOME/toolchains/1.98.1-test"
FAKE_TOOLCHAIN_199="$FAKE_RUSTUP_HOME/toolchains/1.99.0-test"
FAKE_MBX_BIN="$TEST_ROOT/mbx/bin"
TEST_TMPDIR="$TEST_ROOT/tmp"
LOG_FILE="$TEST_ROOT/cargo-target"
mkdir -p "$FAKE_BIN" "$FAKE_TOOLCHAIN/bin" "$FAKE_TOOLCHAIN_199/bin" \
  "$FAKE_MBX_BIN" "$TEST_TMPDIR" "$TEST_ROOT/cargo-home"
TEST_TMPDIR="$(cd "$TEST_TMPDIR" && pwd -P)"

cat > "$FAKE_BIN/rustup" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == show && "$2" == home ]]; then
  printf '%s\n' "$LOCAL_CARGO_TEST_RUSTUP_HOME"
  exit 0
fi
if [[ "$1" == which && "$2" == --toolchain && "$3" == "$LOCAL_CARGO_TEST_POLICY_RUST" ]]; then
  tool="$4"
  if [[ "$LOCAL_CARGO_TEST_BAD_SHIM" == 1 && "$tool" == cargo ]]; then
    printf '%s/cargo\n' "$LOCAL_CARGO_TEST_MBX_BIN"
  else
    printf '%s/bin/%s\n' "$LOCAL_CARGO_TEST_TOOLCHAIN" "$tool"
  fi
  exit 0
fi
echo "unexpected rustup call: $*" >&2
exit 2
EOF

cat > "$FAKE_TOOLCHAIN/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == --version ]]; then
  printf 'cargo %s (self-test)\n' "$LOCAL_CARGO_TEST_POLICY_RUST"
  exit 0
fi
[[ "$0" == "$LOCAL_CARGO_TEST_TOOLCHAIN/bin/cargo" ]]
[[ "$RUSTC" == "$LOCAL_CARGO_TEST_TOOLCHAIN/bin/rustc" ]]
[[ "$RUSTDOC" == "$LOCAL_CARGO_TEST_TOOLCHAIN/bin/rustdoc" ]]
[[ "$RUSTUP_TOOLCHAIN" == "$LOCAL_CARGO_TEST_POLICY_RUST" ]]
[[ "$CARGO_BUILD_JOBS" == 2 ]]
[[ "$PATH" == "$LOCAL_CARGO_TEST_TOOLCHAIN/bin:"* ]]
[[ "$(type -P cargo)" == "$LOCAL_CARGO_TEST_TOOLCHAIN/bin/cargo" ]]
case "$PATH" in
  *"$LOCAL_CARGO_TEST_MBX_BIN"*) echo 'MBX directory remains on PATH' >&2; exit 1 ;;
esac
[[ -d "$CARGO_TARGET_DIR" && ! -L "$CARGO_TARGET_DIR" ]]
case "$CARGO_TARGET_DIR" in
  "$LOCAL_CARGO_TEST_TMPDIR"/velnor-local-cargo-"$LOCAL_CARGO_TEST_NAMESPACE".*) ;;
  *) echo "target is not task-private: $CARGO_TARGET_DIR" >&2; exit 1 ;;
esac
[[ -z "${MBX_CARGO_SHIM_MODE+x}" ]]
[[ -z "${MBX_CACHE_DIR+x}" ]]
printf '%s\n' "$CARGO_TARGET_DIR" > "$LOCAL_CARGO_TEST_LOG"
EOF

cat > "$FAKE_TOOLCHAIN/bin/rustc" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == --version ]]; then printf 'rustc %s (self-test)\n' "$LOCAL_CARGO_TEST_POLICY_RUST"; exit 0; fi
exit 2
EOF

cat > "$FAKE_TOOLCHAIN/bin/rustdoc" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == --version ]]; then printf 'rustdoc %s (self-test)\n' "$LOCAL_CARGO_TEST_POLICY_RUST"; exit 0; fi
exit 2
EOF

cat > "$FAKE_MBX_BIN/cargo" <<'EOF'
#!/usr/bin/env bash
echo 'MBX shim was executed' > "$LOCAL_CARGO_TEST_MBX_SENTINEL"
exit 99
EOF
cat > "$FAKE_MBX_BIN/mbx" <<'EOF'
#!/usr/bin/env bash
echo 'MBX command was executed' > "$LOCAL_CARGO_TEST_MBX_SENTINEL"
exit 99
EOF

cat > "$FAKE_BIN/mktemp" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${LOCAL_CARGO_TEST_SYMLINK_TARGET:-0}" == 1 ]]; then
  real="$LOCAL_CARGO_TEST_TMPDIR/target-real"
  link="$LOCAL_CARGO_TEST_TMPDIR/velnor-local-cargo-$LOCAL_CARGO_TEST_NAMESPACE.link"
  mkdir -p "$real"
  ln -s "$real" "$link"
  printf '%s\n' "$link"
  exit 0
fi
exec /usr/bin/mktemp "$@"
EOF

cp "$FAKE_TOOLCHAIN/bin/cargo" "$FAKE_TOOLCHAIN_199/bin/cargo"
cp "$FAKE_TOOLCHAIN/bin/rustc" "$FAKE_TOOLCHAIN_199/bin/rustc"
cp "$FAKE_TOOLCHAIN/bin/rustdoc" "$FAKE_TOOLCHAIN_199/bin/rustdoc"
chmod +x "$FAKE_BIN/rustup" "$FAKE_BIN/mktemp" \
  "$FAKE_TOOLCHAIN/bin/cargo" "$FAKE_TOOLCHAIN/bin/rustc" \
  "$FAKE_TOOLCHAIN/bin/rustdoc" "$FAKE_TOOLCHAIN_199/bin/cargo" \
  "$FAKE_TOOLCHAIN_199/bin/rustc" "$FAKE_TOOLCHAIN_199/bin/rustdoc" \
  "$FAKE_MBX_BIN/cargo" "$FAKE_MBX_BIN/mbx"

export LOCAL_CARGO_TEST_RUSTUP_HOME="$FAKE_RUSTUP_HOME"
export LOCAL_CARGO_TEST_POLICY_RUST=1.98.1
export LOCAL_CARGO_TEST_TOOLCHAIN="$FAKE_TOOLCHAIN"
export LOCAL_CARGO_TEST_MBX_BIN="$FAKE_MBX_BIN"
export LOCAL_CARGO_TEST_MBX_SENTINEL="$TEST_ROOT/mbx-invoked"
export LOCAL_CARGO_TEST_TMPDIR="$TEST_TMPDIR"
export LOCAL_CARGO_TEST_LOG="$LOG_FILE"
export PATH="$FAKE_MBX_BIN:$FAKE_BIN:$PATH"
export TMPDIR="$TEST_TMPDIR"
export CARGO_HOME="$TEST_ROOT/cargo-home"
export CARGO_TARGET_DIR="$TEST_ROOT/shared-target"
export MBX_CARGO_SHIM_MODE=1
export MBX_CACHE_DIR="$TEST_ROOT/mbx-cache"

expect_failure() {
  local label="$1"
  shift
  if "$@" >"$TEST_ROOT/$label.out" 2>&1; then
    echo "FAIL: $label unexpectedly succeeded" >&2
    exit 1
  fi
}

expect_failure missing-namespace "$HELPER"
expect_failure invalid-namespace "$HELPER" '../shared' test
expect_failure cargo-routing-option "$HELPER" local-check test --config build.target-dir=/tmp/shared
expect_failure cargo-jobs-option "$HELPER" local-check test --jobs 8
[[ ! -f "$LOG_FILE" ]] || { echo 'FAIL: rejected inputs reached Cargo' >&2; exit 1; }

export LOCAL_CARGO_TEST_BAD_SHIM=0
export LOCAL_CARGO_TEST_SYMLINK_TARGET=0
export LOCAL_CARGO_TEST_NAMESPACE=private-test
"$HELPER" "$LOCAL_CARGO_TEST_NAMESPACE" test --locked -p example
TARGET_DIR="$(cat "$LOG_FILE")"
[[ ! -e "$TARGET_DIR" ]] || { echo 'FAIL: private target was not removed after the command' >&2; exit 1; }
[[ ! -e "$LOCAL_CARGO_TEST_MBX_SENTINEL" ]] || { echo 'FAIL: MBX shim ran' >&2; exit 1; }
rm "$LOG_FILE"

printf '[build]\nrustc-wrapper = "mbx"\n' > "$CARGO_HOME/config.toml"
expect_failure cargo-config-wrapper "$HELPER" local-check test --locked
rm "$CARGO_HOME/config.toml"
[[ ! -f "$LOG_FILE" ]] || { echo 'FAIL: Cargo ran despite a configured wrapper' >&2; exit 1; }

export LOCAL_CARGO_TEST_BAD_SHIM=1
expect_failure mbx-shim-path "$HELPER" local-check test --locked
[[ ! -e "$LOCAL_CARGO_TEST_MBX_SENTINEL" ]] || { echo 'FAIL: MBX shim path executed' >&2; exit 1; }

export LOCAL_CARGO_TEST_BAD_SHIM=0
export LOCAL_CARGO_TEST_SYMLINK_TARGET=1
expect_failure symlink-target "$HELPER" "$LOCAL_CARGO_TEST_NAMESPACE" test --locked
[[ ! -f "$LOG_FILE" ]] || { echo 'FAIL: Cargo ran with a symlinked target' >&2; exit 1; }
rm "$TEST_TMPDIR/velnor-local-cargo-$LOCAL_CARGO_TEST_NAMESPACE.link"

MISMATCH_REPO="$TEST_ROOT/policy-mismatch"
mkdir -p "$MISMATCH_REPO/scripts" "$MISMATCH_REPO/.velnor"
cp "$HELPER" "$MISMATCH_REPO/scripts/local-cargo.sh"
printf '[tools]\nrust = "1.98.1"\n' > "$MISMATCH_REPO/mise.toml"
printf '[tools]\nrust = "1.99.0"\n' > "$MISMATCH_REPO/.velnor/version-policy.toml"
export LOCAL_CARGO_TEST_POLICY_RUST=1.99.0
export LOCAL_CARGO_TEST_TOOLCHAIN="$FAKE_TOOLCHAIN_199"
export LOCAL_CARGO_TEST_NAMESPACE=policy-mismatch
export LOCAL_CARGO_TEST_SYMLINK_TARGET=0
MISMATCH_OUTPUT="$TEST_ROOT/policy-mismatch.out"
if ! "$MISMATCH_REPO/scripts/local-cargo.sh" "$LOCAL_CARGO_TEST_NAMESPACE" test --locked -p example >"$MISMATCH_OUTPUT" 2>&1; then
  cat "$MISMATCH_OUTPUT" >&2
  echo 'FAIL: local-cargo rejected the policy Rust pin when mise.toml differed' >&2
  exit 1
fi
/usr/bin/grep -F \
  'local-cargo: warning: mise.toml Rust 1.98.1 differs from Velnor policy Rust 1.99.0; using policy' \
  "$MISMATCH_OUTPUT" >/dev/null || { cat "$MISMATCH_OUTPUT" >&2; echo 'FAIL: policy mismatch warning missing' >&2; exit 1; }
TARGET_DIR="$(cat "$LOG_FILE")"
[[ ! -e "$TARGET_DIR" ]] || { echo 'FAIL: policy mismatch target was not removed' >&2; exit 1; }
[[ ! -e "$LOCAL_CARGO_TEST_MBX_SENTINEL" ]] || { echo 'FAIL: MBX shim ran during policy mismatch' >&2; exit 1; }

echo 'PASS: local-cargo routing, policy pin selection, warning, MBX, target, and cleanup checks'
