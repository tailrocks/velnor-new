#!/usr/bin/env bash
# T04/T07 supplementary goldens: CLI-level plan/generate bytes for Rust-only
# fixtures the parity corpus does not cover (nested, mbx-nextest, empty-suite,
# minimal-cargo CLI surface) plus the producer dogfood repo.
#
# The parity suite (impl_cli_parity_golden) remains the primary bracket
# (plan-v1 response, expected-set, task report); this script pins the
# user-facing plan text and generated YAML bytes around it.
#
# Usage: scripts/capture-opentofu-goldens.sh [capture|check]
#   capture  regenerate docs/proposed/opentofu-goldens/ (only at known-good)
#   check    regenerate to temp and byte-diff (default; never writes goldens)
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/velnor-actions"
GOLDEN_DIR="$ROOT/docs/proposed/opentofu-goldens"
WORK="/tmp/velnor-goldens-work"
MODE="${1:-check}"

FIXTURES="nested mbx-nextest empty-suite minimal-cargo"

fail=0
note() { echo "$1"; }
die() { echo "FAIL: $1"; fail=1; }

build_bin() {
  (cd "$ROOT" && cargo build --locked -p velnor-actions-cli) >/dev/null 2>&1 \
    || { echo "FATAL: cargo build failed"; exit 2; }
}

# Deterministic scratch git checkout of one fixture; echoes "repo head".
setup_case() {
  local case="$1" repo head
  repo="$WORK/$case"
  rm -rf "$repo"
  mkdir -p "$repo"
  cp -r "$ROOT/fixtures/$case/." "$repo/"
  if [ ! -f "$repo/.velnor/config.toml" ]; then
    mkdir -p "$repo/.velnor"
    printf 'schema = 1\n\n[workflow]\ndefault_branch = "main"\n' > "$repo/.velnor/config.toml"
  fi
  (cd "$repo" \
    && git init -q \
    && git add -A \
    && GIT_AUTHOR_NAME=v GIT_AUTHOR_EMAIL=v@v GIT_COMMITTER_NAME=v GIT_COMMITTER_EMAIL=v@v \
       GIT_AUTHOR_DATE='2026-01-01T00:00:00Z' GIT_COMMITTER_DATE='2026-01-01T00:00:00Z' \
       git -c commit.gpgsign=false commit -qm "golden" \
  ) >/dev/null 2>&1 || { echo "FATAL: git setup failed for $case"; exit 2; }
  head=$(git -C "$repo" rev-parse HEAD)
  echo "$repo $head"
}

capture_case() {
  local case="$1" repo="$2" head="$3" out="$4"
  local preview="$out/preview"
  mkdir -p "$out" "$preview"
  (cd "$repo" && "$BIN" plan >"$out/plan.raw.txt" 2>"$out/plan.stderr.txt"; echo "$?" >"$out/plan.exit")
  sed -e "s|Repository: .*|Repository: <repo>|" -e "s|$head|<head>|g" \
    "$out/plan.raw.txt" >"$out/plan.txt"
  rm "$out/plan.raw.txt"
  if [ "$(cat "$out/plan.exit")" = "0" ]; then
    (cd "$repo" && "$BIN" generate --output-dir "$preview" >"$out/generate.stdout.txt" 2>"$out/generate.stderr.txt"; echo "$?" >"$out/generate.exit")
    (cd "$preview" && find . -type f | sort | xargs sha256sum >"$out/tree.sha256")
  fi
}

capture_dogfood() {
  local out="$1"
  local preview="$out/preview"
  local head
  mkdir -p "$out" "$preview"
  head=$(git -C "$ROOT" rev-parse HEAD)
  (cd "$ROOT" && "$BIN" plan >"$out/plan.raw.txt" 2>"$out/plan.stderr.txt"; echo "$?" >"$out/plan.exit")
  sed -e "s|Repository: .*|Repository: <repo>|" -e "s|$head|<head>|g" -e "s|$ROOT|<root>|g" \
    "$out/plan.raw.txt" >"$out/plan.txt"
  rm "$out/plan.raw.txt"
  (cd "$ROOT" && "$BIN" generate --output-dir "$preview" >"$out/generate.stdout.txt" 2>"$out/generate.stderr.txt"; echo "$?" >"$out/generate.exit")
  if [ -d "$preview/.github" ]; then
    if diff -r "$ROOT/.github" "$preview/.github" >"$out/dogfood.diff" 2>&1; then
      echo "identical" >"$out/dogfood.verdict"
    else
      echo "DIFFERS" >"$out/dogfood.verdict"
    fi
    (cd "$preview" && find . -type f | sort | xargs sha256sum >"$out/tree.sha256")
  fi
}

build_bin
rm -rf "$WORK"
mkdir -p "$WORK"

stage="$WORK/stage"
rm -rf "$stage"
mkdir -p "$stage"
for case in $FIXTURES; do
  # shellcheck disable=SC2086
  set -- $(setup_case "$case")
  capture_case "$case" "$1" "$2" "$stage/$case"
  note "captured $case (plan exit $(cat "$stage/$case/plan.exit"))"
done
capture_dogfood "$stage/dogfood"
note "captured dogfood (plan exit $(cat "$stage/dogfood/plan.exit"), tree $(cat "$stage/dogfood/dogfood.verdict" 2>/dev/null))"

if [ "$MODE" = "capture" ]; then
  rm -rf "$GOLDEN_DIR/cases"
  mkdir -p "$GOLDEN_DIR"
  cp -r "$stage" "$GOLDEN_DIR/cases"
  (cd "$GOLDEN_DIR/cases" && find . -type f | sort | xargs sha256sum >"$GOLDEN_DIR/MANIFEST.sha256")
  note "wrote $GOLDEN_DIR/cases"
  exit 0
fi

# check mode
tmp="$WORK/check-diff"
rm -rf "$tmp"
mkdir -p "$tmp"
for case in $FIXTURES dogfood; do
  if ! diff -r "$GOLDEN_DIR/cases/$case" "$stage/$case" >"$tmp/$case.diff" 2>&1; then
    die "golden mismatch: $case (see $tmp/$case.diff)"
    head -20 "$tmp/$case.diff"
  else
    note "match: $case"
  fi
done
[ "$fail" = "0" ] && note "ALL GOLDENS MATCH"
exit "$fail"
