#!/usr/bin/env bash
# Local verification entrypoint: the offline subset of CI in one command.
#
# Stages (every stage always runs; all failures are reported):
#   toolchain       resolve the pinned toolchain (mise.toml [tools] must
#                   equal the CI mirror in .velnor/version-policy.toml for
#                   rust/mr-boxington/nextest, else fail on drift), `mise
#                   install` the specs, and record the effective versions
#   fmt               pinned `cargo fmt --all --check`
#   repo-policy       `scripts/check-freshness.sh` (pins, policy mirror,
#                     upstream evidence, deny policy; the live advisory scan
#                     runs in CI, not here)
#   generated-tree    build the CLI, `generate --output-dir` to a temp dir,
#                     and `diff -r` the committed `.github` tree, including
#                     schema-2 workflows, against `generate --output-dir`.
#   clippy-<crate>    per-crate pinned `cargo clippy --all-targets -- -D warnings`
#   test-<crate>      per-crate pinned `cargo test` (unit plus integration plus doc)
#   doctest-<crate>   per-crate pinned `cargo test --doc` for crates with library
#                     targets (mirrors the CI Doctests step; binary-only
#                     crates have no doctests and are reported skipped)
#   doc-<crate>       per-crate pinned `cargo doc --no-deps` (mirrors the CI
#                     Documentation step: rustdoc warnings fail the build)
#   fixtures          fixture-consuming integration tests by name (currently
#                     the f2-evasion fixture suite), proving the checked-in
#                     `tests/fixtures` tree is exercised and green
#   integration       whole-workspace pass: pinned nextest `ci` profile when
#                     available, else pinned `cargo test`
#
# Every cargo/nextest invocation runs under `mise exec` with explicit
# `tool@exact` specs resolved from mise.toml and cross-checked against
# the CI version-policy mirror: bare ambient cargo never runs here, and
# the repo mise wrapper routes cargo through the pinned MBX shim. Every
# cargo invocation also passes `--locked`; the lockfile is policy. The
# script honors `CARGO_TARGET_DIR` from the environment. Exit status is 0
# only when every stage passes; the failing stage names print at the end.
#
# Usage: scripts/verify-local.sh
set -uo pipefail

ROOT=""
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

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
  shift
  echo "--- verify-local: $name"
  if "$@" >"/tmp/verify-local-$name.log" 2>&1; then
    pass "$name"
  else
    fail "$name (log: /tmp/verify-local-$name.log)"
  fi
}

if ! command -v mise >/dev/null 2>&1; then
  echo "verify-local: FAIL: mise not found on PATH (install mise, then re-run)"
  exit 1
fi

toml_tool_pin() {
  local file="$1" key="$2"
  awk -v key="$key" -f "$ROOT/scripts/toml-tool-pin.awk" "$file"
}

# --- pinned toolchain ----------------------------------------------------------
# Specs come from mise.toml [tools]; each must equal the CI mirror
# (.velnor/version-policy.toml [tools], itself checked against the compiled
# catalog by dogfood generate), so local runs use CI's exact pins.
echo "--- verify-local: toolchain"
SPECS=""
RUST_MISE="$(toml_tool_pin mise.toml rust 2>"/tmp/verify-local-toolchain.log")"
RUST_POLICY="$(toml_tool_pin .velnor/version-policy.toml rust 2>>"/tmp/verify-local-toolchain.log")"
MBX_MISE="$(toml_tool_pin mise.toml mr-boxington 2>>"/tmp/verify-local-toolchain.log")"
MBX_POLICY="$(toml_tool_pin .velnor/version-policy.toml mr-boxington 2>>"/tmp/verify-local-toolchain.log")"
NEXTEST_MISE="$(toml_tool_pin mise.toml aqua:nextest-rs/nextest/cargo-nextest 2>>"/tmp/verify-local-toolchain.log")"
NEXTEST_POLICY="$(toml_tool_pin .velnor/version-policy.toml nextest 2>>"/tmp/verify-local-toolchain.log")"
if [ -z "$RUST_MISE" ] || [ -z "$RUST_POLICY" ] ||
   [ -z "$MBX_MISE" ] || [ -z "$MBX_POLICY" ] ||
   [ -z "$NEXTEST_MISE" ] || [ -z "$NEXTEST_POLICY" ]; then
  cat "/tmp/verify-local-toolchain.log" >&2 || true
  echo "verify-local: FAIL: toolchain (pin drift; log: /tmp/verify-local-toolchain.log)"
  exit 1
fi
if [ "$RUST_MISE" != "$RUST_POLICY" ] || [ "$MBX_MISE" != "$MBX_POLICY" ] ||
   [ "$NEXTEST_MISE" != "$NEXTEST_POLICY" ]; then
  echo "pin drift: mise.toml rust=$RUST_MISE mr-boxington=$MBX_MISE nextest=$NEXTEST_MISE" >&2
  echo "           version-policy rust=$RUST_POLICY mr-boxington=$MBX_POLICY nextest=$NEXTEST_POLICY" >&2
  echo "verify-local: FAIL: toolchain (pin drift; log: /tmp/verify-local-toolchain.log)"
  exit 1
fi
SPECS="rust@$RUST_MISE mr-boxington@$MBX_MISE aqua:nextest-rs/nextest/cargo-nextest@$NEXTEST_MISE"
echo "pinned specs: $SPECS"
# Order is fixed by the pairs list above: rust, mr-boxington, nextest.
# shellcheck disable=SC2206
_PIN_PARTS=($SPECS)
RUST_PIN="${_PIN_PARTS[0]#*@}"
MBX_PIN="${_PIN_PARTS[1]#*@}"
NEXTEST_PIN="${_PIN_PARTS[2]#*@}"
POLICY_MISE="$(toml_tool_pin .velnor/version-policy.toml mise 2>"/tmp/verify-local-mise-policy.log")"
LOCAL_MISE="$(mise --version 2>/dev/null | awk "{print \$1}")"
echo "mise: local $LOCAL_MISE, policy $POLICY_MISE"
if [ "$LOCAL_MISE" != "$POLICY_MISE" ]; then
  echo "WARNING: local mise $LOCAL_MISE differs from policy $POLICY_MISE; continuing with local mise and pinned tools"
fi
# shellcheck disable=SC2206
SPEC_ARR=($SPECS)
if ! mise install "${SPEC_ARR[@]}" >>"/tmp/verify-local-toolchain.log" 2>&1; then
  fail "toolchain (mise install failed; log: /tmp/verify-local-toolchain.log)"
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
POLICY_SPECS="$(repo_policy toolchain-specs 2>"/tmp/verify-local-toolchain-policy.log")"
if [ "$POLICY_SPECS" != "$SPECS" ]; then
  fail "toolchain (Rust policy specs '$POLICY_SPECS' != bootstrap pins '$SPECS'; log: /tmp/verify-local-toolchain-policy.log)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
POLICY_MISE="$(repo_policy mise-version 2>"/tmp/verify-local-mise-policy.log")"
# The effective binaries must BE the pins: a symlink-rust or an ambient
# cargo-nextest next to cargo can otherwise shadow the pinned tools.
CARGO_VER="$("${MISE_EXEC[@]}" cargo --version 2>>"/tmp/verify-local-toolchain.log" | awk "{print \$2}")"
MBX_VER="$("${MISE_EXEC[@]}" mbx --version 2>>"/tmp/verify-local-toolchain.log" | awk "{print \$2}")"
echo "effective: cargo $CARGO_VER, mbx $MBX_VER"
if [ "$CARGO_VER" != "$RUST_PIN" ] || [ "$MBX_VER" != "$MBX_PIN" ]; then
  fail "toolchain (effective cargo $CARGO_VER / mbx $MBX_VER != pins $RUST_PIN / $MBX_PIN)"
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
# Nextest resolves two ways, and neither is stable: the bare
# `cargo-nextest` intermittently shadows to an ambient 0.9.143 in
# `~/.cargo/bin` instead of the mise aqua path. Prefer whichever
# reports the exact pin; the choice is echoed, never assumed.
NEXTEST_RUN=()
BARE_VER="$("${MISE_EXEC[@]}" cargo-nextest --version 2>/dev/null | awk 'NR==1{print $2}')"
SUB_VER="$("${MISE_EXEC[@]}" cargo nextest --version 2>/dev/null | awk 'NR==1{print $2}')"
if [ "$BARE_VER" = "$NEXTEST_PIN" ]; then
  NEXTEST_RUN=("${MISE_EXEC[@]}" cargo-nextest nextest)
  echo "effective: nextest $BARE_VER via bare cargo-nextest"
elif [ "$SUB_VER" = "$NEXTEST_PIN" ]; then
  NEXTEST_RUN=("${MISE_EXEC[@]}" cargo nextest)
  echo "effective: nextest $SUB_VER via cargo nextest"
else
  echo "effective: no pinned nextest (bare $BARE_VER, subcommand $SUB_VER, pin $NEXTEST_PIN); integration falls back to cargo test"
fi
pass "toolchain"

# --- fmt -----------------------------------------------------------------
stage fmt "${MISE_EXEC[@]}" cargo fmt --all --check

# Nested runner workspace. Root `cargo --workspace` excludes it
# (`exclude = ["crates/velnor-runner"]`). cargo-deny 0.20.2 matches
# `CARGO_DENY_VERSION`; pass its own lockfile and scoped policy explicitly.
RUNNER_MANIFEST="crates/velnor-runner/Cargo.toml"
if [ -f "$RUNNER_MANIFEST" ]; then
  stage runner-fmt "${MISE_EXEC[@]}" cargo fmt --manifest-path "$RUNNER_MANIFEST" --all -- --check
  stage runner-clippy "${MISE_EXEC[@]}" cargo clippy --manifest-path "$RUNNER_MANIFEST" --locked --workspace --all-targets -- -D warnings
  if [ "${#NEXTEST_RUN[@]}" -gt 0 ]; then
    stage runner-nextest "${NEXTEST_RUN[@]}" run --manifest-path "$RUNNER_MANIFEST" --locked --workspace
  else
    stage runner-test "${MISE_EXEC[@]}" cargo test --manifest-path "$RUNNER_MANIFEST" --locked --workspace
  fi
  stage runner-doctest "${MISE_EXEC[@]}" cargo test --manifest-path "$RUNNER_MANIFEST" --locked --workspace --doc
  # Invoke the pinned binary. A cargo shim can reject the deny subcommand.
  stage runner-deny mise exec "cargo-deny@0.20.2" -- cargo-deny --locked --manifest-path "$RUNNER_MANIFEST" --config "crates/velnor-runner/deny.toml" check
fi

# --- repo policy -----------------------------------------------------------
stage repo-policy "${MISE_EXEC[@]}" bash scripts/check-freshness.sh

# --- generated-tree freshness ----------------------------------------------
GEN_DIR=""
GEN_DIR="$(mktemp -d 2>/dev/null)"
if [ -z "$GEN_DIR" ]; then
  fail "generated-tree (mktemp failed)"
else
  echo "--- verify-local: generated-tree"
  if "${MISE_EXEC[@]}" cargo build --locked -p velnor-actions-cli --bin velnor-actions \
    >"/tmp/verify-local-generated-build.log" 2>&1; then
    BIN="$(find target/debug target/release -maxdepth 1 -name velnor-actions -type f 2>/dev/null | head -n 1)"
    if [ -z "$BIN" ] && [ -n "${CARGO_TARGET_DIR:-}" ]; then
      BIN="$(find "$CARGO_TARGET_DIR/debug" "$CARGO_TARGET_DIR/release" -maxdepth 1 -name velnor-actions -type f 2>/dev/null | head -n 1)"
    fi
    # Schema 2 emits the full product workflow set; compare it directly.
    if [ -n "$BIN" ] && "$BIN" generate --output-dir "$GEN_DIR/tree" \
      >"/tmp/verify-local-generated-run.log" 2>&1 &&
      diff -r --brief .github "$GEN_DIR/tree/.github" \
        >"/tmp/verify-local-generated-diff.log" 2>&1; then
      pass "generated-tree"
    else
      fail "generated-tree (see /tmp/verify-local-generated-*.log)"
    fi
  else
    fail "generated-tree (build failed: /tmp/verify-local-generated-build.log)"
  fi
  rm -rf "$GEN_DIR"
fi

# --- per-crate clippy, tests, doctests, docs ---------------------------------
MEMBERS=""
MEMBERS="$(repo_policy workspace-members 2>/tmp/verify-local-crate-list.log)"
if [ -z "$MEMBERS" ]; then
  fail "crate-list (log: /tmp/verify-local-crate-list.log)"
else
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "clippy-$safe" "${MISE_EXEC[@]}" cargo clippy --locked -p "$member" --all-targets -- -D warnings
  done
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "test-$safe" "${MISE_EXEC[@]}" cargo test --locked -p "$member"
  done
  LIB_MEMBERS=""
  LIB_MEMBERS="$(repo_policy library-members 2>/tmp/verify-local-doctest-list.log)"
  if [ -z "$LIB_MEMBERS" ]; then
    fail "doctest-list (log: /tmp/verify-local-doctest-list.log)"
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

# --- checked-in integration fixtures -------------------------------------------
# Runs the fixture-consuming suites by name so a stale or unexercised
# `tests/fixtures` tree fails here even when the broad integration pass
# below stays green (e.g. after a filter/rename silently drops them).
echo "--- verify-local: fixtures"
if "${MISE_EXEC[@]}" cargo test --locked -p velnor-actions-orchestrator --test velnor_orchestrator \
  evasion_fixtures_are_all_flagged \
  >"/tmp/verify-local-fixtures.log" 2>&1; then
  pass "fixtures"
else
  fail "fixtures (log: /tmp/verify-local-fixtures.log)"
fi

# --- whole-workspace integration pass ----------------------------------------
echo "--- verify-local: integration"
if [ "${#NEXTEST_RUN[@]}" -gt 0 ]; then
  if "${NEXTEST_RUN[@]}" run --workspace --locked --profile ci --no-tests fail \
    >"/tmp/verify-local-integration.log" 2>&1; then
    pass "integration (nextest ci)"
  else
    fail "integration (log: /tmp/verify-local-integration.log)"
  fi
elif "${MISE_EXEC[@]}" cargo test --locked --workspace >"/tmp/verify-local-integration.log" 2>&1; then
  pass "integration (cargo test)"
else
  fail "integration (log: /tmp/verify-local-integration.log)"
fi

# --- summary -------------------------------------------------------------------
if [ -z "$FAILURES" ]; then
  echo "verify-local: PASS"
  exit 0
else
  echo "verify-local: FAIL:$FAILURES"
  exit 1
fi
