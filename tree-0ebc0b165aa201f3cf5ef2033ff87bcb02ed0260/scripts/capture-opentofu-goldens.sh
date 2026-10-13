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

# Portable link-preserving recursive copy. Plain `cp -r` dereferences
# symlinks (BSD: -r is -RL; GNU: -r on links is non-portable), which
# materialized generated CLAUDE.md symlinks as regular files. `-RP`
# is POSIX and pins link-as-link on both BSD and GNU cp.
copy_tree() {
  cp -RP "$1" "$2"
}

# Deterministic content+link pin of a tree: sha256 per regular file
# (sorted) plus one `link <path> -> <target>` line per symlink (sorted).
# `find -type f` alone skips links, leaving targets unpinned.
hash_tree() {
  (cd "$1" && {
    find . -type f | sort | xargs sha256sum
    find . -type l | sort | while IFS= read -r link; do
      printf 'link %s -> %s\n' "$link" "$(readlink "$link")"
    done
  } >"$2")
}

# Deterministic symlink identity of a tree, one `<path> -> <target>`
# line per link, sorted. `diff -r` follows links on both BSD and GNU,
# so a link-vs-materialized-file pair with equal bytes compares silent
# (proven on BSD); check mode diffs this listing explicitly.
link_identity() {
  (cd "$1" && find . -type l | sort | while IFS= read -r link; do
    printf '%s -> %s\n' "$link" "$(readlink "$link")"
  done)
}

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
  copy_tree "$ROOT/fixtures/$case/." "$repo/"
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
    normalize_repository_log "$out/generate.stderr.txt"
    hash_tree "$preview" "$out/tree.sha256"
  fi
}

normalize_repository_log() {
  local path="$1"
  local normalized="$path.normalized"
  sed -e 's|^Repository: .*|Repository: <repo>|' "$path" >"$normalized"
  mv "$normalized" "$path"
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
  normalize_repository_log "$out/generate.stderr.txt"
  if [ -d "$preview/.github" ]; then
    if diff -r "$ROOT/.github" "$preview/.github" >"$out/dogfood.diff" 2>&1; then
      echo "identical" >"$out/dogfood.verdict"
    else
      echo "DIFFERS" >"$out/dogfood.verdict"
    fi
    hash_tree "$preview" "$out/tree.sha256"
  fi
}

build_bin
rm -rf "$WORK"
mkdir -p "$WORK"

stage="$WORK/stage"
rm -rf "$stage"
mkdir -p "$stage"
for case in $FIXTURES; do
  # intentional word-split: setup_case prints the $1 $2 pair for capture_case
  # shellcheck disable=SC2086,SC2046
  set -- $(setup_case "$case")
  capture_case "$case" "$1" "$2" "$stage/$case"
  note "captured $case (plan exit $(cat "$stage/$case/plan.exit"))"
done
capture_dogfood "$stage/dogfood"
note "captured dogfood (plan exit $(cat "$stage/dogfood/plan.exit"), tree $(cat "$stage/dogfood/dogfood.verdict" 2>/dev/null))"

if [ "$MODE" = "capture" ]; then
  rm -rf "$GOLDEN_DIR/cases"
  mkdir -p "$GOLDEN_DIR"
  copy_tree "$stage" "$GOLDEN_DIR/cases"
  hash_tree "$GOLDEN_DIR/cases" "$GOLDEN_DIR/MANIFEST.sha256"
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
  elif ! link_identity "$GOLDEN_DIR/cases/$case" >"$tmp/$case.golden.links" 2>&1 \
    || ! link_identity "$stage/$case" >"$tmp/$case.stage.links" 2>&1 \
    || ! diff "$tmp/$case.golden.links" "$tmp/$case.stage.links" >"$tmp/$case.links.diff" 2>&1; then
    die "golden symlink mismatch: $case (see $tmp/$case.links.diff)"
    head -20 "$tmp/$case.links.diff"
  else
    note "match: $case"
  fi
done
[ "$fail" = "0" ] && note "ALL GOLDENS MATCH"
exit "$fail"
