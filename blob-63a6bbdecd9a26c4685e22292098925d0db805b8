#!/usr/bin/env bash
# Run one local Cargo command with the Velnor policy Rust pin and a private target.
set -euo pipefail

usage() {
  echo "usage: scripts/local-cargo.sh TASK_NAMESPACE {build|check|clippy|test|doc|fmt|run|bench} [cargo-args...]" >&2
}

fail() {
  echo "local-cargo: $*" >&2
  exit 1
}

if [[ $# -lt 2 ]]; then
  usage
  exit 2
fi

TASK_NAMESPACE="$1"
shift
if [[ ! "$TASK_NAMESPACE" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]]; then
  fail "task namespace must use 1-64 letters, digits, dots, underscores, or hyphens"
fi

CARGO_COMMAND="$1"
shift
case "$CARGO_COMMAND" in
  build | check | clippy | test | doc | fmt | run | bench) ;;
  *)
    usage
    fail "unsupported Cargo command: $CARGO_COMMAND"
    ;;
esac

before_separator=1
for arg in "$@"; do
  if [[ "$arg" == "--" ]]; then
    before_separator=0
    continue
  fi
  if [[ "$before_separator" == 1 ]]; then
    case "$arg" in
      --config | --config=* | --target-dir | --target-dir=* | --jobs | --jobs=* | -j | -j[0-9]*)
        fail "Cargo routing and job overrides are not allowed: $arg"
        ;;
    esac
  fi
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd -P)"

read_rust_pin() {
  awk -F '"' '
    /^\[tools\]$/ { in_tools = 1; next }
    /^\[/ { in_tools = 0 }
    in_tools && /^[[:space:]]*rust[[:space:]]*=/ { print $2; found = 1; exit }
    END { if (!found) exit 1 }
  ' "$1"
}

LOCAL_RUST_PIN="$(read_rust_pin "$ROOT/mise.toml")" || LOCAL_RUST_PIN="<missing>"
POLICY_RUST_PIN="$(read_rust_pin "$ROOT/.velnor/version-policy.toml")" || fail "version policy has no exact Rust pin"
if [[ ! "$POLICY_RUST_PIN" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  fail "version policy Rust pin is not an exact version: $POLICY_RUST_PIN"
fi
RUST_VERSION="$POLICY_RUST_PIN"
if [[ "$LOCAL_RUST_PIN" != "$POLICY_RUST_PIN" ]]; then
  echo "local-cargo: warning: mise.toml Rust $LOCAL_RUST_PIN differs from Velnor policy Rust $POLICY_RUST_PIN; using policy" >&2
fi

physical_path() {
  python3 -c 'import os, sys; print(os.path.realpath(os.path.abspath(sys.argv[1])))' "$1"
}

path_is_within() {
  local child="$1"
  local parent="$2"
  if [[ "$parent" == / ]]; then
    return 0
  fi
  case "$child/" in
    "$parent/"*) return 0 ;;
    *) return 1 ;;
  esac
}

is_disjoint_from_mbx() {
  local candidate="$1"
  local label="$2"
  local mbx_root
  for mbx_root in "${MBX_ROOTS[@]}"; do
    [[ -n "$mbx_root" ]] || continue
    mbx_root="$(physical_path "$mbx_root")" || fail "cannot resolve MBX path: $mbx_root"
    if path_is_within "$candidate" "$mbx_root" || path_is_within "$mbx_root" "$candidate"; then
      fail "$label overlaps an MBX cache path: $candidate"
    fi
  done
}

AMBIENT_MBX_CACHE="${MBX_CACHE_DIR:-}"
MBX_ROOTS=()
if [[ -n "$AMBIENT_MBX_CACHE" ]]; then
  MBX_ROOTS+=("$AMBIENT_MBX_CACHE")
fi
MBX_ROOTS+=("$HOME/Library/Caches/mbx" "${XDG_CACHE_HOME:-$HOME/.cache}/mbx")

RUSTUP_PATH="$(type -P rustup || true)"
[[ -n "$RUSTUP_PATH" && -x "$RUSTUP_PATH" ]] || fail "rustup is required to resolve the pinned toolchain"
RUSTUP_BIN="$(physical_path "$RUSTUP_PATH")" || fail "cannot resolve rustup executable"
[[ "${RUSTUP_BIN##*/}" == rustup ]] || fail "rustup resolution is not a rustup executable: $RUSTUP_BIN"
RUSTUP_HOME_VALUE="$("$RUSTUP_BIN" show home)" || fail "rustup could not report its home"
RUSTUP_HOME_PHYSICAL="$(physical_path "$RUSTUP_HOME_VALUE")" || fail "cannot resolve rustup home"
case "$RUSTUP_BIN" in
  */Application\ Support/mbx/* | */mbx/bin/*) fail "rustup resolved inside the MBX shim tree: $RUSTUP_BIN" ;;
esac

resolve_tool() {
  "$RUSTUP_BIN" which --toolchain "$RUST_VERSION" "$1"
}

CARGO_PATH="$(resolve_tool cargo)" || fail "Rust $RUST_VERSION Cargo is not installed"
RUSTC_PATH="$(resolve_tool rustc)" || fail "Rust $RUST_VERSION rustc is not installed"
RUSTDOC_PATH="$(resolve_tool rustdoc)" || fail "Rust $RUST_VERSION rustdoc is not installed"
CARGO_BIN="$(physical_path "$CARGO_PATH")" || fail "cannot resolve pinned Cargo"
RUSTC_BIN="$(physical_path "$RUSTC_PATH")" || fail "cannot resolve pinned rustc"
RUSTDOC_BIN="$(physical_path "$RUSTDOC_PATH")" || fail "cannot resolve pinned rustdoc"
TOOLCHAIN_BIN="${CARGO_BIN%/*}"
case "$CARGO_BIN" in
  "$RUSTUP_HOME_PHYSICAL"/toolchains/"$RUST_VERSION"-*/bin/cargo) ;;
  *) fail "Cargo is outside the physical Rust $RUST_VERSION toolchain: $CARGO_BIN" ;;
esac
if [[ "$RUSTC_BIN" != "$TOOLCHAIN_BIN/rustc" || "$RUSTDOC_BIN" != "$TOOLCHAIN_BIN/rustdoc" ]]; then
  fail "Cargo, rustc, and rustdoc do not resolve to one physical Rust toolchain"
fi
for executable in "$CARGO_BIN" "$RUSTC_BIN" "$RUSTDOC_BIN"; do
  [[ -x "$executable" ]] || fail "pinned tool is not executable: $executable"
done

CARGO_VERSION_OUTPUT="$("$CARGO_BIN" --version)" || fail "pinned Cargo did not report its version"
RUSTC_VERSION_OUTPUT="$("$RUSTC_BIN" --version)" || fail "pinned rustc did not report its version"
RUSTDOC_VERSION_OUTPUT="$("$RUSTDOC_BIN" --version)" || fail "pinned rustdoc did not report its version"
[[ "$CARGO_VERSION_OUTPUT" == "cargo $RUST_VERSION "* ]] || fail "unexpected Cargo version: $CARGO_VERSION_OUTPUT"
[[ "$RUSTC_VERSION_OUTPUT" == "rustc $RUST_VERSION "* ]] || fail "unexpected rustc version: $RUSTC_VERSION_OUTPUT"
[[ "$RUSTDOC_VERSION_OUTPUT" == "rustdoc $RUST_VERSION "* ]] || fail "unexpected rustdoc version: $RUSTDOC_VERSION_OUTPUT"

CARGO_HOME_VALUE="${CARGO_HOME:-$HOME/.cargo}"
CARGO_HOME_PHYSICAL="$(physical_path "$CARGO_HOME_VALUE")" || fail "cannot resolve Cargo home"
is_disjoint_from_mbx "$CARGO_HOME_PHYSICAL" "Cargo home"
is_disjoint_from_mbx "$RUSTUP_HOME_PHYSICAL" "Rustup home"

check_config_file() {
  local config="$1"
  [[ -f "$config" ]] || return 0
  if awk '
    /^[[:space:]]*#/ { next }
    /(^|[^[:alnum:]_-])("?target-dir"?|"?rustc-wrapper"?|"?rustc-workspace-wrapper"?)[[:space:]]*=/ { found = 1 }
    /(^|[^[:alnum:]_-])(CARGO_TARGET_DIR|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER)[[:space:]]*=/ { found = 1 }
    /(^|[^[:alnum:]_-])MBX_[A-Za-z0-9_]*[[:space:]]*=/ { found = 1 }
    END { exit !found }
  ' "$config"; then
    fail "Cargo config can redirect compiler or target state: $config"
  fi
}

CARGO_HOME_CONFIG="$(physical_path "$CARGO_HOME_VALUE")"
check_config_file "$CARGO_HOME_CONFIG/config.toml"
check_config_file "$CARGO_HOME_CONFIG/config"
CONFIG_DIR="$ROOT"
while :; do
  check_config_file "$CONFIG_DIR/.cargo/config.toml"
  check_config_file "$CONFIG_DIR/.cargo/config"
  [[ "$CONFIG_DIR" == / ]] && break
  CONFIG_DIR="${CONFIG_DIR%/*}"
  [[ -n "$CONFIG_DIR" ]] || CONFIG_DIR=/
done

TEMP_INPUT="${TMPDIR:-/tmp}"
[[ -d "$TEMP_INPUT" ]] || fail "temporary directory does not exist: $TEMP_INPUT"
TEMP_ROOT="$(physical_path "$TEMP_INPUT")" || fail "cannot resolve temporary directory"
[[ -d "$TEMP_ROOT" && ! -L "$TEMP_ROOT" ]] || fail "temporary root is not a physical directory"
is_disjoint_from_mbx "$TEMP_ROOT" "temporary root"

TARGET_DIR=""
OWNER_MARKER=""
OWNER_TOKEN=""
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$TARGET_DIR" ]]; then
    if [[ -L "$TARGET_DIR" || ! -d "$TARGET_DIR" ]]; then
      echo "local-cargo: preserve target; its path is no longer a physical directory: $TARGET_DIR" >&2
      [[ "$status" != 0 ]] || status=1
    elif [[ -f "$OWNER_MARKER" && ! -L "$OWNER_MARKER" && "$(cat "$OWNER_MARKER" 2>/dev/null || true)" == "$OWNER_TOKEN" && "$(physical_path "$TARGET_DIR" 2>/dev/null || true)" == "$TARGET_DIR" ]]; then
      if rm -rf "$TARGET_DIR"; then
        echo "local-cargo: removed only this invocation's target directory" >&2
      else
        echo "local-cargo: could not remove owned target directory: $TARGET_DIR" >&2
        [[ "$status" != 0 ]] || status=1
      fi
    else
      echo "local-cargo: preserve target; ownership proof is missing or changed: $TARGET_DIR" >&2
      [[ "$status" != 0 ]] || status=1
    fi
  fi
  exit "$status"
}

TARGET_DIR="$(mktemp -d "$TEMP_ROOT/velnor-local-cargo-$TASK_NAMESPACE.XXXXXX")" || fail "could not create a private target directory"
[[ -d "$TARGET_DIR" && ! -L "$TARGET_DIR" && -O "$TARGET_DIR" ]] || fail "mktemp did not create an owned physical directory"
TARGET_DIR="$(physical_path "$TARGET_DIR")" || fail "cannot resolve private target directory"
case "$TARGET_DIR" in
  "$TEMP_ROOT"/velnor-local-cargo-"$TASK_NAMESPACE".*) ;;
  *) fail "private target escaped the temporary root: $TARGET_DIR" ;;
esac
is_disjoint_from_mbx "$TARGET_DIR" "Cargo target"
OWNER_MARKER="$TARGET_DIR/.velnor-local-cargo-owner"
OWNER_TOKEN="$TASK_NAMESPACE:$RUST_VERSION:$$:$TARGET_DIR"
(umask 077; printf '%s\n' "$OWNER_TOKEN" > "$OWNER_MARKER") || fail "cannot mark private target ownership"
trap cleanup EXIT

remove_path_entry() {
  local source_path="$1"
  local removed_entry="$2"
  local remaining="$source_path"
  local entry
  local filtered=""
  while [[ "$remaining" == *:* ]]; do
    entry="${remaining%%:*}"
    remaining="${remaining#*:}"
    if [[ -n "$entry" && "$entry" != "$removed_entry" ]]; then
      [[ -n "$filtered" ]] && filtered="$filtered:"
      filtered="$filtered$entry"
    fi
  done
  entry="$remaining"
  if [[ -n "$entry" && "$entry" != "$removed_entry" ]]; then
    [[ -n "$filtered" ]] && filtered="$filtered:"
    filtered="$filtered$entry"
  fi
  printf '%s' "$filtered"
}

filter_ambient_wrapper() {
  local executable="$1"
  local resolved="$executable"
  local directory
  [[ -n "$executable" ]] || return 0
  resolved="$(physical_path "$executable" 2>/dev/null || printf '%s' "$executable")"
  case "$executable:$resolved" in
    */Application\ Support/mbx/bin/* | */mbx/bin/* | */mr-boxington/*)
      directory="${executable%/*}"
      PATH="$(remove_path_entry "$PATH" "$directory")"
      ;;
  esac
}

AMBIENT_CARGO="$(type -P cargo || true)"
AMBIENT_MBX="$(type -P mbx || true)"
filter_ambient_wrapper "$AMBIENT_CARGO"
filter_ambient_wrapper "$AMBIENT_MBX"
export CARGO_HOME="$CARGO_HOME_PHYSICAL"
export RUSTUP_HOME="$RUSTUP_HOME_PHYSICAL"
export RUSTUP_TOOLCHAIN="$RUST_VERSION"
export CARGO_TARGET_DIR="$TARGET_DIR"
export CARGO_BUILD_JOBS=2
export RUSTC="$RUSTC_BIN"
export RUSTDOC="$RUSTDOC_BIN"
unset RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER MBX_CARGO_SHIM_MODE MBX_CACHE_DIR MBX_TARGET_DIR
while IFS='=' read -r variable _; do
  case "$variable" in
    MBX_*) unset "$variable" ;;
  esac
done < <(env)
export PATH="$TOOLCHAIN_BIN:$PATH"

echo "local-cargo: namespace=$TASK_NAMESPACE" >&2
echo "local-cargo: cargo=$CARGO_BIN ($CARGO_VERSION_OUTPUT)" >&2
echo "local-cargo: rustc=$RUSTC_BIN ($RUSTC_VERSION_OUTPUT)" >&2
echo "local-cargo: rustdoc=$RUSTDOC_BIN ($RUSTDOC_VERSION_OUTPUT)" >&2
echo "local-cargo: target=$CARGO_TARGET_DIR jobs=$CARGO_BUILD_JOBS" >&2
df -h "$TEMP_ROOT" >&2

"$CARGO_BIN" "$CARGO_COMMAND" "$@"
