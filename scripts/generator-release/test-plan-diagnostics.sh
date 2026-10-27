#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -uo pipefail

POSITIONAL=()
ARCHIVE_GUARD_INPUT_PATH=""
ARCHIVE_GUARD_INPUT_SHA256=""
SEEN_ARCHIVE_GUARD=0
SEEN_ARCHIVE_GUARD_SHA256=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --archive-guard)
      if [ "$SEEN_ARCHIVE_GUARD" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo 'FATAL: archive guard options must appear once as a path and SHA-256 pair' >&2
        exit 2
      fi
      SEEN_ARCHIVE_GUARD=1
      ARCHIVE_GUARD_INPUT_PATH="$2"
      shift 2
      ;;
    --archive-guard-sha256)
      if [ "$SEEN_ARCHIVE_GUARD_SHA256" -ne 0 ] || [ "$#" -lt 2 ]; then
        echo 'FATAL: archive guard options must appear once as a path and SHA-256 pair' >&2
        exit 2
      fi
      SEEN_ARCHIVE_GUARD_SHA256=1
      ARCHIVE_GUARD_INPUT_SHA256="$2"
      shift 2
      ;;
    --focused-prebuilt)
      POSITIONAL+=("$1")
      shift
      ;;
    --*)
      echo 'FATAL: usage: test-plan-diagnostics.sh ABSOLUTE_CLI FRESH_ABSOLUTE_OUTPUT [--focused-prebuilt ABSOLUTE_CANDIDATE ABSOLUTE_BUILD_EVIDENCE]' >&2
      exit 2
      ;;
    *) POSITIONAL+=("$1"); shift ;;
  esac
done
if [ "$SEEN_ARCHIVE_GUARD" -ne "$SEEN_ARCHIVE_GUARD_SHA256" ]; then
  echo 'FATAL: archive guard path and SHA-256 must be supplied together' >&2
  exit 2
fi
if [ "$SEEN_ARCHIVE_GUARD" -eq 1 ] \
  && { [ -z "$ARCHIVE_GUARD_INPUT_PATH" ] || [ -z "$ARCHIVE_GUARD_INPUT_SHA256" ]; }; then
  echo 'FATAL: archive guard path and SHA-256 must be non-empty' >&2
  exit 2
fi
if [ "${#POSITIONAL[@]}" -eq 2 ]; then
  FOCUSED_PREBUILT=0
  PREBUILT_CLI=''
  PREBUILT_BUILD=''
elif [ "${#POSITIONAL[@]}" -eq 5 ] && [ "${POSITIONAL[2]}" = --focused-prebuilt ]; then
  FOCUSED_PREBUILT=1
  PREBUILT_CLI=${POSITIONAL[3]}
  PREBUILT_BUILD=${POSITIONAL[4]}
else
  echo 'usage: test-plan-diagnostics.sh ABSOLUTE_CLI FRESH_ABSOLUTE_OUTPUT [--focused-prebuilt ABSOLUTE_CANDIDATE ABSOLUTE_BUILD_EVIDENCE]' >&2
  exit 2
fi
QUALIFICATION_GUARD_INPUT_PATH="$ARCHIVE_GUARD_INPUT_PATH"
QUALIFICATION_GUARD_INPUT_SHA256="$ARCHIVE_GUARD_INPUT_SHA256"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
source "$ROOT/scripts/generator-release/qualification-goldens.sh"
prepare_qualification_guard || exit 2
if [ "$SEEN_ARCHIVE_GUARD" -eq 0 ]; then
  ARCHIVE_GUARD_INPUT_PATH="$QUALIFICATION_GUARD_BIN"
  ARCHIVE_GUARD_INPUT_SHA256="$QUALIFICATION_GUARD_SHA256"
fi
ARCHIVE_GUARD_ARGS=(--archive-guard "$ARCHIVE_GUARD_INPUT_PATH" --archive-guard-sha256 "$ARCHIVE_GUARD_INPUT_SHA256")
CLI=${POSITIONAL[0]}
OUTPUT=${POSITIONAL[1]}
[[ "$CLI" = /* ]] || { echo 'FATAL: CLI path must be absolute' >&2; exit 2; }
cli_parent="$(cd "$(dirname "$CLI")" 2>/dev/null && pwd -P)" || { echo 'FATAL: CLI parent must exist' >&2; exit 2; }
CLI="$cli_parent/$(basename "$CLI")"
[[ -f "$CLI" && -x "$CLI" ]] || { echo 'FATAL: CLI must resolve to an executable file' >&2; exit 2; }
if [ "$FOCUSED_PREBUILT" -eq 1 ]; then
  [[ "$PREBUILT_CLI" = /* && "$PREBUILT_BUILD" = /* ]] \
    || { echo 'FATAL: focused prebuilt paths must be absolute' >&2; exit 2; }
  prebuilt_cli_parent="$(cd "$(dirname "$PREBUILT_CLI")" 2>/dev/null && pwd -P)" \
    || { echo 'FATAL: prebuilt CLI parent must exist' >&2; exit 2; }
  PREBUILT_CLI="$prebuilt_cli_parent/$(basename "$PREBUILT_CLI")"
  [[ -f "$PREBUILT_CLI" && -x "$PREBUILT_CLI" ]] \
    || { echo 'FATAL: focused prebuilt CLI must resolve to an executable file' >&2; exit 2; }
  [[ -d "$PREBUILT_BUILD" && ! -L "$PREBUILT_BUILD" ]] \
    || { echo 'FATAL: focused build evidence must be a regular directory' >&2; exit 2; }
  PREBUILT_BUILD="$(cd "$PREBUILT_BUILD" && pwd -P)"
fi
[[ "$OUTPUT" = /* && ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || { echo 'FATAL: output must be a fresh absolute path' >&2; exit 2; }
parent="$(cd "$(dirname "$OUTPUT")" 2>/dev/null && pwd -P)" || { echo 'FATAL: output parent must exist' >&2; exit 2; }
OUTPUT="$parent/$(basename "$OUTPUT")"
case "$OUTPUT" in "$ROOT"|"$ROOT"/*) echo 'FATAL: diagnostic evidence must remain outside the repository' >&2; exit 2 ;; esac
mkdir -m 700 "$OUTPUT" || exit 2

SOURCE_SHA="$(git -C "$ROOT" rev-parse HEAD)" || exit 2
ORIGINAL_PATH="$PATH"
REAL_MISE="$(command -v mise 2>/dev/null)" || REAL_MISE=''
case "$REAL_MISE" in
  /*) ;;
  '' ) echo 'FATAL: real Mise executable is unavailable' >&2; exit 2 ;;
  *)
    real_mise_parent="$(cd "$(dirname "$REAL_MISE")" 2>/dev/null && pwd -P)" \
      || { echo 'FATAL: real Mise executable parent is unavailable' >&2; exit 2; }
    REAL_MISE="$real_mise_parent/$(basename "$REAL_MISE")"
    ;;
esac
if [ ! -x "$REAL_MISE" ]; then
  echo "FATAL: real Mise executable is unavailable: $REAL_MISE" >&2
  exit 2
fi
PINNED_RUST_VERSION="$(test_mise_rust_version)" || {
  echo 'FATAL: could not resolve the owned pinned Rust version' >&2
  exit 2
}
GUARD="${VELNOR_TEST_CARGO_GUARD:-${VELNOR_TEST_RUST_BUILD_GUARD:-}}"
if [[ "$(uname -s)" == Darwin && "${GITHUB_ACTIONS:-}" != true && -z "$GUARD" ]]; then
  echo 'FATAL: set VELNOR_TEST_CARGO_GUARD to the required Rust build guard' >&2
  exit 2
fi
if [[ -n "$GUARD" && ( ! -f "$GUARD" || ! -x "$GUARD" ) ]]; then
  echo "FATAL: the configured Rust build guard is unavailable: $GUARD" >&2
  exit 2
fi
export VELNOR_TEST_CARGO_GUARD="$GUARD"

file_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  else
    shasum -a 256 "$1" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  fi
}

stdin_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  else
    shasum -a 256 | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
  fi
}

apply_diagnostic_source_mutation() {
  node - "$1" <<'NODE'
const fs = require('node:fs');
const path = process.argv[2];
const input = fs.readFileSync(path, 'utf8');
const marker = [
  '    print!("{text}");',
  "    if !text.ends_with('\\n') {",
  '        println!();',
  '    }',
  ''
].join('\n');
const tail = '    ExitCode::SUCCESS\n';
const addition = [
  '    if let Ok(mode) = std::env::var("VELNOR_TEST_PLAN_DIAGNOSTIC_MODE") {',
  '        let emit = match mode.as_str() {',
  '            "quiet" => false,',
  '            "warning-all" | "fail57" => true,',
  '            "nested-only" => cwd.file_name().is_some_and(|name| name == "nested"),',
  '            "dogfood-only" => match std::env::var_os("VELNOR_TEST_PLAN_SOURCE_ROOT") {',
  '                Some(root) => match (cwd.canonicalize(), std::fs::canonicalize(root)) {',
  '                    (Ok(cwd), Ok(root)) => cwd == root,',
  '                    _ => return ExitCode::from(56),',
  '                },',
  '                None => return ExitCode::from(56),',
  '            },',
  '            _ => return ExitCode::from(56),',
  '        };',
  '        if emit {',
  '            let repo = match cwd.canonicalize() { Ok(path) => path, Err(_) => return ExitCode::from(56) };',
  '            let root = match std::env::var_os("VELNOR_TEST_PLAN_SOURCE_ROOT") {',
  '                Some(path) => match std::fs::canonicalize(path) { Ok(path) => path, Err(_) => return ExitCode::from(56) },',
  '                None => return ExitCode::from(56),',
  '            };',
  '            let temp = match std::env::var_os("TMPDIR") {',
  '                Some(path) => match std::fs::canonicalize(path) { Ok(path) => path, Err(_) => return ExitCode::from(56) },',
  '                None => return ExitCode::from(56),',
  '            };',
  '            let entries = match std::fs::read_dir(temp) { Ok(entries) => entries, Err(_) => return ExitCode::from(56) };',
  '            let mut work = None;',
  '            for entry in entries {',
  '                let entry = match entry { Ok(entry) => entry, Err(_) => return ExitCode::from(56) };',
  '                if entry.file_name().to_string_lossy().starts_with("velnor-goldens-work.") {',
  '                    if work.is_some() { return ExitCode::from(56); }',
  '                    work = match entry.path().canonicalize() { Ok(path) => Some(path), Err(_) => return ExitCode::from(56) };',
  '                }',
  '            }',
  '            let work = match work { Some(path) => path, None => return ExitCode::from(56) };',
  '            eprintln!("plan-diagnostic-warning repository={} source-root={} work={}", repo.display(), root.display(), work.display());',
  '        }',
  '        if mode == "fail57" { return ExitCode::from(57); }',
  '    }',
  ''
].join('\n');
if (input.split(marker + tail).length !== 2) {
  process.stderr.write('FATAL: diagnostic source mutation marker count was not exactly one\n');
  process.exit(2);
}
fs.writeFileSync(path, input.replace(marker + tail, marker + addition + tail));
NODE
}

golden_fingerprint() {
  local directory="$1"
  (
    cd "$directory" || exit 2
    {
      files="$(find . -type f -print | LC_ALL=C sort)" || exit 2
      if [ -n "$files" ]; then
        if command -v sha256sum >/dev/null 2>&1; then
          printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 sha256sum || exit 2
        else
          printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 shasum -a 256 || exit 2
        fi
      fi
      find . -type l -print | LC_ALL=C sort | while IFS= read -r path; do
        printf 'link %s -> %s\n' "$path" "$(readlink "$path")"
      done
    } | stdin_sha256
  )
}

CLI_SHA="$(file_sha256 "$CLI")" || exit 2
CLI_VERSION="$(run_qualification_candidate "$CLI" --version | awk 'NR == 1 && NF == 2 && $1 == "velnor-actions" { print $2; next } { exit 1 } END { if (NR != 1) exit 1 }')" || exit 2
printf '%s\n' "$CLI" >"$OUTPUT/input-cli.path"
printf '%s\n' "$CLI_SHA" >"$OUTPUT/input-cli.sha256"
printf '%s\n' "$CLI_VERSION" >"$OUTPUT/input-cli.version"
printf '%s\n' "$SOURCE_SHA" >"$OUTPUT/source.commit"

BUILD="$OUTPUT/build"
mkdir -m 700 "$BUILD" || exit 2
if [ "$FOCUSED_PREBUILT" -eq 1 ]; then
  PREBUILT_SOURCE="$PREBUILT_BUILD/source"
  PREBUILT_PUBLIC="$PREBUILT_SOURCE/crates/velnor-actions-cli/src/dispatch/public.rs"
  PREBUILT_ARCHIVE_SHA="$(cat "$PREBUILT_BUILD/source.tar.sha256" 2>/dev/null)"
  PREBUILT_ORIGINAL_SHA="$(cat "$PREBUILT_BUILD/public.rs.original.sha256" 2>/dev/null)"
  PREBUILT_MUTATED_SHA="$(cat "$PREBUILT_BUILD/public.rs.mutated.sha256" 2>/dev/null)"
  PREBUILT_TARGET="$(cat "$PREBUILT_BUILD/target-directory.physical" 2>/dev/null)"
  PREBUILT_RECEIPT="$PREBUILT_BUILD/receipt.txt"
  ARCHIVE_SHA="$(git -C "$ROOT" archive --format=tar "$SOURCE_SHA" | stdin_sha256)" || exit 2
  ORIGINAL_GIT_SHA="$(git -C "$ROOT" show "$SOURCE_SHA:crates/velnor-actions-cli/src/dispatch/public.rs" | stdin_sha256)" || exit 2
  if [ ! -d "$PREBUILT_SOURCE" ] || [ -L "$PREBUILT_SOURCE" ] \
    || [ ! -f "$PREBUILT_BUILD/source.tar" ] \
    || [ ! -f "$PREBUILT_BUILD/public.rs.original" ] \
    || [ ! -f "$PREBUILT_BUILD/public.rs.patch" ] \
    || [ ! -f "$PREBUILT_BUILD/metadata.json" ] \
    || [ ! -f "$PREBUILT_BUILD/build.log" ] \
    || [ ! -f "$PREBUILT_RECEIPT" ] \
    || [ "$(cat "$PREBUILT_BUILD/build.exit" 2>/dev/null)" != 0 ] \
    || [ "$(cat "$PREBUILT_BUILD/metadata.exit" 2>/dev/null)" != 0 ] \
    || [ "$PREBUILT_ARCHIVE_SHA" != "$ARCHIVE_SHA" ] \
    || [ "$PREBUILT_ORIGINAL_SHA" != "$ORIGINAL_GIT_SHA" ] \
    || [ -z "$PREBUILT_MUTATED_SHA" ] \
    || [[ "$PREBUILT_TARGET" != /* ]]; then
    echo 'FATAL: focused prebuilt evidence does not match the selected source and successful build' >&2
    exit 2
  fi
  if [ "$(file_sha256 "$PREBUILT_BUILD/source.tar")" != "$ARCHIVE_SHA" ] \
    || [ "$(file_sha256 "$PREBUILT_BUILD/public.rs.original")" != "$ORIGINAL_GIT_SHA" ] \
    || [ "$(file_sha256 "$PREBUILT_PUBLIC")" != "$PREBUILT_MUTATED_SHA" ]; then
    echo 'FATAL: focused prebuilt archive or source hashes differ from the selected commit' >&2
    exit 2
  fi
  EXPECTED_PUBLIC="$BUILD/public.rs.expected"
  if ! cp "$PREBUILT_BUILD/public.rs.original" "$EXPECTED_PUBLIC" \
    || ! apply_diagnostic_source_mutation "$EXPECTED_PUBLIC" \
    || ! cmp -s "$EXPECTED_PUBLIC" "$PREBUILT_PUBLIC"; then
    echo 'FATAL: focused prebuilt source does not contain the exact diagnostic-only mutation' >&2
    exit 2
  fi
  diff_rc=0
  diff -u "$PREBUILT_BUILD/public.rs.original" "$PREBUILT_PUBLIC" >"$BUILD/public.rs.patch.check" || diff_rc=$?
  if [ "$diff_rc" -ne 1 ] || ! cmp -s "$BUILD/public.rs.patch.check" "$PREBUILT_BUILD/public.rs.patch"; then
    echo 'FATAL: focused prebuilt public.rs mutation differs from its retained patch' >&2
    exit 2
  fi
  receipt_value() {
    awk -F= -v key="$2" '$1 == key { value = $2; count++ } END { if (count != 1) exit 1; print value }' "$1"
  }
  PREBUILT_CLI_SHA="$(cat "$PREBUILT_BUILD/diagnostic-cli.sha256" 2>/dev/null)"
  PREBUILT_CLI_VERSION="$(cat "$PREBUILT_BUILD/diagnostic-cli.version" 2>/dev/null)"
  PREBUILT_RECEIPT_CLI_SHA="$(receipt_value "$PREBUILT_RECEIPT" cli_sha256)" || exit 2
  PREBUILT_RECEIPT_VERSION="$(receipt_value "$PREBUILT_RECEIPT" cli_version)" || exit 2
  PREBUILT_RECEIPT_SOURCE="$(receipt_value "$PREBUILT_RECEIPT" source_commit)" || exit 2
  PREBUILT_RECEIPT_ARCHIVE="$(receipt_value "$PREBUILT_RECEIPT" source_archive_sha256)" || exit 2
  PREBUILT_RECEIPT_MUTATED="$(receipt_value "$PREBUILT_RECEIPT" mutated_public_rs_sha256)" || exit 2
  PREBUILT_RECEIPT_TARGET="$(receipt_value "$PREBUILT_RECEIPT" target_directory)" || exit 2
  PREBUILT_RECEIPT_CLI="$(receipt_value "$PREBUILT_RECEIPT" cli_path)" || exit 2
  PREBUILT_RECEIPT_INPUT="$(receipt_value "$PREBUILT_RECEIPT" input_cli_sha256)" || exit 2
  PREBUILT_LINK_TARGET="$(cd "$PREBUILT_SOURCE/target" 2>/dev/null && pwd -P)" || exit 2
  case "$PREBUILT_TARGET" in
    */mbx/targets/*) ;;
    *) echo 'FATAL: focused candidate target is outside the MBX-managed target root' >&2; exit 2 ;;
  esac
  if [ ! -L "$PREBUILT_SOURCE/target" ] \
    || [ ! -d "$PREBUILT_TARGET" ] \
    || [ "$PREBUILT_LINK_TARGET" != "$PREBUILT_TARGET" ] \
    || [ "$PREBUILT_CLI" != "$PREBUILT_TARGET/release/velnor-actions" ] \
    || [ "$PREBUILT_RECEIPT_TARGET" != "$PREBUILT_TARGET" ] \
    || [ "$PREBUILT_RECEIPT_CLI" != "$PREBUILT_CLI" ] \
    || [ "$PREBUILT_CLI_SHA" != "$PREBUILT_RECEIPT_CLI_SHA" ] \
    || [ "$PREBUILT_RECEIPT_VERSION" != "$CLI_VERSION" ] \
    || [ "$PREBUILT_CLI_VERSION" != "$CLI_VERSION" ] \
    || [ "$PREBUILT_RECEIPT_SOURCE" != "$SOURCE_SHA" ] \
    || [ "$PREBUILT_RECEIPT_ARCHIVE" != "$ARCHIVE_SHA" ] \
    || [ "$PREBUILT_RECEIPT_MUTATED" != "$PREBUILT_MUTATED_SHA" ] \
    || [ "$PREBUILT_RECEIPT_INPUT" != "$CLI_SHA" ] \
    || [ "$(file_sha256 "$PREBUILT_CLI")" != "$PREBUILT_CLI_SHA" ]; then
    echo 'FATAL: focused candidate identity differs from its source, target mapping, or build receipt' >&2
    exit 2
  fi
  METADATA_TARGET="$(node - "$PREBUILT_BUILD/metadata.json" <<'NODE'
const fs = require('node:fs');
const value = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
if (typeof value.target_directory !== 'string') process.exit(2);
process.stdout.write(value.target_directory);
NODE
)" || exit 2
  if [ "$METADATA_TARGET" != "$PREBUILT_SOURCE/target" ] \
    || ! grep -Eq '^ *Finished .*release.*profile' "$PREBUILT_BUILD/build.log"; then
    echo 'FATAL: focused build metadata or completion log differs from the retained MBX candidate' >&2
    exit 2
  fi
  printf 'source_commit=%s\nsource_archive_sha256=%s\npublic_rs_original_sha256=%s\npublic_rs_mutated_sha256=%s\ntarget_directory=%s\ncli_path=%s\ncli_sha256=%s\ncli_version=%s\ninput_cli_sha256=%s\n' \
    "$SOURCE_SHA" "$ARCHIVE_SHA" "$ORIGINAL_GIT_SHA" "$PREBUILT_MUTATED_SHA" \
    "$PREBUILT_TARGET" "$PREBUILT_CLI" "$PREBUILT_CLI_SHA" "$PREBUILT_CLI_VERSION" "$CLI_SHA" \
    >"$BUILD/prebuilt-reuse.receipt" || exit 2
  SOURCE="$PREBUILT_SOURCE"
  DIAGNOSTIC_CLI="$PREBUILT_CLI"
  DIAGNOSTIC_SHA="$PREBUILT_CLI_SHA"
  DIAGNOSTIC_VERSION="$PREBUILT_CLI_VERSION"
  cp "$PREBUILT_BUILD/build.log" "$BUILD/build.log" || exit 2
  cp "$PREBUILT_BUILD/build.exit" "$BUILD/build.exit" || exit 2
  cp "$PREBUILT_BUILD/build.command" "$BUILD/build.command" || exit 2
  cp "$PREBUILT_BUILD/metadata.json" "$BUILD/metadata.json" || exit 2
  cp "$PREBUILT_BUILD/metadata.stderr" "$BUILD/metadata.stderr" || exit 2
  cp "$PREBUILT_BUILD/metadata.exit" "$BUILD/metadata.exit" || exit 2
  cp "$PREBUILT_BUILD/source.tar.sha256" "$BUILD/source.tar.sha256" || exit 2
  cp "$PREBUILT_BUILD/public.rs.original.sha256" "$BUILD/public.rs.original.sha256" || exit 2
  cp "$PREBUILT_BUILD/public.rs.mutated.sha256" "$BUILD/public.rs.mutated.sha256" || exit 2
else
  SOURCE="$BUILD/source"
  mkdir -m 700 "$SOURCE" || exit 2
if ! git -C "$ROOT" archive --format=tar "$SOURCE_SHA" >"$BUILD/source.tar"; then
  echo 'FATAL: could not export exact source commit for diagnostic CLI' >&2
  exit 2
fi
if ! tar -xf "$BUILD/source.tar" -C "$SOURCE"; then
  echo 'FATAL: could not extract exact source commit for diagnostic CLI' >&2
  exit 2
fi
PUBLIC="$SOURCE/crates/velnor-actions-cli/src/dispatch/public.rs"
if [ ! -f "$PUBLIC" ] || ! cp "$PUBLIC" "$BUILD/public.rs.original"; then
  echo 'FATAL: could not preserve diagnostic fixture source input' >&2
  exit 2
fi
if ! apply_diagnostic_source_mutation "$PUBLIC"; then
  echo 'FATAL: could not apply the exact diagnostic-only CLI source mutation' >&2
  exit 2
fi

patch_rc=0
diff -u "$BUILD/public.rs.original" "$PUBLIC" >"$BUILD/public.rs.patch" || patch_rc=$?
if [ "$patch_rc" -ne 1 ]; then
  echo "FATAL: diagnostic source patch did not produce one expected diff (exit $patch_rc)" >&2
  exit 2
fi
file_sha256 "$BUILD/source.tar" >"$BUILD/source.tar.sha256" || exit 2
file_sha256 "$BUILD/public.rs.original" >"$BUILD/public.rs.original.sha256" || exit 2
file_sha256 "$PUBLIC" >"$BUILD/public.rs.mutated.sha256" || exit 2

HOSTED=0
if [ "${GITHUB_ACTIONS:-}" = true ]; then
  HOSTED=1
  RUST_VERSION="${VELNOR_RUST_VERSION:-}"
  MBX_VERSION="${VELNOR_MBX_VERSION:-}"
  if [ -z "$RUST_VERSION" ] || [ -z "$MBX_VERSION" ]; then
    echo 'FATAL: pinned hosted Rust and MBX versions are unavailable' >&2
    exit 2
  fi
  if [ "$RUST_VERSION" != "$PINNED_RUST_VERSION" ]; then
    echo 'FATAL: hosted Rust build pin differs from the owned workspace request pin' >&2
    exit 2
  fi
  printf 'env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET MISE_NO_CONFIG=1 MBX_GC_AUTO=0 mise --no-config --no-env --no-hooks exec rust@%s mr-boxington@%s -- mbx build --release --locked -p velnor-actions-cli\n' \
    "$RUST_VERSION" "$MBX_VERSION" >"$BUILD/build.command"
  (
    cd "$SOURCE" || exit 2
    env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET MISE_NO_CONFIG=1 MBX_GC_AUTO=0 \
      mise --no-config --no-env --no-hooks exec "rust@$RUST_VERSION" "mr-boxington@$MBX_VERSION" -- \
      mbx build --release --locked -p velnor-actions-cli
  ) >"$BUILD/build.log" 2>&1
  build_rc=$?
else
  if [ -z "$GUARD" ]; then
    echo 'FATAL: set VELNOR_TEST_CARGO_GUARD to the required Rust build guard' >&2
    exit 2
  fi
  if ! git -C "$ROOT" show "$SOURCE_SHA:mise.toml" >"$BUILD/mise.toml.expected" \
    || ! cmp -s "$BUILD/mise.toml.expected" "$SOURCE/mise.toml"; then
    echo 'FATAL: exported mise configuration does not match the exact source commit' >&2
    exit 2
  fi
  file_sha256 "$SOURCE/mise.toml" >"$BUILD/mise.toml.sha256" || exit 2
  if mise trust "$SOURCE/mise.toml" >"$BUILD/mise-trust.log" 2>&1; then
    printf 'mise_trust_exit=0\ntrusted_mise_config=%s\ntrusted_mise_config_sha256=%s\n' \
      "$SOURCE/mise.toml" "$(cat "$BUILD/mise.toml.sha256")" >"$BUILD/mise-trust.receipt"
  else
    trust_rc=$?
    printf 'mise_trust_exit=%s\ntrusted_mise_config=%s\ntrusted_mise_config_sha256=%s\n' \
      "$trust_rc" "$SOURCE/mise.toml" "$(cat "$BUILD/mise.toml.sha256")" >"$BUILD/mise-trust.receipt"
    cat "$BUILD/mise-trust.log" >&2
    exit 2
  fi
  printf 'env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET MBX_GC_AUTO=0 %s -- mise exec -- cargo build --release --locked -p velnor-actions-cli\n' \
    "$GUARD" >"$BUILD/build.command"
  (
    cd "$SOURCE" || exit 2
    env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET MBX_GC_AUTO=0 \
      "$GUARD" -- mise exec -- cargo build --release --locked -p velnor-actions-cli
  ) >"$BUILD/build.log" 2>&1
  build_rc=$?
fi
printf '%s\n' "$build_rc" >"$BUILD/build.exit"
if [ "$build_rc" -ne 0 ]; then
  cat "$BUILD/build.log" >&2
  echo "FATAL: diagnostic CLI build failed with exit $build_rc; evidence: $BUILD" >&2
  exit 2
fi

if [ "$HOSTED" = 1 ]; then
  printf 'env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET MISE_NO_CONFIG=1 MBX_GC_AUTO=0 mise --no-config --no-env --no-hooks exec rust@%s mr-boxington@%s -- mbx metadata --format-version 1 --no-deps --manifest-path Cargo.toml\n' \
    "$RUST_VERSION" "$MBX_VERSION" >"$BUILD/metadata.command"
  (
    cd "$SOURCE" || exit 2
    env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET MISE_NO_CONFIG=1 MBX_GC_AUTO=0 \
      mise --no-config --no-env --no-hooks exec "rust@$RUST_VERSION" "mr-boxington@$MBX_VERSION" -- \
      mbx metadata --format-version 1 --no-deps --manifest-path Cargo.toml
  ) >"$BUILD/metadata.json" 2>"$BUILD/metadata.stderr"
  metadata_rc=$?
else
  printf 'env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET MBX_GC_AUTO=0 %s -- mise exec -- cargo metadata --format-version 1 --no-deps --manifest-path Cargo.toml\n' \
    "$GUARD" >"$BUILD/metadata.command"
  (
    cd "$SOURCE" || exit 2
    env -u CARGO_TARGET_DIR -u CARGO_BUILD_TARGET MBX_GC_AUTO=0 \
      "$GUARD" -- mise exec -- cargo metadata --format-version 1 --no-deps --manifest-path Cargo.toml
  ) >"$BUILD/metadata.json" 2>"$BUILD/metadata.stderr"
  metadata_rc=$?
fi
printf '%s\n' "$metadata_rc" >"$BUILD/metadata.exit"
if [ "$metadata_rc" -ne 0 ]; then
  cat "$BUILD/metadata.stderr" >&2
  echo "FATAL: guarded diagnostic target metadata failed with exit $metadata_rc; evidence: $BUILD" >&2
  exit 2
fi
fi
TARGET_DIR_LEXICAL="$(node - "$BUILD/metadata.json" <<'NODE'
const fs = require('node:fs');
const value = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
if (typeof value.target_directory !== 'string' || !value.target_directory.startsWith('/')) process.exit(2);
process.stdout.write(value.target_directory);
NODE
)" || { echo 'FATAL: diagnostic metadata did not identify an absolute target directory' >&2; exit 2; }
if [ "$TARGET_DIR_LEXICAL" != "$SOURCE/target" ] || [ ! -L "$TARGET_DIR_LEXICAL" ]; then
  echo "FATAL: diagnostic target is not the archive workspace's MBX-managed target link: $TARGET_DIR_LEXICAL" >&2
  exit 2
fi
TARGET_DIR="$(cd "$TARGET_DIR_LEXICAL" 2>/dev/null && pwd -P)" || { echo 'FATAL: diagnostic target directory could not be resolved' >&2; exit 2; }
case "$TARGET_DIR" in "$OUTPUT"|"$OUTPUT"/*|"$SOURCE"|"$SOURCE"/*)
  echo "FATAL: diagnostic build target resolves inside evidence source: $TARGET_DIR" >&2
  exit 2
  ;;
esac
DIAGNOSTIC_CLI="$TARGET_DIR/release/velnor-actions"
if [ ! -f "$DIAGNOSTIC_CLI" ] || [ ! -x "$DIAGNOSTIC_CLI" ]; then
  echo "FATAL: diagnostic CLI build succeeded without producing $DIAGNOSTIC_CLI" >&2
  exit 2
fi
DIAGNOSTIC_SHA="$(file_sha256 "$DIAGNOSTIC_CLI")" || exit 2
DIAGNOSTIC_VERSION="$(run_qualification_candidate "$DIAGNOSTIC_CLI" --version | awk 'NR == 1 && NF == 2 && $1 == "velnor-actions" { print $2; next } { exit 1 } END { if (NR != 1) exit 1 }')" || exit 2
if [ "$DIAGNOSTIC_VERSION" != "$CLI_VERSION" ]; then
  echo "FATAL: diagnostic CLI version $DIAGNOSTIC_VERSION differs from supplied CLI version $CLI_VERSION" >&2
  exit 2
fi
if [ "$(file_sha256 "$CLI")" != "$CLI_SHA" ]; then
  echo 'FATAL: diagnostic build changed the supplied CLI binary' >&2
  exit 2
fi
printf '%s\n' "$TARGET_DIR" >"$BUILD/target-directory.physical"
printf '%s\n' "$DIAGNOSTIC_CLI" >"$BUILD/diagnostic-cli.path"
printf '%s\n' "$DIAGNOSTIC_SHA" >"$BUILD/diagnostic-cli.sha256"
printf '%s\n' "$DIAGNOSTIC_VERSION" >"$BUILD/diagnostic-cli.version"
printf 'source_commit=%s\nsource_archive_sha256=%s\nmutated_public_rs_sha256=%s\ntarget_directory=%s\ncli_path=%s\ncli_sha256=%s\ncli_version=%s\ninput_cli_sha256=%s\n' \
  "$SOURCE_SHA" "$(cat "$BUILD/source.tar.sha256")" "$(cat "$BUILD/public.rs.mutated.sha256")" \
  "$TARGET_DIR" "$DIAGNOSTIC_CLI" "$DIAGNOSTIC_SHA" "$DIAGNOSTIC_VERSION" "$CLI_SHA" >"$BUILD/receipt.txt"
cp "$BUILD/build.log" "$OUTPUT/diagnostic-build.log" || exit 2
cp "$BUILD/build.exit" "$OUTPUT/diagnostic-build.exit" || exit 2
cp "$BUILD/build.command" "$OUTPUT/diagnostic-build.command.txt" || exit 2
cp "$BUILD/receipt.txt" "$OUTPUT/diagnostic-build.receipt.txt" || exit 2
cp "$BUILD/metadata.stderr" "$OUTPUT/diagnostic-metadata.stderr.txt" || exit 2
cp "$BUILD/metadata.exit" "$OUTPUT/diagnostic-metadata.exit" || exit 2
printf '%s\n' "$DIAGNOSTIC_CLI" >"$OUTPUT/diagnostic-cli.path.txt"
printf '%s\n' "$DIAGNOSTIC_SHA" >"$OUTPUT/diagnostic-cli.sha256.txt"
printf '%s\n' "$SOURCE_SHA" >"$OUTPUT/diagnostic-source.commit.txt"

STUB_BIN="$OUTPUT/stub-bin"
MARKERS="$OUTPUT/guard-markers"
CARGO_BUILD_MARKER="$OUTPUT/cargo-build-invoked"
MISE_ADAPTER_BIN="$OUTPUT/mise-adapter-bin"
mkdir -m 700 "$STUB_BIN" "$MARKERS" || exit 2
cat >"$STUB_BIN/cargo" <<'STUB'
#!/bin/sh
if [ "${1-}" = "build" ]; then
  printf invoked >"$VELNOR_TEST_CARGO_MARKER"
  exit 79
fi
printf 'cargo-invocation=%s\n' "$*" >>"$VELNOR_TEST_CARGO_RECEIPT"
PATH="$VELNOR_TEST_REAL_CARGO_PATH"
export PATH
if [ -n "${VELNOR_TEST_CARGO_GUARD:-}" ]; then
  "$VELNOR_TEST_CARGO_GUARD" -- mise exec -- cargo "$@"
  guard_rc=$?
  printf 'guard-exit=%s\n' "$guard_rc" >>"$VELNOR_TEST_CARGO_RECEIPT"
  exit "$guard_rc"
fi
exit 79
STUB
chmod u+x "$STUB_BIN/cargo" || exit 2

if ! test_mise_resolve_cargo "$REAL_MISE" "$PINNED_RUST_VERSION" \
  "$OUTPUT/mise-cargo-resolution" "$STUB_BIN/cargo"; then
  exit 2
fi
MISE_CARGO_PATH="$TEST_MISE_CARGO_PATH"
test_mise_adapter_create "$MISE_ADAPTER_BIN" "$REAL_MISE" "$ORIGINAL_PATH" \
  "$GUARD" "$PINNED_RUST_VERSION" || exit 2

verify_guard_receipt() {
  local label="$1" marker="$2" direct_receipt="$3"
  if ! test_mise_adapter_verify "$label" "$marker" "$PINNED_RUST_VERSION" "$GUARD"; then
    return 1
  fi
  if [ -s "$direct_receipt" ] && [ -n "$GUARD" ] && ! awk '
      /^cargo-invocation=/ { requested++ }
      /^guard-exit=/ { completed++; if ($0 != "guard-exit=0") failed=1 }
      /^build-blocked$/ { failed=1 }
      END { if (requested == 0 || requested != completed || failed) exit 1 }
    ' "$direct_receipt"; then
    cat "$direct_receipt" >&2
    echo "FAIL: direct collector Cargo request lacked a successful guard receipt: $label" >&2
    return 1
  fi
  if [ -s "$direct_receipt" ] && [ -z "$GUARD" ]; then
    cat "$direct_receipt" >&2
    echo "FAIL: collector bypassed real Mise metadata routing: $label" >&2
    return 1
  fi
}

printf '%s\n' "$PINNED_RUST_VERSION" >"$OUTPUT/mise-rust-version.txt"

GOLDEN_DIR="$ROOT/docs/proposed/opentofu-goldens"
if [ -n "${VELNOR_GOLDEN_DIR:-}" ]; then
  CALLER_DIR="$(pwd -P)" || { echo 'FATAL: could not resolve caller directory' >&2; exit 2; }
  case "$VELNOR_GOLDEN_DIR" in
    /*) requested_golden_dir="$VELNOR_GOLDEN_DIR" ;;
    *) requested_golden_dir="$CALLER_DIR/$VELNOR_GOLDEN_DIR" ;;
  esac
  golden_name="${requested_golden_dir##*/}"
  golden_parent="${requested_golden_dir%/*}"
  if [ -z "$golden_name" ] || [ "$golden_name" = . ] || [ "$golden_name" = .. ] \
    || ! golden_parent="$(cd -- "$golden_parent" 2>/dev/null && pwd -P)"; then
    echo 'FATAL: external golden directory parent must exist' >&2
    exit 2
  fi
  GOLDEN_DIR="$golden_parent/$golden_name"
  case "$GOLDEN_DIR" in
    "$ROOT"|"$ROOT"/*)
      echo 'FATAL: VELNOR_GOLDEN_DIR must be outside the repository' >&2
      exit 2
      ;;
  esac
fi
if [ -L "$GOLDEN_DIR" ] || [ ! -d "$GOLDEN_DIR" ]; then
  echo 'FATAL: selected external plan diagnostic oracle must be an existing regular directory' >&2
  exit 2
fi
printf '%s\n' "$GOLDEN_DIR" >"$OUTPUT/selected-oracle.path.txt" || exit 2
BEFORE="$(golden_fingerprint "$GOLDEN_DIR")" || exit 2
printf '%s\n' "$BEFORE" >"$OUTPUT/selected-oracle.fingerprint.txt" || exit 2
EMPTY_ORACLE="$OUTPUT/oracle-empty"
EXPECTED_ORACLE="$OUTPUT/oracle-warning"
mkdir -m 700 "$EMPTY_ORACLE" || exit 2
cp -RP "$GOLDEN_DIR/." "$EMPTY_ORACLE/" || exit 2
if [ "$(golden_fingerprint "$EMPTY_ORACLE")" != "$BEFORE" ]; then
  echo 'FATAL: copied empty plan oracle differs from the selected external oracle' >&2
  exit 2
fi
for case in nested mbx-nextest empty-suite minimal-cargo dogfood; do
  if [ -s "$GOLDEN_DIR/cases/$case/plan.stderr.txt" ]; then
    echo "FATAL: selected plan stderr oracle is not empty for $case" >&2
    exit 2
  fi
done
mkdir -m 700 "$EXPECTED_ORACLE" || exit 2
cp -RP "$EMPTY_ORACLE/." "$EXPECTED_ORACLE/" || exit 2
for case in nested mbx-nextest empty-suite minimal-cargo dogfood; do
  if [ "$case" = dogfood ]; then
    normalized='plan-diagnostic-warning repository=<root> source-root=<root> work=<work>'
  else
    normalized='plan-diagnostic-warning repository=<repo> source-root=<root> work=<work>'
  fi
  printf '%s\n' "$normalized" >"$EXPECTED_ORACLE/cases/$case/plan.stderr.txt" || exit 2
done

diagnostic_workspace_from_log() {
  local label="$1" log="$OUTPUT/$1.log" tmp_root workspace physical_tmp
  workspace="$(sed -n \
    -e 's/^retaining successful golden evidence workspace: //p' \
    -e 's/^retaining failed golden workspace: //p' "$log" | tail -1)"
  if [ -z "$workspace" ] || [ ! -d "$workspace" ]; then
    echo "FAIL: collector did not retain its evidence workspace: $log" >&2
    return 1
  fi
  tmp_root="$(cat "$OUTPUT/$label.tmp-root.txt")" || return 1
  physical_tmp="$(cd "$tmp_root" 2>/dev/null && pwd -P)" || return 1
  case "$workspace" in
    "$physical_tmp"/velnor-goldens-work.*) ;;
    *) echo "FAIL: collector workspace escaped its scenario TMPDIR: $workspace" >&2; return 1 ;;
  esac
  printf '%s\n' "$workspace" >"$OUTPUT/$label.workpath.txt" || return 1
  printf '%s\n' "$workspace"
}

preserve_raw_plan_evidence() {
  local label="$1" workspace workspace_base workspace_parent physical_tmp raw case_name case_dir source destination
  workspace="$(sed -n \
    -e 's/^retaining successful golden evidence workspace: //p' \
    -e 's/^retaining failed golden workspace: //p' "$OUTPUT/$label.log" | tail -n 1)"
  if [ -z "$workspace" ]; then return 0; fi
  workspace="$(diagnostic_workspace_from_log "$label")" || return 1
  workspace_base="${workspace##*/}"
  if [[ ! "$workspace_base" =~ ^velnor-goldens-work\.[A-Za-z0-9]+$ ]] \
    || [ -L "$workspace" ] || [ ! -d "$workspace" ]; then
    printf 'unsafe retained golden workspace: %s\n' "$workspace" >&2
    return 1
  fi
  workspace_parent="$(cd "$workspace/.." 2>/dev/null && pwd -P)" || return 1
  physical_tmp="$(cd "$(cat "$OUTPUT/$label.tmp-root.txt")" 2>/dev/null && pwd -P)" || return 1
  if [ "$workspace_parent" != "$physical_tmp" ] \
    || [ "$(cd "$workspace" && pwd -P)" != "$workspace" ]; then
    printf 'retained golden workspace is outside physical TMPDIR: %s\n' "$workspace" >&2
    return 1
  fi
  raw="$workspace/raw"
  if [ -L "$raw" ]; then
    echo "unsafe retained raw diagnostics directory: $raw" >&2
    return 1
  fi
  if [ ! -e "$raw" ]; then return 0; fi
  if [ ! -d "$raw" ]; then
    printf 'retained raw diagnostics path is not a directory: %s\n' "$raw" >&2
    return 1
  fi
  for case_name in nested mbx-nextest empty-suite minimal-cargo dogfood; do
    case_dir="$raw/$case_name"
    if [ -L "$case_dir" ]; then
      printf 'unsafe retained diagnostics case directory: %s\n' "$case_dir" >&2
      return 1
    fi
    if [ ! -e "$case_dir" ]; then continue; fi
    if [ ! -d "$case_dir" ]; then
      printf 'retained diagnostics case path is not a directory: %s\n' "$case_dir" >&2
      return 1
    fi
    source="$case_dir/plan.stderr.raw.txt"
    if [ -L "$source" ]; then
      printf 'unsafe retained plan diagnostics file: %s\n' "$source" >&2
      return 1
    fi
    if [ ! -e "$source" ]; then continue; fi
    if [ ! -f "$source" ]; then
      printf 'retained plan diagnostics path is not a regular file: %s\n' "$source" >&2
      return 1
    fi
    destination="$OUTPUT/$label-$case_name-plan.stderr.raw.txt"
    if [ -e "$destination" ] || [ -L "$destination" ] || ! cp -p "$source" "$destination"; then
      printf 'could not safely preserve retained plan diagnostics: %s\n' "$source" >&2
      return 1
    fi
  done
}

run_collector() {
  local label="$1" mode="$2" oracle="$3" spaced="$4" log="$OUTPUT/$1.log"
  local receipt="$MARKERS/$label" tmp_root="$OUTPUT/tmp/$label" temp_bin="$DIAGNOSTIC_CLI" temp_dir="$OUTPUT/tmp/$label/TMPDIR with spaces"
  local mise_receipt="$MARKERS/$label.mise"
  local collector_rc=0
  if [ "$label" = baseline ]; then temp_bin="$CLI"; fi
  mkdir -p "$tmp_root" || return 2
  if [ "$spaced" = 1 ]; then
    temp_dir="$tmp_root/actual temporary root"
    mkdir -m 700 "$temp_dir" || return 2
    local symlink_tmp="$tmp_root/TMPDIR symlink"
    ln -s "$temp_dir" "$symlink_tmp" || return 2
    temp_dir="$symlink_tmp"
    local spaced_dir="$tmp_root/space path"
    mkdir -m 700 "$spaced_dir" || return 2
    temp_bin="$spaced_dir/velnor-actions"
    ln -s "$DIAGNOSTIC_CLI" "$temp_bin" || return 2
    if [ "$(file_sha256 "$temp_bin")" != "$DIAGNOSTIC_SHA" ]; then
      echo 'FAIL: spaced diagnostic CLI path changed binary identity' >&2
      return 1
    fi
  else
    mkdir -m 700 "$temp_dir" || return 2
  fi
  printf '%s\n' "$temp_dir" >"$OUTPUT/$label.tmp-root.txt" || return 2
  : >"$receipt" || return 2
  : >"$mise_receipt" || return 2
  rm -f "$CARGO_BUILD_MARKER"
  printf 'TMPDIR=%s\nCLI=%s\nmode=%s\n' "$temp_dir" "$temp_bin" "$mode" >"$OUTPUT/$label.command.txt"
  (
    unset CARGO_TARGET_DIR CARGO_BUILD_TARGET
    export PATH="$STUB_BIN:$MISE_ADAPTER_BIN:$ORIGINAL_PATH"
    export MBX_GC_AUTO=0
    export TMPDIR="$temp_dir"
    export VELNOR_GOLDEN_DIR="$oracle"
    export VELNOR_GOLDEN_KEEP_WORK=1
    export VELNOR_TEST_CARGO_MARKER="$CARGO_BUILD_MARKER"
    export VELNOR_TEST_CARGO_RECEIPT="$receipt"
    export VELNOR_TEST_REAL_CARGO_PATH="$ORIGINAL_PATH"
    export VELNOR_TEST_CARGO_GUARD="$GUARD"
    export VELNOR_TEST_REAL_MISE="$REAL_MISE"
    export VELNOR_TEST_ORIGINAL_PATH="$ORIGINAL_PATH"
    export VELNOR_TEST_RUST_VERSION="$PINNED_RUST_VERSION"
    export VELNOR_TEST_MISE_CARGO_PATH="$MISE_CARGO_PATH"
    export VELNOR_TEST_MISE_RECEIPT="$mise_receipt"
    export VELNOR_TEST_PLAN_SOURCE_ROOT="$ROOT"
    if [ -n "$mode" ]; then export VELNOR_TEST_PLAN_DIAGNOSTIC_MODE="$mode"; else unset VELNOR_TEST_PLAN_DIAGNOSTIC_MODE; fi
    bash "$ROOT/scripts/capture-opentofu-goldens.sh" "${ARCHIVE_GUARD_ARGS[@]}" check "$temp_bin"
  ) >"$log" 2>&1 || collector_rc=$?
  printf '%s\n' "$collector_rc" >"$OUTPUT/$label.exit"
  if ! preserve_raw_plan_evidence "$label"; then return 2; fi
  if ! verify_guard_receipt "$label" "$mise_receipt" "$receipt"; then
    cat "$log" >&2
    return 2
  fi
  if [ -e "$CARGO_BUILD_MARKER" ]; then
    cat "$log" >&2
    echo "FAIL: collector invoked an implicit cargo build: $label" >&2
    return 2
  fi
  DIAGNOSTIC_LAST_LOG="$log"
  DIAGNOSTIC_LAST_RC="$collector_rc"
}

assert_plan_outputs() {
  local label="$1" mode="$2" work case repo raw staged plan_exit expected_raw expected_normalized should_warn failed=0 expected_raw_file expected_normalized_file
  work="$(diagnostic_workspace_from_log "$label")" || return 1
  for case in nested mbx-nextest empty-suite minimal-cargo dogfood; do
    if [ "$case" = dogfood ]; then repo="$ROOT"; else repo="$work/$case"; fi
    raw="$work/raw/$case/plan.stderr.raw.txt"
    staged="$work/stage/$case/plan.stderr.txt"
    plan_exit="$work/stage/$case/plan.exit"
    if [ ! -f "$raw" ] || [ ! -f "$staged" ] || [ ! -f "$plan_exit" ]; then
      echo "FAIL: collector did not retain raw and staged plan evidence for $label/$case" >&2
      failed=1
      continue
    fi
    cp "$raw" "$OUTPUT/$label.$case.plan.stderr.raw.txt" || { failed=1; continue; }
    cp "$staged" "$OUTPUT/$label.$case.plan.stderr.txt" || { failed=1; continue; }
    cp "$plan_exit" "$OUTPUT/$label.$case.plan.exit" || { failed=1; continue; }
    if [ "$(cat "$plan_exit")" != 0 ]; then
      echo "FAIL: successful diagnostic plan returned nonzero for $label/$case" >&2
      failed=1
      continue
    fi
    should_warn=0
    case "$mode:$case" in
      warning-all:*|fail57:*) should_warn=1 ;;
      nested-only:nested|dogfood-only:dogfood) should_warn=1 ;;
    esac
    if [ "$should_warn" = 1 ]; then
      if [ "$case" = dogfood ]; then
        expected_raw="plan-diagnostic-warning repository=$ROOT source-root=$ROOT work=$work"
        expected_normalized='plan-diagnostic-warning repository=<root> source-root=<root> work=<work>'
      else
        expected_raw="plan-diagnostic-warning repository=$repo source-root=$ROOT work=$work"
        expected_normalized='plan-diagnostic-warning repository=<repo> source-root=<root> work=<work>'
      fi
      expected_raw_file="$OUTPUT/$label.$case.expected-plan.stderr.raw.txt"
      expected_normalized_file="$OUTPUT/$label.$case.expected-plan.stderr.normalized.txt"
      printf '%s\n' "$expected_raw" >"$expected_raw_file"
      printf '%s\n' "$expected_normalized" >"$expected_normalized_file"
      if ! cmp -s "$expected_raw_file" "$raw" \
        || ! cmp -s "$expected_normalized_file" "$staged"; then
        echo "FAIL: raw and normalized plan warning bytes differ for $label/$case" >&2
        failed=1
      fi
    elif [ -s "$raw" ] || [ -s "$staged" ]; then
      echo "FAIL: unaffected plan emitted diagnostic stderr for $label/$case" >&2
      failed=1
    fi
  done
  return "$failed"
}

assert_mismatch_scope() {
  local label="$1" expected_case="$2" work="$3" mismatch_count diff_count diff_header
  mismatch_count="$(grep -Fc 'FAIL: golden mismatch:' "$OUTPUT/$label.log" || true)"
  if [ "$mismatch_count" != 1 ] || ! grep -Fq "FAIL: golden mismatch: $expected_case " "$OUTPUT/$label.log"; then
    cat "$OUTPUT/$label.log" >&2
    echo "FAIL: collector mismatch scope differs from $expected_case for $label" >&2
    return 1
  fi
  diff_header="$work/check-diff/$expected_case.diff"
  diff_count="$(grep -c '^diff -r ' "$diff_header" || true)"
  if [ "$diff_count" != 1 ] || ! grep -Fq '/plan.stderr.txt' "$diff_header"; then
    cat "$diff_header" >&2
    echo "FAIL: $label mismatch included output beyond plan.stderr.txt" >&2
    return 1
  fi
}

BEHAVIOR_FAILED=0
: >"$OUTPUT/behavior-failures.log"
record_behavior_failure() {
  printf '%s\n' "$*" >>"$OUTPUT/behavior-failures.log"
  printf 'FAIL: %s\n' "$*" >&2
  BEHAVIOR_FAILED=1
}

run_collector baseline '' "$EMPTY_ORACLE" 0 || exit $?
if [ "$DIAGNOSTIC_LAST_RC" -ne 0 ] || ! grep -Fq 'ALL GOLDENS MATCH' "$DIAGNOSTIC_LAST_LOG"; then
  cat "$DIAGNOSTIC_LAST_LOG" >&2
  echo 'FAIL: supplied CLI does not pass the copied empty plan stderr oracle' >&2
  exit 1
fi
assert_plan_outputs baseline quiet >/dev/null || exit $?

run_collector quiet quiet "$EMPTY_ORACLE" 0 || exit $?
if [ "$DIAGNOSTIC_LAST_RC" -ne 0 ] || ! grep -Fq 'ALL GOLDENS MATCH' "$DIAGNOSTIC_LAST_LOG"; then
  cat "$DIAGNOSTIC_LAST_LOG" >&2
  echo 'FAIL: quiet compiled CLI does not pass the copied empty plan stderr oracle' >&2
  exit 1
fi
assert_plan_outputs quiet quiet >/dev/null || exit $?

run_collector warning-empty warning-all "$EMPTY_ORACLE" 0 || exit $?
warning_work="$(diagnostic_workspace_from_log warning-empty)" || exit 2
if ! assert_plan_outputs warning-empty warning-all >/dev/null; then
  record_behavior_failure 'warning-empty raw/staged warning bytes did not match every scenario'
fi
if [ "$DIAGNOSTIC_LAST_RC" -ne 1 ] \
  || [ "$(grep -Fc 'FAIL: golden mismatch:' "$DIAGNOSTIC_LAST_LOG" || true)" != 5 ]; then
  record_behavior_failure 'warning-empty did not produce five plan-stderr mismatches'
fi
for case in nested mbx-nextest empty-suite minimal-cargo dogfood; do
  diff_header="$warning_work/check-diff/$case.diff"
  if [ "$(grep -c '^diff -r ' "$diff_header" || true)" != 1 ] \
    || ! grep -Fq '/plan.stderr.txt' "$diff_header"; then
    record_behavior_failure "warning-all changed output beyond plan.stderr.txt for $case"
  fi
done

run_collector warning-expected warning-all "$EXPECTED_ORACLE" 0 || exit $?
if [ "$DIAGNOSTIC_LAST_RC" -ne 0 ] || ! grep -Fq 'ALL GOLDENS MATCH' "$DIAGNOSTIC_LAST_LOG"; then
  record_behavior_failure 'warning-all did not match the hand-authored external oracle'
fi
if ! assert_plan_outputs warning-expected warning-all >/dev/null; then
  record_behavior_failure 'warning-expected raw/staged warning bytes did not match every scenario'
fi

run_collector nested-only nested-only "$EMPTY_ORACLE" 0 || exit $?
 if ! assert_plan_outputs nested-only nested-only >/dev/null; then
  record_behavior_failure 'nested-only raw/staged warning bytes did not match the intended nested case'
fi
nested_work="$(diagnostic_workspace_from_log nested-only)" || exit 2
if [ "$DIAGNOSTIC_LAST_RC" -ne 1 ]; then
  record_behavior_failure 'nested-only warning did not produce one collector mismatch'
fi
if ! assert_mismatch_scope nested-only nested "$nested_work"; then
  record_behavior_failure 'nested-only mismatch scope did not isolate the nested plan stderr'
fi

run_collector dogfood-only dogfood-only "$EMPTY_ORACLE" 0 || exit $?
if ! assert_plan_outputs dogfood-only dogfood-only >/dev/null; then
  record_behavior_failure 'dogfood-only raw/staged warning bytes did not match the intended dogfood case'
fi
dogfood_work="$(diagnostic_workspace_from_log dogfood-only)" || exit 2
if [ "$DIAGNOSTIC_LAST_RC" -ne 1 ]; then
  record_behavior_failure 'dogfood-only warning did not produce one collector mismatch'
fi
if ! assert_mismatch_scope dogfood-only dogfood "$dogfood_work"; then
  record_behavior_failure 'dogfood-only mismatch scope did not isolate the dogfood plan stderr'
fi

run_collector warning-spaced-symlink warning-all "$EXPECTED_ORACLE" 1 || exit $?
if [ "$DIAGNOSTIC_LAST_RC" -ne 0 ] || ! grep -Fq 'ALL GOLDENS MATCH' "$DIAGNOSTIC_LAST_LOG"; then
  record_behavior_failure 'warning candidate failed with spaced executable or symlinked TMPDIR'
fi
if ! assert_plan_outputs warning-spaced-symlink warning-all >/dev/null; then
  record_behavior_failure 'warning candidate with spaced executable did not preserve exact diagnostics'
fi

run_collector failure57 fail57 "$EMPTY_ORACLE" 0 || exit $?
failure_work="$(diagnostic_workspace_from_log failure57)" || exit $?
if [ "$DIAGNOSTIC_LAST_RC" -ne 2 ] \
  || ! grep -Fq 'FATAL: plan failed for nested' "$DIAGNOSTIC_LAST_LOG" \
  || [ "$(cat "$failure_work/stage/nested/plan.exit")" != 57 ]; then
  record_behavior_failure 'nonzero plan exit was not retained and reported'
fi
printf 'plan-diagnostic-warning repository=%s source-root=%s work=%s\n' \
  "$failure_work/nested" "$ROOT" "$failure_work" >"$OUTPUT/expected-failure.raw"
printf 'plan-diagnostic-warning repository=<repo> source-root=<root> work=<work>\n' \
  >"$OUTPUT/expected-failure.normalized"
if ! cmp -s "$OUTPUT/expected-failure.raw" "$failure_work/raw/nested/plan.stderr.raw.txt" \
  || ! cmp -s "$OUTPUT/expected-failure.normalized" "$failure_work/stage/nested/plan.stderr.txt"; then
  record_behavior_failure 'fail57 did not retain exact raw and normalized warning bytes'
fi
cp "$failure_work/raw/nested/plan.stderr.raw.txt" "$OUTPUT/failure57.nested.plan.stderr.raw.txt" || exit 2
cp "$failure_work/stage/nested/plan.stderr.txt" "$OUTPUT/failure57.nested.plan.stderr.txt" || exit 2
cp "$failure_work/stage/nested/plan.exit" "$OUTPUT/failure57.nested.plan.exit" || exit 2

AFTER="$(golden_fingerprint "$GOLDEN_DIR")" || exit 2
if [ "$AFTER" != "$BEFORE" ]; then
  echo 'FAIL: diagnostic tests changed the checked-in golden oracle' >&2
  exit 1
fi
printf 'baseline=%s quiet=%s warning_empty=%s warning_expected=%s nested_only=%s dogfood_only=%s spaced_symlink=%s fail57=%s\n' \
  "$(cat "$OUTPUT/baseline.exit")" "$(cat "$OUTPUT/quiet.exit")" \
  "$(cat "$OUTPUT/warning-empty.exit")" "$(cat "$OUTPUT/warning-expected.exit")" \
  "$(cat "$OUTPUT/nested-only.exit")" "$(cat "$OUTPUT/dogfood-only.exit")" \
  "$(cat "$OUTPUT/warning-spaced-symlink.exit")" "$(cat "$OUTPUT/failure57.exit")" \
  >"$OUTPUT/result.txt"
if [ "$BEHAVIOR_FAILED" -ne 0 ]; then
  echo 'FAIL: one or more plan diagnostic behavior assertions failed' >&2
  exit 1
fi
echo 'passed compiled plan stderr baseline, warning rejection, exact normalization, scoped warnings, symlink paths, and exit 57'
