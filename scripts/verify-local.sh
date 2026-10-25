#!/usr/bin/env bash
set -uo pipefail

ROOT=""
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

create_log_dir() {
  local parent candidate fallback
  parent="${TMPDIR:-/tmp}"
  if [ ! -d "$parent" ] || [ ! -w "$parent" ] || [ ! -x "$parent" ]; then
    parent="/tmp"
  fi
  parent="$(cd -P -- "$parent" 2>/dev/null && pwd -P)" || parent=""
  if [ -z "$parent" ]; then
    parent="$(cd -P -- /tmp 2>/dev/null && pwd -P)" || return 1
  fi
  candidate="$( (umask 077; mktemp -d "$parent/velnor-verify-local.XXXXXXXX") 2>/dev/null)" || {
    fallback="$(cd -P -- /tmp 2>/dev/null && pwd -P)" || return 1
    [ "$parent" = "$fallback" ] && return 1
    candidate="$( (umask 077; mktemp -d "$fallback/velnor-verify-local.XXXXXXXX") 2>/dev/null)" || return 1
  }
  if [ ! -d "$candidate" ] || [ ! -O "$candidate" ]; then
    return 1
  fi
  candidate="$(cd -P -- "$candidate" 2>/dev/null && pwd -P)" || return 1
  if [ ! -d "$candidate" ] || [ ! -O "$candidate" ]; then
    return 1
  fi
  printf '%s\n' "$candidate"
}

LOG_DIR="$(create_log_dir)" || {
  echo "verify-local: FAIL: could not create a private log directory under TMPDIR or /tmp"
  exit 1
}
TOOLCHAIN_LOG="$LOG_DIR/toolchain.log"
MISE_PIN_LOG="$LOG_DIR/mise-pin.log"
MISE_POLICY_LOG="$LOG_DIR/mise-policy.log"
TOOLCHAIN_POLICY_LOG="$LOG_DIR/toolchain-policy.log"
GENERATED_ARTIFACTS="$LOG_DIR/generated-artifacts.jsonl"
GENERATED_BUILD_LOG="$LOG_DIR/generated-build.log"
GENERATED_RUN_LOG="$LOG_DIR/generated-run.log"
GENERATED_DIFF_LOG="$LOG_DIR/generated-diff.log"
CRATE_LIST_LOG="$LOG_DIR/crate-list.log"
DOCTEST_LIST_LOG="$LOG_DIR/doctest-list.log"
INTEGRATION_LOG="$LOG_DIR/integration.log"
echo "verify-local: log directory: $LOG_DIR"

FAILURES=""

fail() {
  echo "FAIL: $1"
  FAILURES="$FAILURES $1"
}

pass() {
  echo "ok: $1"
}

stage() {
  local name="$1"
  local log="$LOG_DIR/$1.log"
  shift
  echo "--- verify-local: $name"
  if "$@" >"$log" 2>&1; then
    pass "$name"
  else
    fail "$name (log: $log)"
  fi
}

if ! command -v mise >/dev/null 2>&1; then
  echo "verify-local: FAIL: mise not found on PATH (install mise, then re-run; logs: $LOG_DIR)"
  exit 1
fi

toml_tool_pin() {
  local file="$1" key="$2"
  awk -v key="$key" -f "$ROOT/scripts/toml-tool-pin.awk" "$file"
}

echo "--- verify-local: toolchain"
SPECS=""
RUST_LOCAL="$(toml_tool_pin mise.toml rust 2>/dev/null || true)"
MBX_LOCAL="$(toml_tool_pin mise.toml mr-boxington 2>/dev/null || true)"
NEXTEST_LOCAL="$(toml_tool_pin mise.toml aqua:nextest-rs/nextest/cargo-nextest 2>/dev/null || true)"
RUST_POLICY="$(toml_tool_pin .velnor/version-policy.toml rust 2>"$TOOLCHAIN_LOG")"
MBX_POLICY="$(toml_tool_pin .velnor/version-policy.toml mr-boxington 2>>"$TOOLCHAIN_LOG")"
NEXTEST_POLICY="$(toml_tool_pin .velnor/version-policy.toml nextest 2>>"$TOOLCHAIN_LOG")"
if [ -z "$RUST_POLICY" ] || [ -z "$MBX_POLICY" ] || [ -z "$NEXTEST_POLICY" ]; then
  cat "$TOOLCHAIN_LOG" >&2 || true
  echo "verify-local: FAIL: toolchain (missing policy pin; log: $TOOLCHAIN_LOG)"
  exit 1
fi
echo "local mise.toml tools: rust=${RUST_LOCAL:-<missing>} mr-boxington=${MBX_LOCAL:-<missing>} nextest=${NEXTEST_LOCAL:-<missing>}"
if [ "$RUST_LOCAL" != "$RUST_POLICY" ] || [ "$MBX_LOCAL" != "$MBX_POLICY" ] ||
   [ "$NEXTEST_LOCAL" != "$NEXTEST_POLICY" ]; then
  echo "WARNING: local mise.toml tools differ from Velnor CI policy; verify-local uses policy pins"
fi
SPECS="rust@$RUST_POLICY mr-boxington@$MBX_POLICY aqua:nextest-rs/nextest/cargo-nextest@$NEXTEST_POLICY"
echo "pinned specs: $SPECS"
# shellcheck disable=SC2206
_PIN_PARTS=($SPECS)
RUST_PIN="${_PIN_PARTS[0]#*@}"
MBX_PIN="${_PIN_PARTS[1]#*@}"
NEXTEST_PIN="${_PIN_PARTS[2]#*@}"
POLICY_MISE="$(toml_tool_pin .velnor/version-policy.toml mise 2>"$MISE_PIN_LOG")"
LOCAL_MISE="$(mise --version 2>/dev/null | awk "{print \$1}")"
echo "mise: local $LOCAL_MISE, policy $POLICY_MISE"
if [ "$LOCAL_MISE" != "$POLICY_MISE" ]; then
  echo "WARNING: local mise $LOCAL_MISE differs from policy $POLICY_MISE; continuing with local mise and pinned tools"
fi
# shellcheck disable=SC2206
SPEC_ARR=($SPECS)
if ! mise install "${SPEC_ARR[@]}" >>"$TOOLCHAIN_LOG" 2>&1; then
  fail "toolchain (mise install failed; log: $TOOLCHAIN_LOG)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
MISE_EXEC=(mise exec "${SPEC_ARR[@]}" --)
repo_policy() {
  local action="$1"
  env \
    VELNOR_INTERNAL_OP=repo-policy-v1 \
    VELNOR_REPO_POLICY_ACTION="$action" \
    VELNOR_REPO_POLICY_ROOT="$ROOT" \
    "${MISE_EXEC[@]}" cargo run --quiet --locked -p velnor-actions-cli \
      --bin velnor-actions
}
POLICY_SPECS="$(repo_policy toolchain-specs 2>"$TOOLCHAIN_POLICY_LOG")"
if [ "$POLICY_SPECS" != "$SPECS" ]; then
  fail "toolchain (Rust policy specs '$POLICY_SPECS' != bootstrap pins '$SPECS'; log: $TOOLCHAIN_POLICY_LOG)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
POLICY_MISE="$(repo_policy mise-version 2>"$MISE_POLICY_LOG")"
CARGO_VER="$("${MISE_EXEC[@]}" cargo --version 2>>"$TOOLCHAIN_LOG" | awk "{print \$2}")"
MBX_VER="$("${MISE_EXEC[@]}" mbx --version 2>>"$TOOLCHAIN_LOG" | awk "{print \$2}")"
echo "effective: cargo $CARGO_VER, mbx $MBX_VER"
if [ "$CARGO_VER" != "$RUST_PIN" ] || [ "$MBX_VER" != "$MBX_PIN" ]; then
  fail "toolchain (effective cargo $CARGO_VER / mbx $MBX_VER != pins $RUST_PIN / $MBX_PIN; log: $TOOLCHAIN_LOG)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
NEXTEST_RUN=()
NEXTEST_BIN="$(mise which cargo-nextest --tool "aqua:nextest-rs/nextest/cargo-nextest@$NEXTEST_PIN" 2>/dev/null || true)"
NEXTEST_BIN_VER="$("${MISE_EXEC[@]}" "${NEXTEST_BIN:-cargo-nextest}" --version 2>/dev/null | awk 'NR==1{print $2}')"
BARE_VER="$("${MISE_EXEC[@]}" cargo-nextest --version 2>/dev/null | awk 'NR==1{print $2}')"
SUB_VER="$("${MISE_EXEC[@]}" cargo nextest --version 2>/dev/null | awk 'NR==1{print $2}')"
if [ -n "$NEXTEST_BIN" ] && [ "$NEXTEST_BIN_VER" = "$NEXTEST_PIN" ]; then
  NEXTEST_RUN=("${MISE_EXEC[@]}" "$NEXTEST_BIN" nextest)
  echo "effective: nextest $NEXTEST_BIN_VER via mise which cargo-nextest"
elif [ "$BARE_VER" = "$NEXTEST_PIN" ]; then
  NEXTEST_RUN=("${MISE_EXEC[@]}" cargo-nextest nextest)
  echo "effective: nextest $BARE_VER via bare cargo-nextest"
elif [ "$SUB_VER" = "$NEXTEST_PIN" ]; then
  NEXTEST_RUN=("${MISE_EXEC[@]}" cargo nextest)
  echo "effective: nextest $SUB_VER via cargo nextest"
else
  echo "effective: no pinned nextest (bare $BARE_VER, subcommand $SUB_VER, pin $NEXTEST_PIN); nextest is required"
  fail "toolchain (pinned nextest missing)"
fi
pass "toolchain"

stage fmt "${MISE_EXEC[@]}" cargo fmt --all --check

RUNNER_MANIFEST="crates/velnor-runner/Cargo.toml"
if [ -f "$RUNNER_MANIFEST" ]; then
  stage runner-fmt "${MISE_EXEC[@]}" cargo fmt --manifest-path "$RUNNER_MANIFEST" --all -- --check
  stage runner-clippy "${MISE_EXEC[@]}" cargo clippy --manifest-path "$RUNNER_MANIFEST" --locked --workspace --all-targets -- -D warnings
  stage runner-nextest "${NEXTEST_RUN[@]}" run --manifest-path "$RUNNER_MANIFEST" --locked --workspace --no-tests warn
  stage runner-doctest "${MISE_EXEC[@]}" cargo test --manifest-path "$RUNNER_MANIFEST" --locked --workspace --doc
  stage runner-deny mise exec "cargo-deny@0.20.2" -- cargo-deny --locked --manifest-path "$RUNNER_MANIFEST" --config "crates/velnor-runner/deny.toml" check
fi

stage repo-policy "${MISE_EXEC[@]}" bash scripts/check-freshness.sh

GEN_DIR=""
GEN_DIR="$(mktemp -d 2>/dev/null)"
if [ -z "$GEN_DIR" ]; then
  fail "generated-tree (mktemp failed; logs: $LOG_DIR)"
else
  echo "--- verify-local: generated-tree"
  if "${MISE_EXEC[@]}" cargo build --locked --message-format=json-render-diagnostics \
    -p velnor-actions-cli --bin velnor-actions \
    >"$GENERATED_ARTIFACTS" \
    2>"$GENERATED_BUILD_LOG"; then
    BIN="$(VELNOR_REPO_POLICY_ACTION=artifact-executable \
      VELNOR_REPO_POLICY_MESSAGES="$GENERATED_ARTIFACTS" \
      VELNOR_REPO_POLICY_MANIFEST=crates/velnor-actions-cli/Cargo.toml \
      VELNOR_REPO_POLICY_TARGET=velnor-actions \
      repo_policy artifact-executable 2>>"$GENERATED_BUILD_LOG")"
    : >"$GENERATED_RUN_LOG"
    : >"$GENERATED_DIFF_LOG"
    if [ -n "$BIN" ] && "$BIN" generate --output-dir "$GEN_DIR/tree" \
      >"$GENERATED_RUN_LOG" 2>&1 &&
      diff -r --brief .github "$GEN_DIR/tree/.github" \
        >"$GENERATED_DIFF_LOG" 2>&1; then
      pass "generated-tree"
    else
      fail "generated-tree (logs: $GENERATED_BUILD_LOG, $GENERATED_ARTIFACTS, $GENERATED_RUN_LOG, $GENERATED_DIFF_LOG)"
    fi
  else
    fail "generated-tree (build failed: log $GENERATED_BUILD_LOG, artifacts $GENERATED_ARTIFACTS)"
  fi
  rm -rf "$GEN_DIR"
fi

MEMBERS=""
MEMBERS="$(repo_policy workspace-members 2>"$CRATE_LIST_LOG")"
if [ -z "$MEMBERS" ]; then
  fail "crate-list (log: $CRATE_LIST_LOG)"
else
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "clippy-$safe" "${MISE_EXEC[@]}" cargo clippy --locked -p "$member" --all-targets -- -D warnings
  done
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "test-$safe" "${NEXTEST_RUN[@]}" run --locked -p "$member" --no-tests warn
  done
  LIB_MEMBERS=""
  LIB_MEMBERS="$(repo_policy library-members 2>"$DOCTEST_LIST_LOG")"
  if [ -z "$LIB_MEMBERS" ]; then
    fail "doctest-list (log: $DOCTEST_LIST_LOG)"
  else
    for member in $MEMBERS; do
      safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
      case " $LIB_MEMBERS " in
        *" $member "*)
          stage "doctest-$safe" "${MISE_EXEC[@]}" cargo test --locked -p "$member" --doc
          ;;
        *)
          echo "skip: doctest-$safe (no library targets)"
          ;;
      esac
    done
  fi
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "doc-$safe" "${MISE_EXEC[@]}" cargo doc --locked --offline -p "$member" --no-deps
  done
fi

echo "--- verify-local: integration"
if "${NEXTEST_RUN[@]}" run --workspace --locked --profile ci --no-tests warn \
  >"$INTEGRATION_LOG" 2>&1; then
  pass "integration (nextest ci)"
else
  fail "integration (log: $INTEGRATION_LOG)"
fi

if [ -z "$FAILURES" ]; then
  echo "verify-local: PASS"
  exit 0
else
  echo "verify-local: FAIL:$FAILURES"
  echo "verify-local: logs retained in $LOG_DIR"
  exit 1
fi
