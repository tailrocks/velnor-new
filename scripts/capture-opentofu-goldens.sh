#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# T04/T07 supplementary goldens: CLI-level plan/generate bytes for Rust-only
# fixtures the parity corpus does not cover (nested, mbx-nextest, empty-suite,
# minimal-cargo CLI surface) plus the producer dogfood repo.
#
# The parity suite (impl_cli_parity_golden) remains the primary bracket
# (plan-v1 response, expected-set, task report); this script pins the
# user-facing plan text and generated YAML bytes around it.
#
# Usage: scripts/capture-opentofu-goldens.sh --archive-guard PATH --archive-guard-sha256 SHA256 [capture|check [CLI_BINARY]]
#        scripts/capture-opentofu-goldens.sh --archive-guard PATH --archive-guard-sha256 SHA256 check-release CLI_BINARY CANDIDATE_MANIFEST MANIFEST_SHA256
#   capture  regenerate docs/proposed/opentofu-goldens/ (only at known-good)
#   check    regenerate to temp and byte-diff (default; never writes goldens)
#   CLI_BINARY uses that exact executable and skips the default debug build.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
GOLDEN_DIR="$ROOT/docs/proposed/opentofu-goldens"
WORK=""
QUALIFICATION_BUN_BIN=""
MODE="check"
MODE_EXPLICIT=0
BIN_EXPLICIT=0
KEEP_WORK="${VELNOR_GOLDEN_KEEP_WORK:-0}"

qualification_bun_bin() {
  if [ -n "$QUALIFICATION_BUN_BIN" ]; then
    return 0
  fi
  local declared candidate actual
  if ! command -v mise >/dev/null 2>&1; then
    echo 'FATAL: Mise is required to select the pinned Bun runtime' >&2
    return 1
  fi
  declared="$(mise --no-config --no-env --no-hooks config get -f "$ROOT/mise.toml" tools.bun)" || return 1
  if [ "$declared" != "1.4.4" ]; then
    printf 'FATAL: collector requires the declared stable Bun pin 1.4.4, found %s\n' "$declared" >&2
    return 1
  fi
  if [ "${VELNOR_BUN_VERSION+x}" = x ] && [ "$VELNOR_BUN_VERSION" != "$declared" ]; then
    printf 'FATAL: supplied Bun version %s differs from the declared pin %s\n' "$VELNOR_BUN_VERSION" "$declared" >&2
    return 1
  fi
  if [ "${VELNOR_BUN_BIN+x}" = x ]; then
    case "$VELNOR_BUN_BIN" in
      /*) candidate="$VELNOR_BUN_BIN" ;;
      *) echo 'FATAL: supplied Bun executable path must be absolute' >&2; return 1 ;;
    esac
    if [ ! -x "$candidate" ]; then
      printf 'FATAL: supplied Bun executable is unavailable: %s\n' "$candidate" >&2
      return 1
    fi
  else
    candidate="$(MISE_EXEC_AUTO_INSTALL=0 mise --no-config --no-env --no-hooks which bun --tool "bun@$declared")" || {
      printf 'FATAL: pinned bun@%s is not installed; install it with Mise before running the collector\n' "$declared" >&2
      return 1
    }
    case "$candidate" in
      /*) ;;
      *) printf 'FATAL: Mise returned a non-absolute Bun executable path: %s\n' "$candidate" >&2; return 1 ;;
    esac
  fi
  actual="$("$candidate" --version)" || return 1
  if [ "$actual" != "$declared" ]; then
    printf 'FATAL: installed Bun %s differs from the declared pin %s\n' "$actual" "$declared" >&2
    return 1
  fi
  QUALIFICATION_BUN_BIN="$candidate"
}

case "$KEEP_WORK" in
  0|1) ;;
  *) echo "FATAL: VELNOR_GOLDEN_KEEP_WORK must be 0 or 1"; exit 2 ;;
esac
POSITIONAL=()
ARCHIVE_GUARD_INPUT_PATH=""
ARCHIVE_GUARD_INPUT_SHA256=""
SEEN_ARCHIVE_GUARD=0
SEEN_ARCHIVE_GUARD_SHA256=0
if [ "$#" -gt 0 ]; then
  case "$1" in
    capture|check|check-release) MODE="$1"; MODE_EXPLICIT=1; shift ;;
  esac
fi
while [ "$#" -gt 0 ]; do
  case "$1" in
    capture|check|check-release)
      if [ "$MODE_EXPLICIT" -ne 0 ]; then
        echo "FATAL: usage: $0 [--archive-guard PATH --archive-guard-sha256 SHA256] [capture|check [CLI_BINARY]]"
        exit 2
      fi
      MODE="$1"
      MODE_EXPLICIT=1
      shift
      ;;
    --archive-guard)
      if [ "$SEEN_ARCHIVE_GUARD" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo "FATAL: archive guard options must appear once as a path and SHA-256 pair"
        exit 2
      fi
      SEEN_ARCHIVE_GUARD=1
      ARCHIVE_GUARD_INPUT_PATH="$2"
      shift 2
      ;;
    --archive-guard-sha256)
      if [ "$SEEN_ARCHIVE_GUARD_SHA256" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo "FATAL: archive guard options must appear once as a path and SHA-256 pair"
        exit 2
      fi
      SEEN_ARCHIVE_GUARD_SHA256=1
      ARCHIVE_GUARD_INPUT_SHA256="$2"
      shift 2
      ;;
    --*)
      echo "FATAL: usage: $0 [--archive-guard PATH --archive-guard-sha256 SHA256] [capture|check [CLI_BINARY]]"
      exit 2
      ;;
    *) POSITIONAL+=("$1"); shift ;;
  esac
done
if [ "$MODE_EXPLICIT" -eq 0 ] && [ "${#POSITIONAL[@]}" -gt 0 ]; then
  echo "FATAL: usage: $0 [--archive-guard PATH --archive-guard-sha256 SHA256] [capture|check [CLI_BINARY]]"
  exit 2
fi
if [ "$SEEN_ARCHIVE_GUARD" -ne "$SEEN_ARCHIVE_GUARD_SHA256" ]; then
  echo "FATAL: archive guard path and SHA-256 must be supplied together"
  exit 2
fi
if [ "$SEEN_ARCHIVE_GUARD" -eq 1 ] \
  && { [ -z "$ARCHIVE_GUARD_INPUT_PATH" ] || [ -z "$ARCHIVE_GUARD_INPUT_SHA256" ]; }; then
  echo "FATAL: archive guard path and SHA-256 must be non-empty"
  exit 2
fi
if [ "$MODE" = "check-release" ]; then
  if [ "${#POSITIONAL[@]}" -ne 3 ]; then
    echo "FATAL: usage: $0 [--archive-guard PATH --archive-guard-sha256 SHA256] check-release CLI_BINARY CANDIDATE_MANIFEST MANIFEST_SHA256"
    exit 2
  fi
elif [ "${#POSITIONAL[@]}" -gt 1 ]; then
  echo "FATAL: usage: $0 [--archive-guard PATH --archive-guard-sha256 SHA256] [capture|check [CLI_BINARY]]"
  exit 2
fi
if ! CALLER_DIR="$(pwd -P)"; then
  echo "FATAL: could not resolve caller directory"
  exit 2
fi
if [ -n "${VELNOR_GOLDEN_DIR:-}" ]; then
  case "$VELNOR_GOLDEN_DIR" in
    /*) requested_golden_dir="$VELNOR_GOLDEN_DIR" ;;
    *) requested_golden_dir="$CALLER_DIR/$VELNOR_GOLDEN_DIR" ;;
  esac
  golden_name="${requested_golden_dir##*/}"
  golden_parent="${requested_golden_dir%/*}"
  if [ -z "$golden_name" ] || [ "$golden_name" = . ] || [ "$golden_name" = .. ] \
    || ! golden_parent="$(cd -- "$golden_parent" 2>/dev/null && pwd -P)"; then
    echo "FATAL: external golden directory parent must exist"
    exit 2
  fi
  GOLDEN_DIR="$golden_parent/$golden_name"
  case "$GOLDEN_DIR" in
    "$ROOT"|"$ROOT"/*)
      echo "FATAL: VELNOR_GOLDEN_DIR must be outside the repository"
      exit 2
      ;;
  esac
  if [ -L "$GOLDEN_DIR" ] || { [ -e "$GOLDEN_DIR" ] && [ ! -d "$GOLDEN_DIR" ]; }; then
    echo "FATAL: external golden directory must be a regular directory path"
    exit 2
  fi
fi
if [ "$MODE" = "check-release" ]; then
  BIN_EXPLICIT=1
  BIN_ARG="${POSITIONAL[0]}"
  MANIFEST_ARG="${POSITIONAL[1]}"
  CANDIDATE_MANIFEST_SHA256="${POSITIONAL[2]}"
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
elif [ "${#POSITIONAL[@]}" -eq 1 ]; then
  BIN_EXPLICIT=1
  BIN_ARG="${POSITIONAL[0]}"
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
QUALIFICATION_GUARD_INPUT_PATH="$ARCHIVE_GUARD_INPUT_PATH"
QUALIFICATION_GUARD_INPUT_SHA256="$ARCHIVE_GUARD_INPUT_SHA256"

FIXTURES="nested mbx-nextest empty-suite minimal-cargo"

fail=0
note() { echo "$1"; }
die() { echo "FAIL: $1"; fail=1; }

source "$ROOT/scripts/generator-release/qualification-goldens.sh"
prepare_qualification_guard || exit 2

# Portable link-preserving recursive copy. Plain `cp -r` dereferences
# symlinks (BSD: -r is -RL; GNU: -r on links is non-portable), which
# materialized generated CLAUDE.md symlinks as regular files. `-RP`
# is POSIX and pins link-as-link on both BSD and GNU cp.
copy_tree() {
  cp -RP "$1" "$2"
}

# Schema-only current-version input for disposable ConsumerV1 scratch repos.
# Canonical URLs exercise the manifest shape; deterministic markers let the
# checked-in oracle match check-release after it validates the exact candidate
# manifest. These values are not release evidence and never leave this scratch
# repo.
fixture_binary_target() {
  local header target file_size phoff phnum ph_index ph_offset ph_header
  local interp_offset interp_size interp_path expected_interp matched_interpreters
  local byte_index
  local -a bytes ph_bytes
  header="$(LC_ALL=C od -An -v -tu1 -N64 "$BIN" | LC_ALL=C tr '\n' ' ')" || return 1
  read -r -a bytes <<<"$header"
  if [ "${#bytes[@]}" -lt 20 ]; then
    printf 'FATAL: unsupported or ambiguous fixture binary target: truncated executable header %s\n' "$header" >&2
    return 1
  fi

  if [ "${bytes[0]}" = 127 ] && [ "${bytes[1]}" = 69 ] \
    && [ "${bytes[2]}" = 76 ] && [ "${bytes[3]}" = 70 ]; then
    if [ "${#bytes[@]}" -ne 64 ] || [ "${bytes[4]}" != 2 ] \
      || [ "${bytes[5]}" != 1 ] || [ "${bytes[6]}" != 1 ] \
      || { [ "${bytes[16]}" != 2 ] && [ "${bytes[16]}" != 3 ]; } \
      || [ "${bytes[17]}" != 0 ] || [ "${bytes[18]}" != 62 ] \
      || [ "${bytes[19]}" != 0 ] || [ "${bytes[20]}" != 1 ] \
      || [ "${bytes[21]}" != 0 ] || [ "${bytes[22]}" != 0 ] \
      || [ "${bytes[23]}" != 0 ] || [ "${bytes[52]}" != 64 ] \
      || [ "${bytes[53]}" != 0 ] || [ "${bytes[54]}" != 56 ] \
      || [ "${bytes[55]}" != 0 ]; then
      printf 'FATAL: unsupported or ambiguous fixture binary target: invalid ELF64 x86-64 header %s\n' "$header" >&2
      return 1
    fi
    for byte_index in 36 37 38 39; do
      if [ "${bytes[$byte_index]}" != 0 ]; then
        printf 'FATAL: unsupported or ambiguous fixture binary target: ELF program table offset exceeds the supported 32-bit file-offset range\n' >&2
        return 1
      fi
    done
    phoff=$((bytes[32] + (bytes[33] << 8) + (bytes[34] << 16) + (bytes[35] << 24)))
    phnum=$((bytes[56] + (bytes[57] << 8)))
    file_size="$(wc -c <"$BIN")" || return 1
    if [ "$phnum" -eq 0 ] || [ "$phoff" -gt "$file_size" ] \
      || [ "$((phnum * 56))" -gt "$((file_size - phoff))" ]; then
      printf 'FATAL: unsupported or ambiguous fixture binary target: invalid ELF program table bounds\n' >&2
      return 1
    fi
    expected_interp="$(printf '%s\000' /lib64/ld-linux-x86-64.so.2 | LC_ALL=C od -An -v -tu1 | LC_ALL=C tr '\n' ' ')"
    matched_interpreters=0
    for ((ph_index = 0; ph_index < phnum; ph_index++)); do
      ph_offset=$((phoff + ph_index * 56))
      ph_header="$(LC_ALL=C od -An -v -tu1 -j "$ph_offset" -N56 "$BIN" | LC_ALL=C tr '\n' ' ')" || return 1
      read -r -a ph_bytes <<<"$ph_header"
      if [ "${#ph_bytes[@]}" -ne 56 ]; then
        printf 'FATAL: unsupported or ambiguous fixture binary target: truncated ELF program header\n' >&2
        return 1
      fi
      if [ "${ph_bytes[0]}" = 3 ] && [ "${ph_bytes[1]}" = 0 ] \
        && [ "${ph_bytes[2]}" = 0 ] && [ "${ph_bytes[3]}" = 0 ]; then
        for byte_index in 12 13 14 15 36 37 38 39; do
          if [ "${ph_bytes[$byte_index]}" != 0 ]; then
            printf 'FATAL: unsupported or ambiguous fixture binary target: ELF interpreter exceeds the supported 32-bit file-offset range\n' >&2
            return 1
          fi
        done
        interp_offset=$((ph_bytes[8] + (ph_bytes[9] << 8) + (ph_bytes[10] << 16) + (ph_bytes[11] << 24)))
        interp_size=$((ph_bytes[32] + (ph_bytes[33] << 8) + (ph_bytes[34] << 16) + (ph_bytes[35] << 24)))
        if [ "$interp_size" -ne 28 ] || [ "$interp_offset" -gt "$file_size" ] \
          || [ "$interp_size" -gt "$((file_size - interp_offset))" ]; then
          printf 'FATAL: unsupported or ambiguous fixture binary target: invalid ELF interpreter bounds\n' >&2
          return 1
        fi
        interp_path="$(LC_ALL=C od -An -v -tu1 -j "$interp_offset" -N"$interp_size" "$BIN" | LC_ALL=C tr '\n' ' ')" || return 1
        if [ "$interp_path" = "$expected_interp" ]; then
          matched_interpreters=$((matched_interpreters + 1))
        else
          printf 'FATAL: unsupported or ambiguous fixture binary target: unexpected ELF interpreter\n' >&2
          return 1
        fi
      fi
    done
    if [ "$matched_interpreters" -eq 1 ]; then
      printf '%s\n' x86_64-unknown-linux-gnu
    else
      printf 'FATAL: unsupported or ambiguous fixture binary target: missing or duplicate GNU ELF interpreter\n' >&2
      return 1
    fi
  elif [ "${bytes[0]}" = 207 ] && [ "${bytes[1]}" = 250 ] \
    && [ "${bytes[2]}" = 237 ] && [ "${bytes[3]}" = 254 ] \
    && [ "${bytes[12]}" = 2 ] && [ "${bytes[13]}" = 0 ] \
    && [ "${bytes[14]}" = 0 ] && [ "${bytes[15]}" = 0 ] \
    && [ "${#bytes[@]}" -ge 32 ]; then
    case "${bytes[4]}:${bytes[5]}:${bytes[6]}:${bytes[7]}" in
      '12:0:0:1') printf '%s\n' aarch64-apple-darwin ;;
      '7:0:0:1') printf '%s\n' x86_64-apple-darwin ;;
      *) printf 'FATAL: unsupported or ambiguous fixture binary target: Mach-O header %s\n' "$header" >&2; return 1 ;;
    esac
  else
    printf 'FATAL: unsupported or ambiguous fixture binary target: executable header %s\n' "$header" >&2
    return 1
  fi
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

normalize_plan_stderr() {
  local path="$1" bun_bin
  shift
  qualification_bun_bin || return 1
  bun_bin="$QUALIFICATION_BUN_BIN"
  "$bun_bin" --bun run -- - "$path" "$@" <<'NODE'
const fs = require('node:fs');
const file = process.argv[2];
const args = process.argv.slice(3);
if (args.length === 0 || args.length % 2 !== 0) {
  process.stderr.write('FATAL: invalid plan stderr path normalization arguments\n');
  process.exit(2);
}
const mappings = [];
for (let index = 0; index < args.length; index += 2) {
  if (!args[index]) {
    process.stderr.write('FATAL: empty path in plan stderr normalization\n');
    process.exit(2);
  }
  mappings.push([Buffer.from(args[index]), Buffer.from(args[index + 1])]);
}
mappings.sort((left, right) => right[0].length - left[0].length);
let content = fs.readFileSync(file);
for (const [needle, replacement] of mappings) {
  const chunks = [];
  let offset = 0;
  while (true) {
    const match = content.indexOf(needle, offset);
    if (match < 0) break;
    chunks.push(content.subarray(offset, match), replacement);
    offset = match + needle.length;
  }
  if (offset !== 0) {
    chunks.push(content.subarray(offset));
    content = Buffer.concat(chunks);
  }
}
fs.writeFileSync(file, content);
NODE
}

# Deterministic content+link pin of a tree: sha256 per regular file
# (sorted) plus one `link <path> -> <target>` line per symlink (sorted).
# `find -type f` alone skips links, leaving targets unpinned.
hash_tree() {
  (cd "$1" && {
    files="$(find . -type f | LC_ALL=C sort)" || exit 1
    if [ -n "$files" ]; then
      if command -v sha256sum >/dev/null 2>&1; then
        printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 sha256sum || exit 1
      elif command -v shasum >/dev/null 2>&1; then
        printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 shasum -a 256 || exit 1
      else
        echo "FATAL: no SHA-256 utility is available" >&2
        exit 1
      fi
    fi
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
  local case="$1" repo head binary_target
  repo="$WORK/$case"
  mkdir -p "$repo"
  copy_tree "$ROOT/fixtures/$case/." "$repo/"
  mkdir -p "$repo/.velnor"
  if [ ! -f "$repo/.velnor/config.toml" ]; then
    printf 'schema = 1\n\n[workflow]\ndefault_branch = "main"\n' > "$repo/.velnor/config.toml"
  fi
  if [ "$MODE" = "check-release" ]; then
    stage_candidate_manifest "$repo" "$case"
  else
    if ! binary_target="$(fixture_binary_target)" \
      || ! prepare_fixture_consumer_manifest "$repo" "$BIN" "$binary_target"; then
      echo "FATAL: could not prepare fixture-only consumer input for $case"
      exit 2
    fi
  fi
  local git_log="$WORK/$case-git-setup.log"
  (cd "$repo" \
    && fixture_git init -q \
    && fixture_git add -A \
    && fixture_git commit -qm "golden" \
  ) >"$git_log" 2>&1 || { cat "$git_log" >&2; echo "FATAL: git setup failed for $case"; exit 2; }
  rm -f "$git_log"
  head=$(fixture_git -C "$repo" rev-parse HEAD)
  echo "$head"
}

capture_case() {
  local case="$1" repo="$2" head="$3" out="$4"
  local preview="$out/preview"
  local raw="$WORK/raw/$case"
  mkdir -p "$out" "$preview" "$raw"
  (cd "$repo" && run_qualification_candidate "$BIN" plan >"$raw/plan.raw.txt" 2>"$raw/plan.stderr.raw.txt"; echo "$?" >"$out/plan.exit")
  if ! cp "$raw/plan.stderr.raw.txt" "$out/plan.stderr.txt" \
    || ! normalize_plan_stderr "$out/plan.stderr.txt" \
      "$repo" '<repo>' "$ROOT" '<root>' "$WORK" '<work>'; then
    echo "FATAL: could not stage normalized plan diagnostics for $case"
    exit 2
  fi
  if [ "$(cat "$out/plan.exit")" != "0" ]; then
    echo "FATAL: plan failed for $case"
    cat "$raw/plan.stderr.raw.txt" >&2
    exit 2
  fi
  sed -e "s|Repository: .*|Repository: <repo>|" -e "s|$head|<head>|g" \
    "$raw/plan.raw.txt" >"$out/plan.txt"
  if [ "$KEEP_WORK" -eq 1 ] && ! cp "$raw/plan.raw.txt" "$raw/plan.source-raw.txt"; then
    echo "FATAL: could not retain raw plan output for $case"
    exit 2
  fi
  (cd "$repo" && run_qualification_candidate "$BIN" generate --output-dir "$preview" >"$out/generate.stdout.txt" 2>"$raw/generate.stderr.raw.txt"; echo "$?" >"$out/generate.exit")
  if ! cp "$raw/generate.stderr.raw.txt" "$out/generate.stderr.txt"; then
    echo "FATAL: could not stage generate diagnostics for $case"
    exit 2
  fi
  if ! normalize_generate_stderr "$out/generate.stderr.txt"; then
    echo "FATAL: could not normalize generate diagnostics for $case"
    exit 2
  fi
  if [ "$(cat "$out/generate.exit")" != "0" ] || [ ! -d "$preview/.github" ]; then
    echo "FATAL: generate failed for $case"
    exit 2
  fi
  local CANDIDATE_MANIFEST="$repo/.velnor/release-manifest.json"
  if ! validate_candidate_output_bindings "$preview" \
    || ! normalize_candidate_digests "$preview"; then
    echo "FATAL: fixture output bindings failed for $case"
    exit 2
  fi
  hash_tree "$preview" "$out/tree.sha256"
}

check_release_policy_negative() {
  local repo="$WORK/hostile-config" output="$WORK/hostile-output"
  setup_case hostile-config >/dev/null
  if (cd "$repo" && run_qualification_candidate "$BIN" generate --output-dir "$output" >"$WORK/hostile.stdout" 2>"$WORK/hostile.stderr"); then
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
  local raw="$WORK/raw/dogfood"
  local compare="$WORK/dogfood-compare"
  local head diff_status
  mkdir -p "$out" "$preview" "$raw"
  head=$(git -C "$ROOT" rev-parse HEAD)
  (cd "$ROOT" && run_qualification_candidate "$BIN" plan >"$raw/plan.raw.txt" 2>"$raw/plan.stderr.raw.txt"; echo "$?" >"$out/plan.exit")
  if ! cp "$raw/plan.stderr.raw.txt" "$out/plan.stderr.txt" \
    || ! normalize_plan_stderr "$out/plan.stderr.txt" \
      "$ROOT" '<root>' "$WORK" '<work>'; then
    echo "FATAL: could not stage normalized dogfood plan diagnostics"
    exit 2
  fi
  if [ "$(cat "$out/plan.exit")" != "0" ]; then
    echo "FATAL: dogfood plan failed"
    cat "$raw/plan.stderr.raw.txt" >&2
    exit 2
  fi
  sed -e "s|Repository: .*|Repository: <repo>|" -e "s|$head|<head>|g" -e "s|$ROOT|<root>|g" \
    "$raw/plan.raw.txt" >"$out/plan.txt"
  if [ "$KEEP_WORK" -eq 1 ] && ! cp "$raw/plan.raw.txt" "$raw/plan.source-raw.txt"; then
    echo "FATAL: could not retain raw dogfood plan output"
    exit 2
  fi
  (cd "$ROOT" && run_qualification_candidate "$BIN" generate --output-dir "$preview" >"$out/generate.stdout.txt" 2>"$raw/generate.stderr.raw.txt"; echo "$?" >"$out/generate.exit")
  if ! cp "$raw/generate.stderr.raw.txt" "$out/generate.stderr.txt"; then
    echo "FATAL: could not stage raw dogfood generate diagnostics"
    exit 2
  fi
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
  if [ "$result" -eq 0 ] && [ "$KEEP_WORK" -ne 1 ]; then
    if ! rm -rf "$WORK"; then
      echo "FATAL: could not remove private golden workspace: $WORK" >&2
      trap - EXIT
      exit 2
    fi
  else
    if [ "$result" -eq 0 ]; then
      echo "retaining successful golden evidence workspace: $WORK" >&2
    else
      echo "retaining failed golden workspace: $WORK" >&2
    fi
  fi
}
trap cleanup_work EXIT

if ! physical_work="$(cd -- "$WORK" && pwd -P)"; then
  echo "FATAL: could not resolve the private golden workspace path" >&2
  exit 2
fi
WORK="$physical_work"

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
pids=()
cases=()
for case in $FIXTURES; do
  cases+=("$case")
  (
    repo="$WORK/$case"
    if ! head="$(setup_case "$case")"; then
      echo "FATAL: could not set up fixture $case"
      exit 2
    fi
    capture_case "$case" "$repo" "$head" "$stage/$case"
    note "captured $case (plan exit $(cat "$stage/$case/plan.exit"))"
  ) >"$WORK/$case.capture.log" 2>&1 &
  pids+=("$!")
done
statuses=()
for pid in "${pids[@]}"; do
  status=0
  wait "$pid" || status=$?
  statuses+=("$status")
done
for index in "${!cases[@]}"; do
  case="${cases[$index]}"
  cat "$WORK/$case.capture.log"
  rm -f "$WORK/$case.capture.log"
  if [ "${statuses[$index]}" -ne 0 ]; then
    exit "${statuses[$index]}"
  fi
done
capture_dogfood "$stage/dogfood"
note "captured dogfood (plan exit $(cat "$stage/dogfood/plan.exit"), tree $(cat "$stage/dogfood/dogfood.verdict" 2>/dev/null))"

if [ "$MODE" = "capture" ]; then
  if [ ! -d "$stage/dogfood/preview/.github" ]; then
    die "dogfood preview has no .github directory"
  fi
  if [ "$(cat "$stage/dogfood/dogfood.verdict" 2>/dev/null)" != "identical" ]; then
    die "dogfood generated workflows are not identical to repository workflows"
  fi
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
