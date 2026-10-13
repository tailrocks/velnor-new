#!/usr/bin/env bash
# Local verification entrypoint: the offline subset of CI in one command.
#
# Stages (every stage always runs; all failures are reported):
#   fmt               `cargo fmt --all --check`
#   repo-policy       `scripts/check-freshness.sh` (pins, policy mirror,
#                     upstream evidence, deny policy; the live advisory scan
#                     runs in CI, not here)
#   generated-tree    build the CLI, `generate --output-dir` to a temp dir,
#                     and `diff -r` the staged `.github` tree (mirrors the CI
#                     "Check generated files" step)
#   clippy-<crate>    per-crate `cargo clippy --all-targets -- -D warnings`
#   test-<crate>      per-crate `cargo test` (unit plus integration plus doc)
#   doctest-<crate>   per-crate `cargo test --doc` for crates with library
#                     targets (mirrors the CI Doctests step; binary-only
#                     crates have no doctests and are reported skipped)
#   doc-<crate>       per-crate `cargo doc --no-deps` (mirrors the CI
#                     Documentation step: rustdoc warnings fail the build)
#   fixtures          fixture-consuming integration tests by name (currently
#                     the f2-evasion fixture suite), proving the checked-in
#                     `tests/fixtures` tree is exercised and green
#   integration       whole-workspace pass: nextest `ci` profile when
#                     `cargo nextest` is installed, else `cargo test`
#
# Every cargo invocation passes `--locked`; the lockfile is policy. The
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

# --- fmt -----------------------------------------------------------------
stage fmt cargo fmt --all --check

# --- repo policy -----------------------------------------------------------
stage repo-policy scripts/check-freshness.sh

# --- generated-tree freshness ----------------------------------------------
GEN_DIR=""
GEN_DIR="$(mktemp -d 2>/dev/null)"
if [ -z "$GEN_DIR" ]; then
  fail "generated-tree (mktemp failed)"
else
  echo "--- verify-local: generated-tree"
  if cargo build --locked -p velnor-actions-cli --bin velnor-actions \
    >"/tmp/verify-local-generated-build.log" 2>&1; then
    BIN="$(find target/debug target/release -maxdepth 1 -name velnor-actions -type f 2>/dev/null | head -n 1)"
    if [ -z "$BIN" ] && [ -n "${CARGO_TARGET_DIR:-}" ]; then
      BIN="$(find "$CARGO_TARGET_DIR/debug" "$CARGO_TARGET_DIR/release" -maxdepth 1 -name velnor-actions -type f 2>/dev/null | head -n 1)"
    fi
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
MEMBERS="$(python3 -c 'import json,subprocess; print(" ".join(sorted(p["name"] for p in json.loads(subprocess.run(["cargo","metadata","--locked","--no-deps","--format-version","1","--offline"],capture_output=True,text=True,check=True).stdout)["packages"])))' 2>/tmp/verify-local-crate-list.log)"
if [ -z "$MEMBERS" ]; then
  fail "crate-list (log: /tmp/verify-local-crate-list.log)"
else
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "clippy-$safe" cargo clippy --locked -p "$member" --all-targets -- -D warnings
  done
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "test-$safe" cargo test --locked -p "$member"
  done
  LIB_MEMBERS=""
  LIB_MEMBERS="$(python3 -c 'import json,subprocess; print(" ".join(sorted(p["name"] for p in json.loads(subprocess.run(["cargo","metadata","--locked","--no-deps","--format-version","1","--offline"],capture_output=True,text=True,check=True).stdout)["packages"] if any("lib" in t.get("kind", []) for t in p["targets"]))))' 2>/tmp/verify-local-doctest-list.log)"
  if [ -z "$LIB_MEMBERS" ]; then
    fail "doctest-list (log: /tmp/verify-local-doctest-list.log)"
  else
    for member in $MEMBERS; do
      safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
      case " $LIB_MEMBERS " in
        *" $member "*)
          stage "doctest-$safe" cargo test --locked -p "$member" --doc
          ;;
        *)
          echo "skip: doctest-$safe (no library targets)"
          ;;
      esac
    done
  fi
  for member in $MEMBERS; do
    safe="$(printf '%s' "$member" | tr -c 'A-Za-z0-9' '_')"
    stage "doc-$safe" cargo doc --locked --offline -p "$member" --no-deps
  done
fi

# --- checked-in integration fixtures -------------------------------------------
# Runs the fixture-consuming suites by name so a stale or unexercised
# `tests/fixtures` tree fails here even when the broad integration pass
# below stays green (e.g. after a filter/rename silently drops them).
echo "--- verify-local: fixtures"
if cargo test --locked -p velnor-actions-orchestrator --test velnor_orchestrator \
  evasion_fixtures_are_all_flagged \
  >"/tmp/verify-local-fixtures.log" 2>&1; then
  pass "fixtures"
else
  fail "fixtures (log: /tmp/verify-local-fixtures.log)"
fi

# --- whole-workspace integration pass ----------------------------------------
echo "--- verify-local: integration"
if cargo nextest --version >/dev/null 2>&1; then
  if cargo nextest run --workspace --locked --profile ci --no-tests fail \
    >"/tmp/verify-local-integration.log" 2>&1; then
    pass "integration (nextest ci)"
  else
    fail "integration (log: /tmp/verify-local-integration.log)"
  fi
elif cargo test --locked --workspace >"/tmp/verify-local-integration.log" 2>&1; then
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
