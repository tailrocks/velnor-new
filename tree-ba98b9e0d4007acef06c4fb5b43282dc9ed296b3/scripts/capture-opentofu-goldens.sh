#!/usr/bin/env bash
# T04/T07 supplementary goldens: CLI-level plan/generate bytes for Rust-only
# fixtures the parity corpus does not cover (nested, mbx-nextest, empty-suite,
# minimal-cargo CLI surface) plus the producer dogfood repo.
#
# The parity suite (impl_cli_parity_golden) remains the primary bracket
# (plan-v1 response, expected-set, task report); this script pins the
# user-facing plan text and generated YAML bytes around it.
#
# Usage: scripts/capture-opentofu-goldens.sh [capture|check [CLI_BINARY]]
#        scripts/capture-opentofu-goldens.sh check-release CLI_BINARY CANDIDATE_MANIFEST MANIFEST_SHA256
#   capture  regenerate crates/apps/velnor-actions-cli/fixtures/opentofu-goldens/ (only at known-good)
#   check    regenerate to temp and byte-diff (default; never writes goldens)
#   CLI_BINARY uses that exact executable and skips the default debug build.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GOLDEN_DIR="$ROOT/crates/apps/velnor-actions-cli/fixtures/opentofu-goldens"
WORK=""
MODE="${1:-check}"
BIN_EXPLICIT=0

case "$MODE" in
  capture|check|check-release) ;;
  *)
    echo "FATAL: usage: $0 [capture|check [CLI_BINARY]]"
    exit 2
    ;;
esac
if [ "$MODE" = "check-release" ]; then
  if [ "$#" -ne 4 ]; then
    echo "FATAL: usage: $0 check-release CLI_BINARY CANDIDATE_MANIFEST MANIFEST_SHA256"
    exit 2
  fi
elif [ "$#" -gt 2 ]; then
  echo "FATAL: usage: $0 [capture|check [CLI_BINARY]]"
  exit 2
fi
if ! CALLER_DIR="$(pwd -P)"; then
  echo "FATAL: could not resolve caller directory"
  exit 2
fi
if [ "$MODE" = "check-release" ]; then
  BIN_EXPLICIT=1
  BIN_ARG="$2"
  MANIFEST_ARG="$3"
  CANDIDATE_MANIFEST_SHA256="$4"
  if [ -z "$BIN_ARG" ] || [ -z "$MANIFEST_ARG" ] || [ -z "$CANDIDATE_MANIFEST_SHA256" ]; then
    echo "FATAL: explicit candidate binary, manifest path, and manifest SHA-256 are required"
    exit 2
  fi
  case "$BIN_ARG" in
    /*) BIN="$BIN_ARG" ;;
    *) BIN="$CALLER_DIR/$BIN_ARG" ;;
  esac
  case "$MANIFEST_ARG" in
    /*) CANDIDATE_MANIFEST="$MANIFEST_ARG" ;;
    *) CANDIDATE_MANIFEST="$CALLER_DIR/$MANIFEST_ARG" ;;
  esac
elif [ "$#" -eq 2 ]; then
  BIN_EXPLICIT=1
  BIN_ARG="$2"
  if [ -z "$BIN_ARG" ]; then
    echo "FATAL: explicit CLI binary path is empty"
    exit 2
  fi
  case "$BIN_ARG" in
    /*) BIN="$BIN_ARG" ;;
    *) BIN="$CALLER_DIR/$BIN_ARG" ;;
  esac
else
  BIN="$ROOT/target/debug/velnor-actions"
fi

FIXTURES="nested mbx-nextest empty-suite minimal-cargo"

fail=0
note() { echo "$1"; }
die() { echo "FAIL: $1"; fail=1; }

source "$ROOT/scripts/generator-release/qualification-goldens.sh"

# Portable link-preserving recursive copy. Plain `cp -r` dereferences
# symlinks (BSD: -r is -RL; GNU: -r on links is non-portable), which
# materialized generated CLAUDE.md symlinks as regular files. `-RP`
# is POSIX and pins link-as-link on both BSD and GNU cp.
copy_tree() {
  cp -RP "$1" "$2"
}

# Explicit schema-only input for positive ConsumerV1 fixture repos. It
# carries placeholder values and is never used as release evidence.
write_fixture_consumer_manifest() {
  local repo="$1" manifest="$1/.velnor/release-manifest.json"
  if [ -e "$manifest" ] || [ -L "$manifest" ]; then
    [ -f "$manifest" ] && [ ! -L "$manifest" ]
    return $?
  fi
  if [ -L "$repo/.velnor" ] || { [ -e "$repo/.velnor" ] && [ ! -d "$repo/.velnor" ]; }; then
    return 1
  fi
  mkdir -p "$repo/.velnor" || return 1
  cp "$ROOT/fixtures/consumer-release-manifest.json" "$manifest"
}

# Do not pin checkout-specific paths printed by `generate` in stderr goldens.
normalize_generate_stderr() {
  local path="$1" normalized="$1.normalized"
  if ! sed -e 's|^Preview: .*|Preview: <preview>|' \
    -e 's|^Repository: .*|Repository: <repo>|' \
    "$path" >"$normalized"; then
    rm -f "$normalized"
    return 1
  fi
  if ! mv "$normalized" "$path"; then
    rm -f "$normalized"
    return 1
  fi
}

# Deterministic content+link pin of a tree: sha256 per regular file
# (sorted) plus one `link <path> -> <target>` line per symlink (sorted).
# `find -type f` alone skips links, leaving targets unpinned.
hash_tree() {
  (cd "$1" && {
    find . -type f | LC_ALL=C sort | while IFS= read -r file; do
      local_digest="$(file_sha256 "$file")" || exit 1
      printf '%s  %s\n' "$local_digest" "$file"
    done
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
  if [ "$BIN_EXPLICIT" -eq 1 ]; then
    if [ ! -f "$BIN" ] || [ ! -x "$BIN" ]; then
      echo "FATAL: explicit CLI binary must be a regular executable file: $BIN"
      exit 2
    fi
    return 0
  fi
  (cd "$ROOT" && cargo build --locked -p velnor-actions-cli) >/dev/null 2>&1 \
    || { echo "FATAL: cargo build failed"; exit 2; }
}

# Deterministic scratch git checkout of one fixture; echoes its commit head.
setup_case() {
  local case="$1" repo head
  repo="$WORK/$case"
  mkdir -p "$repo"
  copy_tree "$ROOT/fixtures/$case/." "$repo/"
  mkdir -p "$repo/.velnor"
  if [ ! -f "$repo/.velnor/config.toml" ]; then
    printf 'schema = 1\n\n[workflow]\ndefault_branch = "main"\n' > "$repo/.velnor/config.toml"
  fi
  if [ "$MODE" = "check-release" ]; then
    stage_candidate_manifest "$repo" "$case"
  elif ! write_fixture_consumer_manifest "$repo"; then
    echo "FATAL: could not prepare explicit consumer schema fixture for $case"
    exit 2
  fi
  (cd "$repo" \
    && git init -q \
    && git add -A \
    && GIT_AUTHOR_NAME=v GIT_AUTHOR_EMAIL=v@v GIT_COMMITTER_NAME=v GIT_COMMITTER_EMAIL=v@v \
       GIT_AUTHOR_DATE='2026-01-01T00:00:00Z' GIT_COMMITTER_DATE='2026-01-01T00:00:00Z' \
       git -c commit.gpgsign=false commit -qm "golden" \
  ) >/dev/null 2>&1 || { echo "FATAL: git setup failed for $case"; exit 2; }
  head=$(git -C "$repo" rev-parse HEAD)
  echo "$head"
}

capture_case() {
  local case="$1" repo="$2" head="$3" out="$4"
  local preview="$out/preview"
  mkdir -p "$out" "$preview"
  (cd "$repo" && "$BIN" plan >"$out/plan.raw.txt" 2>"$out/plan.stderr.txt"; echo "$?" >"$out/plan.exit")
  if [ "$(cat "$out/plan.exit")" != "0" ]; then
    echo "FATAL: plan failed for $case"
    exit 2
  fi
  sed -e "s|Repository: .*|Repository: <repo>|" -e "s|$head|<head>|g" \
    "$out/plan.raw.txt" >"$out/plan.txt"
  rm "$out/plan.raw.txt"
  (cd "$repo" && "$BIN" generate --output-dir "$preview" >"$out/generate.stdout.txt" 2>"$out/generate.stderr.txt"; echo "$?" >"$out/generate.exit")
  if ! normalize_generate_stderr "$out/generate.stderr.txt"; then
    echo "FATAL: could not normalize generate diagnostics for $case"
    exit 2
  fi
  if [ "$(cat "$out/generate.exit")" != "0" ] || [ ! -d "$preview/.github" ]; then
    echo "FATAL: generate failed for $case"
    exit 2
  fi
  hash_tree "$preview" "$out/tree.sha256"
}

check_release_policy_negative() {
  local repo="$WORK/hostile-config" output="$WORK/hostile-output"
  setup_case hostile-config >/dev/null
  if (cd "$repo" && "$BIN" generate --output-dir "$output" >"$WORK/hostile.stdout" 2>"$WORK/hostile.stderr"); then
    echo "FAIL: release candidate accepted the hostile policy fixture"
    fail=1
  elif ! grep -F '.velnor/config.toml: evil: unknown_config_field' "$WORK/hostile.stderr"; then
    echo "FAIL: release candidate rejected the hostile fixture for an unexpected reason"
    fail=1
  else
    note "release policy negative match: hostile-config"
  fi
}

capture_dogfood() {
  local out="$1"
  local preview="$out/preview"
  local compare="$WORK/dogfood-compare"
  local head diff_status
  mkdir -p "$out" "$preview"
  head=$(git -C "$ROOT" rev-parse HEAD)
  (cd "$ROOT" && "$BIN" plan >"$out/plan.raw.txt" 2>"$out/plan.stderr.txt"; echo "$?" >"$out/plan.exit")
  if [ "$(cat "$out/plan.exit")" != "0" ]; then
    echo "FATAL: dogfood plan failed"
    exit 2
  fi
  sed -e "s|Repository: .*|Repository: <repo>|" -e "s|$head|<head>|g" -e "s|$ROOT|<root>|g" \
    "$out/plan.raw.txt" >"$out/plan.txt"
  rm "$out/plan.raw.txt"
  (cd "$ROOT" && "$BIN" generate --output-dir "$preview" >"$out/generate.stdout.txt" 2>"$out/generate.stderr.txt"; echo "$?" >"$out/generate.exit")
  if ! normalize_generate_stderr "$out/generate.stderr.txt"; then
    echo "FATAL: could not normalize dogfood generate diagnostics"
    exit 2
  fi
  if [ "$(cat "$out/generate.exit")" != "0" ] || [ ! -d "$preview/.github" ]; then
    echo "FATAL: dogfood generate failed"
    exit 2
  fi
  mkdir -p "$compare/shipping" "$compare/generated"
  if ! ln -s "$ROOT/.github" "$compare/shipping/.github" \
    || ! ln -s "$preview/.github" "$compare/generated/.github"; then
    echo "FATAL: could not prepare dogfood comparison"
    exit 2
  fi
  if (cd "$compare" && diff -r shipping/.github generated/.github) \
    >"$out/dogfood.diff" 2>&1; then
    echo "identical" >"$out/dogfood.verdict"
  else
    diff_status=$?
    if [ "$diff_status" -eq 1 ]; then
      echo "DIFFERS" >"$out/dogfood.verdict"
    else
      echo "FATAL: dogfood tree comparison failed (diff status $diff_status)"
      exit 2
    fi
  fi
  hash_tree "$preview" "$out/tree.sha256"
}

build_bin
if [ "$MODE" = "check-release" ]; then
  validate_candidate_manifest
fi
if ! WORK="$(mktemp -d "${TMPDIR:-/tmp}/velnor-goldens-work.XXXXXX")"; then
  echo "FATAL: could not create a private golden workspace"
  exit 2
fi
# The EXIT trap invokes this function after capture/check completes.
# shellcheck disable=SC2329
cleanup_work() {
  local result=$?
  if [ "$result" -eq 0 ]; then
    if ! rm -rf "$WORK"; then
      echo "FATAL: could not remove private golden workspace: $WORK" >&2
      trap - EXIT
      exit 2
    fi
  else
    echo "retaining failed golden workspace: $WORK" >&2
  fi
}
trap cleanup_work EXIT

stage="$WORK/stage"
mkdir -p "$stage"
if [ "$MODE" = "check-release" ]; then
  for case in $FIXTURES; do
    repo="$WORK/$case"
    if ! head="$(setup_case "$case")"; then
      echo "FATAL: could not set up release fixture $case"
      exit 2
    fi
    capture_release_case "$case" "$repo" "$stage/$case"
    if ! diff "$GOLDEN_DIR/cases/$case/tree.sha256" "$stage/$case/tree.sha256"; then
      die "release fixture golden mismatch: $case"
    else
      note "release fixture match: $case"
    fi
    if ! link_identity "$GOLDEN_DIR/cases/$case/preview" >"$stage/$case/golden.links" \
      || ! link_identity "$stage/$case/preview" >"$stage/$case/stage.links" \
      || ! diff "$stage/$case/golden.links" "$stage/$case/stage.links"; then
      die "release fixture symlink mismatch: $case"
    fi
  done
  check_release_policy_negative
  capture_release_dogfood "$stage/dogfood"
  [ "$fail" = "0" ] && note "ALL RELEASE FIXTURE GOLDENS AND DOGFOOD PARITY MATCH"
  exit "$fail"
fi
for case in $FIXTURES; do
  repo="$WORK/$case"
  if ! head="$(setup_case "$case")"; then
    echo "FATAL: could not set up fixture $case"
    exit 2
  fi
  capture_case "$case" "$repo" "$head" "$stage/$case"
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
