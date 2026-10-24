#!/usr/bin/env bash
# Local verification entrypoint: the offline subset of CI in one command.
#
# The script stops after toolchain errors. It reports later stage errors.
#   toolchain       resolve the exact CI toolchain from
#                   .velnor/version-policy.toml, report local mise.toml
#                   tool versions for context, install policy specs, and
#                   record effective tool paths and versions
#   root-deny       pinned cargo-deny audit of the root workspace
#   fmt             pinned `cargo fmt --all --check`
#   repo-policy       pinned Rust freshness operation (pins, policy mirror,
#                     upstream evidence, and deny policy)
#   generated-selector focused regression tests for Cargo artifact selection
#   generated-tree    build the CLI, select the exact executable Cargo reports,
#                     `generate --output-dir` to a temp dir, and `diff -r` the
#                     committed `.github` tree against that generated tree.
#   clippy-<crate>    per-crate pinned MBX strict Clippy check
#   test-<crate>      per-crate pinned Nextest unit and integration tests
#   doctest-<crate>   per-crate pinned MBX doctests for library targets
#                     (mirrors the CI Doctests step; binary-only
#                     crates have no doctests and are reported skipped)
#   doc-<crate>       per-crate pinned MBX documentation build (mirrors the CI
#                     Documentation step: rustdoc warnings fail the build)
#   fixtures          fixture-consuming integration tests by name (currently
#                     the f2-evasion fixture suite), proving the checked-in
#                     `tests/fixtures` tree is exercised and green
#   integration       whole-workspace pass with the pinned Nextest `ci` profile
#
# Rust commands run under `mise --no-config exec` with exact tool specs.
# The script uses the selected Rust toolchain's `bin` directory first.
# Specs come from .velnor/version-policy.toml; mise.toml holds
# local developer selections and may differ without changing the CI pins.
# It does not use project wrappers or other Rust tools on PATH.
# It uses MBX for builds, tests, docs, and strict Clippy checks.
# Nextest runs ordinary suites. MBX `test --doc` runs doctests separately.
# Build commands use `--locked`.
# script honors `CARGO_TARGET_DIR` from the environment. Exit status is 0
# only when every stage passes; the failing stage names print at the end.
#
# Usage: scripts/verify-local.sh
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
GENERATED_SELECTOR_LOG="$LOG_DIR/generated-selector.log"
GENERATED_ARTIFACTS="$LOG_DIR/generated-artifacts.jsonl"
GENERATED_BUILD_LOG="$LOG_DIR/generated-build.log"
GENERATED_RUN_LOG="$LOG_DIR/generated-run.log"
GENERATED_DIFF_LOG="$LOG_DIR/generated-diff.log"
CRATE_LIST_LOG="$LOG_DIR/crate-list.log"
DOCTEST_LIST_LOG="$LOG_DIR/doctest-list.log"
FIXTURES_LOG="$LOG_DIR/fixtures.log"
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

prepare_nextest_dispatcher() {
  local dispatcher="$1"
  cat >"$dispatcher" <<'DISPATCHER' || return 1
#!/bin/sh
case "${1-}" in
  nextest) exec "$NEXTEST_BIN" "$@" ;;
  *) exec "$CARGO_TOOL_PATH" "$@" ;;
esac
DISPATCHER
  chmod 700 "$dispatcher"
}

if ! command -v mise >/dev/null 2>&1; then
  echo "verify-local: FAIL: mise not found on PATH (install mise, then re-run; logs: $LOG_DIR)"
  exit 1
fi

toml_tool_pin() {
  local file="$1" key="$2"
  awk -v key="$key" -f "$ROOT/scripts/toml-tool-pin.awk" "$file"
}

# --- pinned toolchain ----------------------------------------------------------
# Velnor's CI toolchain is owned by version policy and the compiled catalog.
# mise.toml remains a local developer selection, so report its values but do
# not rewrite them or require them to equal the Velnor-owned CI pins.
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
# Order is fixed by the pairs list above: rust, mr-boxington, nextest.
# shellcheck disable=SC2206
_PIN_PARTS=($SPECS)
RUST_PIN="${_PIN_PARTS[0]#*@}"
MBX_PIN="${_PIN_PARTS[1]#*@}"
NEXTEST_PIN="${_PIN_PARTS[2]#*@}"
CARGO_DENY_PIN="0.20.2"
POLICY_MISE="$(toml_tool_pin .velnor/version-policy.toml mise 2>"$MISE_PIN_LOG")"
LOCAL_MISE="$(mise --version 2>/dev/null | awk "{print \$1}")"
echo "mise: local $LOCAL_MISE, policy $POLICY_MISE"
if [ "$LOCAL_MISE" != "$POLICY_MISE" ]; then
  echo "WARNING: local mise $LOCAL_MISE differs from policy $POLICY_MISE; continuing with local mise and pinned tools"
fi
# shellcheck disable=SC2206
SPEC_ARR=($SPECS)
if ! mise --no-config install "${SPEC_ARR[@]}" >>"$TOOLCHAIN_LOG" 2>&1; then
  fail "toolchain (mise install failed; log: $TOOLCHAIN_LOG)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
if ! mise --no-config install "cargo-deny@$CARGO_DENY_PIN" >>"$TOOLCHAIN_LOG" 2>&1; then
  fail "toolchain (cargo-deny $CARGO_DENY_PIN install failed; log: $TOOLCHAIN_LOG)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
MISE_EXEC=(mise --no-config exec "${SPEC_ARR[@]}" --)
RUSTC_TOOL_PATH="$("${MISE_EXEC[@]}" rustup which --toolchain "$RUST_PIN" rustc 2>>"$TOOLCHAIN_LOG")"
CARGO_TOOL_PATH="$("${MISE_EXEC[@]}" rustup which --toolchain "$RUST_PIN" cargo 2>>"$TOOLCHAIN_LOG")"
CLIPPY_TOOL_PATH="$("${MISE_EXEC[@]}" rustup which --toolchain "$RUST_PIN" cargo-clippy 2>>"$TOOLCHAIN_LOG")"
CARGO_FMT_TOOL_PATH="$("${MISE_EXEC[@]}" rustup which --toolchain "$RUST_PIN" cargo-fmt 2>>"$TOOLCHAIN_LOG")"
RUSTFMT_TOOL_PATH="$("${MISE_EXEC[@]}" rustup which --toolchain "$RUST_PIN" rustfmt 2>>"$TOOLCHAIN_LOG")"
CARGO_DENY_BIN="$(mise --no-config which cargo-deny --tool "cargo-deny@$CARGO_DENY_PIN" 2>>"$TOOLCHAIN_LOG")"
if [ -z "$RUSTC_TOOL_PATH" ] || [ -z "$CARGO_TOOL_PATH" ] ||
   [ -z "$CLIPPY_TOOL_PATH" ] || [ -z "$CARGO_FMT_TOOL_PATH" ] ||
   [ -z "$RUSTFMT_TOOL_PATH" ]; then
  fail "toolchain (pinned Rust component missing; log: $TOOLCHAIN_LOG)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
RUST_TOOLCHAIN_BIN="$(dirname "$RUSTC_TOOL_PATH")"
# The child shell expands these values after it adds the toolchain path from
# the comparison. The parent does not expand them when it makes the command array.
# shellcheck disable=SC2016
RUST_EXEC=("${MISE_EXEC[@]}" env "RUST_TOOLCHAIN_BIN=$RUST_TOOLCHAIN_BIN" sh -c \
  'PATH="$RUST_TOOLCHAIN_BIN:$PATH" exec "$@"' verify-local-rust)

effective_path() {
  local executable="$1"
  # Keep $1 for the child shell, where it is the requested executable name.
  # shellcheck disable=SC2016
  "${RUST_EXEC[@]}" sh -c 'command -v "$1"' verify-local-path "$executable"
}

RUSTC_EFFECTIVE_PATH="$(effective_path rustc)"
CARGO_EFFECTIVE_PATH="$(effective_path cargo)"
CLIPPY_EFFECTIVE_PATH="$(effective_path cargo-clippy)"
CARGO_FMT_EFFECTIVE_PATH="$(effective_path cargo-fmt)"
RUSTFMT_EFFECTIVE_PATH="$(effective_path rustfmt)"
RUSTC_VER="$("${RUST_EXEC[@]}" rustc --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
CARGO_VER="$("${RUST_EXEC[@]}" cargo --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
CLIPPY_VER="$("${RUST_EXEC[@]}" cargo clippy --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
MBX_CLIPPY_VER="$("${RUST_EXEC[@]}" mbx clippy --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
RUSTFMT_VER="$("${RUST_EXEC[@]}" cargo fmt --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
RUSTFMT_BIN_VER="$("${RUST_EXEC[@]}" rustfmt --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
CLIPPY_BIN_VER=""
CARGO_FMT_BIN_VER=""
CARGO_DENY_VER=""
if [ -x "$CLIPPY_TOOL_PATH" ]; then
  CLIPPY_BIN_VER="$("$CLIPPY_TOOL_PATH" --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
fi
if [ -x "$CARGO_FMT_TOOL_PATH" ]; then
  CARGO_FMT_BIN_VER="$("$CARGO_FMT_TOOL_PATH" --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
fi
if [ -x "$CARGO_DENY_BIN" ]; then
  CARGO_DENY_VER="$("$CARGO_DENY_BIN" --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
fi

repo_policy() {
  local action="$1"
  env \
    VELNOR_INTERNAL_OP=repo-policy-v1 \
    VELNOR_REPO_POLICY_ACTION="$action" \
    VELNOR_REPO_POLICY_ROOT="$ROOT" \
    VELNOR_REPO_POLICY_CHECK_UPSTREAM=0 \
    VELNOR_REPO_POLICY_WITH_ADVISORIES=0 \
    "${RUST_EXEC[@]}" mbx run --quiet --locked -p velnor-actions-cli \
      --bin velnor-actions
}
POLICY_SPECS="$(repo_policy toolchain-specs 2>"$TOOLCHAIN_POLICY_LOG")"
if [ "$POLICY_SPECS" != "$SPECS" ]; then
  fail "toolchain (Rust policy specs '$POLICY_SPECS' != bootstrap pins '$SPECS'; log: $TOOLCHAIN_POLICY_LOG)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
POLICY_MISE="$(repo_policy mise-version 2>"$MISE_POLICY_LOG")"
# Compare actual tool paths and versions before verification starts.
MBX_VER="$("${RUST_EXEC[@]}" mbx --version 2>>"$TOOLCHAIN_LOG" | awk '{print $2}')"
echo "effective: rustc $RUSTC_VER [$RUSTC_EFFECTIVE_PATH], cargo $CARGO_VER [$CARGO_EFFECTIVE_PATH]"
echo "effective: clippy $CLIPPY_VER [$CLIPPY_EFFECTIVE_PATH], MBX clippy $MBX_CLIPPY_VER"
echo "effective: cargo-fmt $RUSTFMT_VER [$CARGO_FMT_EFFECTIVE_PATH]"
echo "effective: rustfmt $RUSTFMT_BIN_VER [$RUSTFMT_EFFECTIVE_PATH], mbx $MBX_VER"
echo "effective: cargo-deny $CARGO_DENY_VER [$CARGO_DENY_BIN]"
if [ "$RUSTC_VER" != "$RUST_PIN" ] || [ "$CARGO_VER" != "$RUST_PIN" ] ||
   [ "$MBX_VER" != "$MBX_PIN" ] ||
   [ "$RUSTC_EFFECTIVE_PATH" != "$RUSTC_TOOL_PATH" ] ||
   [ "$CARGO_EFFECTIVE_PATH" != "$CARGO_TOOL_PATH" ] ||
   [ "$CLIPPY_EFFECTIVE_PATH" != "$CLIPPY_TOOL_PATH" ] ||
   [ "$CARGO_FMT_EFFECTIVE_PATH" != "$CARGO_FMT_TOOL_PATH" ] ||
   [ "$RUSTFMT_EFFECTIVE_PATH" != "$RUSTFMT_TOOL_PATH" ] ||
   [ "$CLIPPY_VER" != "$CLIPPY_BIN_VER" ] ||
   [ "$MBX_CLIPPY_VER" != "$CLIPPY_BIN_VER" ] ||
   [ "$RUSTFMT_VER" != "$CARGO_FMT_BIN_VER" ] ||
   [ "$CARGO_FMT_BIN_VER" != "$RUSTFMT_BIN_VER" ] ||
   [ "$CARGO_DENY_VER" != "$CARGO_DENY_PIN" ]; then
  fail "toolchain (effective Rust tools do not match the selected toolchain; log: $TOOLCHAIN_LOG)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
# Resolve the pinned Nextest executable and give MBX a Cargo dispatcher.
# Cargo can search CARGO_HOME/bin before PATH for external commands.
# Changing PATH alone cannot reliably select the pinned executable.
NEXTEST_RUN=()
nextest_route_is_pinned() {
  local bin="$1" binary_version="$2" route_version="$3" expected_version="$4"
  [ -n "$bin" ] && [ -x "$bin" ] &&
    [ "$binary_version" = "$expected_version" ] &&
    [ "$route_version" = "$expected_version" ]
}

NEXTEST_BIN="$(mise --no-config which cargo-nextest --tool "aqua:nextest-rs/nextest/cargo-nextest@$NEXTEST_PIN" 2>/dev/null || true)"
NEXTEST_BIN_VER=""
if [ -n "$NEXTEST_BIN" ]; then
  NEXTEST_BIN_VER="$("$NEXTEST_BIN" --version 2>/dev/null | awk 'NR==1{print $2}')"
fi
if [ -n "$NEXTEST_BIN" ] && [ "$NEXTEST_BIN_VER" = "$NEXTEST_PIN" ]; then
  CARGO_DISPATCHER="$LOG_DIR/cargo-dispatch"
  if ! prepare_nextest_dispatcher "$CARGO_DISPATCHER"; then
    fail "toolchain (The script cannot prepare the Cargo dispatcher for pinned Nextest. Log: $TOOLCHAIN_LOG)"
    echo "verify-local: FAIL:$FAILURES"
    exit 1
  fi
  # shellcheck disable=SC2016
  NEXTEST_RUN=("${RUST_EXEC[@]}" env "CARGO=$CARGO_DISPATCHER" "NEXTEST_BIN=$NEXTEST_BIN" \
    "CARGO_TOOL_PATH=$CARGO_TOOL_PATH" sh -c 'exec mbx nextest "$@"' verify-local-nextest)
  NEXTEST_RUN_VER="$("${NEXTEST_RUN[@]}" --version 2>/dev/null | awk 'NR==1{print $2}')"
  echo "effective: nextest $NEXTEST_RUN_VER via pinned executable and MBX Cargo dispatcher"
else
  echo "effective: no pinned Nextest binary at $NEXTEST_BIN (pin $NEXTEST_PIN)"
fi
if ! nextest_route_is_pinned "$NEXTEST_BIN" "$NEXTEST_BIN_VER" "${NEXTEST_RUN_VER:-}" "$NEXTEST_PIN"; then
  fail "toolchain (pinned Nextest executable or MBX Nextest version mismatch; expected $NEXTEST_PIN)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
pass "toolchain"

# --- fmt -----------------------------------------------------------------
stage fmt "${RUST_EXEC[@]}" cargo fmt --all --check

# Audit both dependency graphs with pinned cargo-deny 0.20.2.
# The root and nested workspaces use separate lockfiles and policies.
stage root-deny "${RUST_EXEC[@]}" "$CARGO_DENY_BIN" --locked \
  --manifest-path "$ROOT/Cargo.toml" --config "$ROOT/deny.toml" check

# Nested runner workspace. Root `cargo --workspace` excludes it
# (`exclude = ["crates/velnor-runner"]`). Pass its lockfile and scoped policy explicitly.
RUNNER_MANIFEST="crates/velnor-runner/Cargo.toml"
if [ -f "$RUNNER_MANIFEST" ]; then
  stage runner-fmt "${RUST_EXEC[@]}" cargo fmt --manifest-path "$RUNNER_MANIFEST" --all -- --check
  stage runner-clippy "${RUST_EXEC[@]}" mbx clippy --manifest-path "$RUNNER_MANIFEST" --locked --workspace --all-targets -- -D warnings
  stage runner-nextest "${NEXTEST_RUN[@]}" run --manifest-path "$RUNNER_MANIFEST" --locked --workspace
  stage runner-doctest "${RUST_EXEC[@]}" mbx test --manifest-path "$RUNNER_MANIFEST" --locked --workspace --doc
  stage runner-deny "${RUST_EXEC[@]}" "$CARGO_DENY_BIN" --locked \
    --manifest-path "$ROOT/$RUNNER_MANIFEST" --config "$ROOT/crates/velnor-runner/deny.toml" check
else
  fail "runner-workspace (manifest missing: $RUNNER_MANIFEST)"
fi

# --- repo policy -----------------------------------------------------------
stage repo-policy repo_policy freshness

# --- generated-tree freshness ----------------------------------------------
GEN_DIR=""
GEN_DIR="$(mktemp -d 2>/dev/null)"
if [ -z "$GEN_DIR" ]; then
  fail "generated-tree (mktemp failed; logs: $LOG_DIR)"
else
  echo "--- verify-local: generated-selector"
  if python3 -B scripts/test_cargo_artifact_executable.py \
    >"$GENERATED_SELECTOR_LOG" 2>&1; then
    pass "generated-selector"
  else
    fail "generated-selector (log: $GENERATED_SELECTOR_LOG)"
  fi
  echo "--- verify-local: generated-tree"
  if "${RUST_EXEC[@]}" mbx build --locked --message-format=json-render-diagnostics \
    -p velnor-actions-cli --bin velnor-actions \
    >"$GENERATED_ARTIFACTS" \
    2>"$GENERATED_BUILD_LOG"; then
    BIN="$(python3 scripts/cargo_artifact_executable.py \
      --workspace-root "$ROOT" \
      --manifest-path crates/velnor-actions-cli/Cargo.toml \
      --target velnor-actions \
      "$GENERATED_ARTIFACTS" \
      2>>"$GENERATED_BUILD_LOG")"
    # Schema 2 emits the full product workflow set; compare it directly.
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

# --- per-crate clippy, tests, doctests, docs ---------------------------------
MEMBERS=""
MEMBERS="$(repo_policy workspace-members 2>"$CRATE_LIST_LOG")"
if [ -z "$MEMBERS" ]; then
  fail "crate-list (log: $CRATE_LIST_LOG)"
else
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "clippy-$safe" "${RUST_EXEC[@]}" mbx clippy --locked -p "$member" --all-targets -- -D warnings
  done
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "test-$safe" "${NEXTEST_RUN[@]}" run --locked -p "$member"
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
          stage "doctest-$safe" "${RUST_EXEC[@]}" mbx test --locked -p "$member" --doc
          ;;
        *)
          echo "skip: doctest-$safe (no library targets)"
          ;;
      esac
    done
  fi
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "doc-$safe" "${RUST_EXEC[@]}" mbx doc --locked --offline -p "$member" --no-deps
  done
fi

# --- checked-in integration fixtures -------------------------------------------
# Runs the fixture-consuming suites by name so a missing or unexercised
# `tests/fixtures` tree fails here even when the broad integration pass
# below stays green (e.g. after a filter/rename silently drops them).
echo "--- verify-local: fixtures"
if "${NEXTEST_RUN[@]}" run --locked -p velnor-actions-orchestrator --test velnor_orchestrator \
  -E 'test(evasion_fixtures_are_all_flagged)' \
  >"$FIXTURES_LOG" 2>&1; then
  pass "fixtures"
else
  fail "fixtures (log: $FIXTURES_LOG)"
fi

# --- whole-workspace integration pass ----------------------------------------
echo "--- verify-local: integration"
if "${NEXTEST_RUN[@]}" run --workspace --locked --profile ci --no-tests fail \
  >"$INTEGRATION_LOG" 2>&1; then
  pass "integration (nextest ci)"
else
  fail "integration (log: $INTEGRATION_LOG)"
fi

# --- summary -------------------------------------------------------------------
if [ -z "$FAILURES" ]; then
  echo "verify-local: PASS"
  exit 0
else
  echo "verify-local: FAIL:$FAILURES"
  echo "verify-local: logs retained in $LOG_DIR"
  exit 1
fi
