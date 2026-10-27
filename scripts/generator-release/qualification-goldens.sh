#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Candidate release manifest verification and qualification parity helpers.

source "$ROOT/scripts/generator-release/fixture-git.sh"

MAX_CANDIDATE_MANIFEST_BYTES=8388608
QUALIFICATION_GUARD_BIN="$ROOT/.velnor/archive-guard/bin/velnor-archive-guard"
QUALIFICATION_GUARD_INPUT_PATH="${QUALIFICATION_GUARD_INPUT_PATH:-}"
QUALIFICATION_GUARD_INPUT_SHA256="${QUALIFICATION_GUARD_INPUT_SHA256:-}"
QUALIFICATION_GUARD_READY=0
QUALIFICATION_GUARD_SHA256=''
QUALIFICATION_GUARD_IDENTITY=''
QUALIFICATION_GUARD_FINGERPRINT=''

test_mise_rust_version() {
  local tool_version file_version
  tool_version="$(awk -F '"' '
    /^\[tools\][[:space:]]*$/ { in_tools = 1; next }
    /^\[/ { in_tools = 0 }
    in_tools && /^[[:space:]]*rust[[:space:]]*=[[:space:]]*\{/ {
      if (NF < 3 || $2 == "") { failed = 1; next }
      version = $2
      count++
    }
    END {
      if (failed || count != 1) exit 1
      print version
    }
  ' "$ROOT/mise.toml")" || return 1
  file_version="$(awk -F '"' '
    /^[[:space:]]*channel[[:space:]]*=/ {
      if (NF < 3 || $2 == "") { failed = 1; next }
      version = $2
      count++
    }
    END {
      if (failed || count != 1) exit 1
      print version
    }
  ' "$ROOT/rust-toolchain.toml")" || return 1
  [[ "$tool_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ \
    && "$tool_version" == "$file_version" ]] || return 1
  printf '%s\n' "$tool_version"
}

test_mise_resolve_cargo() {
  local real_mise="$1" rust_version="$2" prefix="$3" stub_cargo="$4"
  local rust_spec="rust@$rust_version" resolution_rc=0 cargo_path
  printf '%s --no-config --no-env --no-hooks which cargo --tool %s\n' \
    "$real_mise" "$rust_spec" >"$prefix.command.txt" || return 1
  env MISE_NO_CONFIG=1 MISE_NO_ENV=1 MISE_NO_HOOKS=1 MISE_LOCKFILE=0 \
    MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    "$real_mise" --no-config --no-env --no-hooks which cargo --tool "$rust_spec" \
    >"$prefix.stdout.txt" 2>"$prefix.stderr.txt" \
    || resolution_rc=$?
  printf '%s\n' "$resolution_rc" >"$prefix.exit" || return 1
  if [ "$resolution_rc" -ne 0 ] \
    || ! awk 'NR == 1 { if ($0 !~ /^\//) exit 1; next } END { if (NR != 1) exit 1 }' \
      "$prefix.stdout.txt"; then
    cat "$prefix.stderr.txt" >&2
    echo 'FATAL: real Mise could not resolve the pinned Rust Cargo executable' >&2
    return 1
  fi
  cargo_path="$(cat "$prefix.stdout.txt")" || return 1
  if [ ! -x "$cargo_path" ] || { [ -n "$stub_cargo" ] && [ "$cargo_path" = "$stub_cargo" ]; }; then
    echo "FATAL: real Mise resolved an invalid Cargo executable: $cargo_path" >&2
    return 1
  fi
  TEST_MISE_CARGO_PATH="$cargo_path"
  export TEST_MISE_CARGO_PATH
}

test_mise_adapter_create() {
  local adapter_dir="$1" real_mise="$2" original_path="$3" guard="$4" rust_version="$5"
  if [[ "$adapter_dir" != /* || "$real_mise" != /* || ! -x "$real_mise" \
    || -z "$original_path" || ! "$rust_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo 'FATAL: test Mise adapter inputs are invalid' >&2
    return 1
  fi
  if [[ -n "$guard" && ( "$guard" != /* || ! -x "$guard" ) ]]; then
    echo 'FATAL: test Mise adapter guard path is invalid' >&2
    return 1
  fi
  mkdir -p "$adapter_dir" || return 1
  adapter_dir="$(cd "$adapter_dir" && pwd -P)" || return 1
  cat >"$adapter_dir/mise" <<'ADAPTER'
#!/usr/bin/env bash
set -uo pipefail

REAL_MISE="${VELNOR_TEST_REAL_MISE:-}"
ORIGINAL_PATH="${VELNOR_TEST_ORIGINAL_PATH:-}"
RUST_VERSION="${VELNOR_TEST_RUST_VERSION:-}"
RUST_SPEC="rust@$RUST_VERSION"
GUARD="${VELNOR_TEST_CARGO_GUARD:-}"
RECEIPT="${VELNOR_TEST_MISE_RECEIPT:-}"
CARGO_PATH="${VELNOR_TEST_MISE_CARGO_PATH:-}"

if [[ "$REAL_MISE" != /* || ! -x "$REAL_MISE" || -z "$ORIGINAL_PATH" ]]; then
  echo 'FATAL: test Mise adapter has no absolute real Mise path or original PATH' >&2
  exit 125
fi

arguments=("$@")
payload_index=-1
for ((arg_index = 0; arg_index < ${#arguments[@]}; arg_index++)); do
  if [[ "${arguments[$arg_index]}" == exec ]]; then
    for ((separator_index = arg_index + 1; separator_index < ${#arguments[@]}; separator_index++)); do
      if [[ "${arguments[$separator_index]}" == -- ]]; then
        payload_index=$((separator_index + 1))
        break
      fi
    done
    break
  fi
done

cargo_request=0
if (( payload_index >= 0 && payload_index < ${#arguments[@]} )); then
  payload_program="${arguments[$payload_index]}"
  [[ "${payload_program##*/}" == cargo ]] && cargo_request=1
fi

PATH="$ORIGINAL_PATH"
export PATH
if [ "$cargo_request" -eq 0 ]; then
  exec "$REAL_MISE" "$@"
fi

if [ -z "$RECEIPT" ] || [ ! -f "$RECEIPT" ]; then
  echo 'FATAL: test Mise adapter received Cargo without a receipt path' >&2
  exit 125
fi

request_pid=$$
record_arguments() {
  local name="$1" argument_index=0
  shift
  for argument in "$@"; do
    printf '%s-argv[%s]=%s\n' "$name" "$argument_index" "$argument" >>"$RECEIPT"
    argument_index=$((argument_index + 1))
  done
}

printf 'metadata-request-begin pid=%s\n' "$request_pid" >>"$RECEIPT"
record_arguments original "$@"

reject_request() {
  local reason="$1"
  printf 'metadata-request-rejected pid=%s reason=%s\n' "$request_pid" "$reason" >>"$RECEIPT"
  printf 'request-exit pid=%s code=78\n' "$request_pid" >>"$RECEIPT"
  printf 'metadata-request-end pid=%s\n' "$request_pid" >>"$RECEIPT"
  echo "FATAL: test Mise adapter rejected Cargo request: $reason" >&2
  exit 78
}

if [ "$#" -ne 13 ] && [ "$#" -ne 15 ]; then
  reject_request 'unsupported-argv-count'
fi
if [[ "${arguments[0]}" != --no-config || "${arguments[1]}" != --no-env \
  || "${arguments[2]}" != --no-hooks || "${arguments[3]}" != exec \
  || "${arguments[5]}" != -- || "${arguments[6]}" != cargo \
  || "${arguments[7]}" != metadata || "${arguments[8]}" != --format-version \
  || "${arguments[9]}" != 1 ]]; then
  reject_request 'unsupported-mise-or-metadata-argv'
fi

rust_spec="${arguments[4]}"
if [[ ! "$rust_spec" =~ ^rust@[0-9]+\.[0-9]+\.[0-9]+$ || "$rust_spec" != "$RUST_SPEC" ]]; then
  reject_request 'rust-spec-mismatch'
fi
printf 'rust-spec pid=%s value=%s\n' "$request_pid" "$rust_spec" >>"$RECEIPT"

if [ "$#" -eq 13 ]; then
  if [[ "${arguments[10]}" != --no-deps || "${arguments[11]}" != --manifest-path ]]; then
    reject_request 'unsupported-metadata-options'
  fi
  manifest_path="${arguments[12]}"
  metadata_shape=discovery
else
  if [[ "${arguments[10]}" != --locked || "${arguments[11]}" != --offline \
    || "${arguments[12]}" != --all-features || "${arguments[13]}" != --manifest-path ]]; then
    reject_request 'unsupported-metadata-options'
  fi
  manifest_path="${arguments[14]}"
  metadata_shape=qualification
fi

original_cwd="$(pwd -P)" || reject_request 'original-cwd-unavailable'
if [ ! -x "$CARGO_PATH" ]; then
  reject_request 'real-mise-cargo-path-unavailable'
fi

identity_rc=0
rustc_identity="$("$REAL_MISE" --no-config --no-env --no-hooks exec "$rust_spec" -- rustc --version 2>&1)" \
  || identity_rc=$?
if [ "$identity_rc" -ne 0 ] \
  || [[ ! "$rustc_identity" =~ ^rustc[[:space:]]([0-9]+\.[0-9]+\.[0-9]+)([[:space:]]|$) ]] \
  || [ "${BASH_REMATCH[1]:-}" != "$RUST_VERSION" ]; then
  printf 'rustc-identity pid=%s output=%s\n' "$request_pid" "$rustc_identity" >>"$RECEIPT"
  printf 'request-exit pid=%s code=78\n' "$request_pid" >>"$RECEIPT"
  printf 'metadata-request-end pid=%s\n' "$request_pid" >>"$RECEIPT"
  echo 'FATAL: test Mise adapter Rust toolchain identity differs from the requested pin' >&2
  exit 78
fi
printf 'rustc-version pid=%s value=%s\n' "$request_pid" "${BASH_REMATCH[1]}" >>"$RECEIPT"
printf 'rustc-identity pid=%s output=%s\n' "$request_pid" "$rustc_identity" >>"$RECEIPT"
printf 'real-cargo-path pid=%s value=%s\n' "$request_pid" "$CARGO_PATH" >>"$RECEIPT"
printf 'original-cwd pid=%s value=%s\n' "$request_pid" "$original_cwd" >>"$RECEIPT"
printf 'manifest-original pid=%s value=%s\n' "$request_pid" "$manifest_path" >>"$RECEIPT"
printf 'metadata-shape pid=%s value=%s\n' "$request_pid" "$metadata_shape" >>"$RECEIPT"

if [ -z "$GUARD" ]; then
  "$REAL_MISE" "$@"
  request_rc=$?
  printf 'request-exit pid=%s code=%s\n' "$request_pid" "$request_rc" >>"$RECEIPT"
  printf 'metadata-request-end pid=%s\n' "$request_pid" >>"$RECEIPT"
  exit "$request_rc"
fi

if [[ "$manifest_path" != /* || "${manifest_path##*/}" != Cargo.toml \
  || ! -f "$manifest_path" || -L "$manifest_path" ]]; then
  reject_request 'manifest-is-not-absolute-regular-Cargo.toml'
fi
manifest_parent="$(cd "$(dirname "$manifest_path")" 2>/dev/null && pwd -P)" \
  || reject_request 'manifest-parent-unavailable'
manifest_translated="$manifest_parent/Cargo.toml"
if [ ! -f "$manifest_translated" ] || [ -L "$manifest_translated" ]; then
  reject_request 'translated-manifest-unavailable'
fi
if [ "$original_cwd" != "$manifest_parent" ]; then
  case "$original_cwd" in
    /) ;;
    *) [[ "$manifest_parent" == "$original_cwd"/* ]] || reject_request 'manifest-parent-is-not-below-original-cwd' ;;
  esac
  config_dir="$manifest_parent"
  while [ "$config_dir" != "$original_cwd" ]; do
    if [ -e "$config_dir/.cargo/config" ] || [ -L "$config_dir/.cargo/config" ] \
      || [ -e "$config_dir/.cargo/config.toml" ] || [ -L "$config_dir/.cargo/config.toml" ]; then
      reject_request 'manifest-parent-cwd-would-change-cargo-config'
    fi
    config_dir="$(dirname "$config_dir")"
  done
fi
printf 'manifest-parent pid=%s value=%s\n' "$request_pid" "$manifest_parent" >>"$RECEIPT"

translated=(--no-config --no-env --no-hooks exec "$rust_spec" -- "$GUARD" -- mise exec -- cargo metadata --format-version 1)
if [ "$metadata_shape" = discovery ]; then
  translated+=(--no-deps --manifest-path Cargo.toml)
else
  translated+=(--locked --offline --all-features --manifest-path Cargo.toml)
fi
printf 'translated-cwd pid=%s value=%s\n' "$request_pid" "$manifest_parent" >>"$RECEIPT"
record_arguments translated "${translated[@]}"

(cd "$manifest_parent" && "$REAL_MISE" "${translated[@]}")
metadata_rc=$?
printf 'guarded-metadata-exit pid=%s code=%s\n' "$request_pid" "$metadata_rc" >>"$RECEIPT"
printf 'request-exit pid=%s code=%s\n' "$request_pid" "$metadata_rc" >>"$RECEIPT"
printf 'metadata-request-end pid=%s\n' "$request_pid" >>"$RECEIPT"
exit "$metadata_rc"
ADAPTER
  chmod u+x "$adapter_dir/mise" || return 1
  TEST_MISE_ADAPTER_BIN="$adapter_dir"
  TEST_MISE_REAL_MISE="$real_mise"
  TEST_MISE_ORIGINAL_PATH="$original_path"
  TEST_MISE_RUST_VERSION="$rust_version"
  TEST_MISE_GUARD="$guard"
  export TEST_MISE_ADAPTER_BIN TEST_MISE_REAL_MISE TEST_MISE_ORIGINAL_PATH TEST_MISE_RUST_VERSION TEST_MISE_GUARD
}

test_mise_adapter_verify() {
  local label="$1" receipt="$2" rust_version="$3" guard="$4"
  if [ ! -s "$receipt" ] || ! awk -v expected_version="$rust_version" -v guard_required="$([ -n "$guard" ] && echo 1 || echo 0)" '
    function field(name, field_number, prefix) {
      prefix = name "="
      for (field_number = 1; field_number <= NF; field_number++) {
        if (index($field_number, prefix) == 1) return substr($field_number, length(prefix) + 1)
      }
      return ""
    }
    /^metadata-request-begin / {
      id = field("pid")
      if (id == "" || started[id]++) failed = 1
      requests++
    }
    /^metadata-request-rejected / { failed = 1 }
    /^rust-spec / {
      id = field("pid")
      if (!started[id] || field("value") != "rust@" expected_version) failed = 1
      specs[id]++
    }
    /^rustc-version / {
      id = field("pid")
      if (!started[id] || field("value") != expected_version) failed = 1
      identities[id]++
    }
    /^guarded-metadata-exit / {
      id = field("pid")
      if (!started[id] || field("code") != "0") failed = 1
      guards[id]++
    }
    /^request-exit / {
      id = field("pid")
      if (!started[id] || field("code") != "0") failed = 1
      completions[id]++
    }
    /^metadata-request-end / {
      id = field("pid")
      if (!started[id]) failed = 1
      endings[id]++
    }
    END {
      if (requests == 0) failed = 1
      for (id in started) {
        if (specs[id] != 1 || identities[id] != 1 || completions[id] != 1 || endings[id] != 1) failed = 1
        if (guard_required && guards[id] != 1) failed = 1
        if (!guard_required && guards[id] != 0) failed = 1
      }
      if (failed) exit 1
    }
  ' "$receipt"; then
    cat "$receipt" >&2
    echo "FAIL: test Mise adapter did not complete a guarded metadata request for $label" >&2
    return 1
  fi
}

candidate_tree_oid() {
  if ! git -C "$ROOT" diff --quiet --; then
    echo "FATAL: dogfood candidate has unstaged tracked changes" >&2
    return 1
  fi
  git -C "$ROOT" write-tree
}

file_sha256() {
  local path="$1" output
  if command -v sha256sum >/dev/null 2>&1; then
    output="$(sha256sum "$path")" || return 1
  elif command -v shasum >/dev/null 2>&1; then
    output="$(shasum -a 256 "$path")" || return 1
  else
    echo "FATAL: no SHA-256 utility is available" >&2
    return 1
  fi
  printf '%s\n' "$output" | awk 'NR == 1 && length($1) == 64 && $1 !~ /[^0-9a-f]/ { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }'
}

file_size_bytes() {
  local path="$1" size
  case "$path" in
    /*) ;;
    *) path="./$path" ;;
  esac
  if size="$(stat -c '%s' -- "$path" 2>/dev/null)"; then
    :
  elif size="$(stat -f '%z' "$path" 2>/dev/null)"; then
    :
  else
    return 1
  fi
  [[ "$size" =~ ^[0-9]+$ ]] || return 1
  printf '%s\n' "$size"
}

json_has_unique_keys() {
  local path="$1"
  prepare_qualification_guard || return 1
  verify_qualification_guard || return 1
  if ! "$QUALIFICATION_GUARD_BIN" qualification-manifest "$path"; then
    return 1
  fi
  verify_qualification_guard
}

qualification_guard_identity() {
  if [[ "$(uname -s)" == Darwin ]]; then
    stat -f '%d:%i:%l' "$QUALIFICATION_GUARD_BIN"
  else
    stat -c '%d:%i:%h' "$QUALIFICATION_GUARD_BIN"
  fi
}

stat_identity_for_guard_parent() {
  if [[ "$(uname -s)" == Darwin ]]; then
    stat -f '%u %Lp' "$1"
  else
    stat -c '%u %a' "$1"
  fi
}

admit_qualification_guard_input() {
  local path="$QUALIFICATION_GUARD_INPUT_PATH" expected="$QUALIFICATION_GUARD_INPUT_SHA256"
  local parent basename physical_parent identity owner mode links actual_sha
  if [[ -z "$path" || -z "$expected" || ! "$path" == /* \
    || ! "$expected" =~ ^[0-9a-f]{64}$ ]]; then
    echo "FATAL: explicit archive guard path and lowercase SHA-256 are required" >&2
    return 1
  fi
  basename="${path##*/}"
  parent="${path%/*}"
  [[ -n "$parent" ]] || parent="/"
  if [[ -z "$basename" || ! -d "$parent" || -L "$parent" \
    || ! -f "$path" || -L "$path" || ! -x "$path" ]]; then
    echo "FATAL: explicit archive guard must be a regular executable in a private directory" >&2
    return 1
  fi
  if ! physical_parent="$(cd -- "$parent" 2>/dev/null && pwd -P)" \
    || [[ "$physical_parent/$basename" != "$path" ]]; then
    echo "FATAL: explicit archive guard path must be canonical" >&2
    return 1
  fi
  if [[ "$(uname -s)" == Darwin ]]; then
    identity="$(stat -f '%u %Lp %l' "$path")" || return 1
  else
    identity="$(stat -c '%u %a %h' "$path")" || return 1
  fi
  read -r owner mode links <<<"$identity"
  if [[ "$owner" != "$(id -u)" || ! "$mode" =~ ^[0-7]+$ || "$links" != 1 ]] \
    || (( (8#$mode & 022) != 0 || (8#$mode & 0111) == 0 )); then
    echo "FATAL: explicit archive guard ownership, permissions, or link count are unsafe" >&2
    return 1
  fi
  read -r owner mode <<<"$(stat_identity_for_guard_parent "$parent")" || return 1
  if [[ "$owner" != "$(id -u)" || ! "$mode" =~ ^[0-7]+$ ]] \
    || (( (8#$mode & 077) != 0 )); then
    echo "FATAL: explicit archive guard parent must be private to the current user" >&2
    return 1
  fi
  if ! actual_sha="$(file_sha256 "$path")" || [[ "$actual_sha" != "$expected" ]]; then
    echo "FATAL: explicit archive guard SHA-256 does not match supplied value" >&2
    return 1
  fi
  QUALIFICATION_GUARD_BIN="$path"
  QUALIFICATION_GUARD_SHA256="$expected"
  export QUALIFICATION_GUARD_INPUT_PATH QUALIFICATION_GUARD_INPUT_SHA256
}

prepare_qualification_guard() {
  local compiled_fingerprint source_fingerprint
  if [[ "$QUALIFICATION_GUARD_READY" == 1 ]]; then
    return 0
  fi
  if [[ -n "$QUALIFICATION_GUARD_INPUT_PATH" || -n "$QUALIFICATION_GUARD_INPUT_SHA256" ]]; then
    admit_qualification_guard_input || return 1
  elif ! bash "$ROOT/scripts/build-owned-archive-guard.sh" >/dev/null; then
    echo "FATAL: could not provision trusted qualification archive guard" >&2
    return 1
  fi
  if [[ ! -f "$QUALIFICATION_GUARD_BIN" || -L "$QUALIFICATION_GUARD_BIN" \
    || ! -x "$QUALIFICATION_GUARD_BIN" ]]; then
    echo "FATAL: trusted qualification archive guard is missing or unsafe" >&2
    return 1
  fi
  if ! compiled_fingerprint="$("$QUALIFICATION_GUARD_BIN" --fingerprint)" \
    || ! source_fingerprint="$("$QUALIFICATION_GUARD_BIN" source-fingerprint "$ROOT")" \
    || [[ ! "$compiled_fingerprint" =~ ^[0-9a-f]{64}$ \
      || "$source_fingerprint" != "$compiled_fingerprint" ]]; then
    echo "FATAL: trusted qualification archive guard source fingerprint is stale" >&2
    return 1
  fi
  if ! QUALIFICATION_GUARD_SHA256="$(file_sha256 "$QUALIFICATION_GUARD_BIN")" \
    || ! QUALIFICATION_GUARD_IDENTITY="$(qualification_guard_identity)"; then
    echo "FATAL: could not bind trusted qualification archive guard identity" >&2
    return 1
  fi
  QUALIFICATION_GUARD_FINGERPRINT="$compiled_fingerprint"
  QUALIFICATION_GUARD_READY=1
}

verify_qualification_guard() {
  local actual_sha identity compiled_fingerprint source_fingerprint
  if [[ "$QUALIFICATION_GUARD_READY" != 1 \
    || ! -f "$QUALIFICATION_GUARD_BIN" || -L "$QUALIFICATION_GUARD_BIN" \
    || ! -x "$QUALIFICATION_GUARD_BIN" ]]; then
    echo "FATAL: trusted qualification archive guard identity is unavailable" >&2
    return 1
  fi
  if ! actual_sha="$(file_sha256 "$QUALIFICATION_GUARD_BIN")" \
    || ! identity="$(qualification_guard_identity)" \
    || [[ "$actual_sha" != "$QUALIFICATION_GUARD_SHA256" \
      || "$identity" != "$QUALIFICATION_GUARD_IDENTITY" ]]; then
    echo "FATAL: trusted qualification archive guard changed during candidate execution" >&2
    return 1
  fi
  if ! compiled_fingerprint="$("$QUALIFICATION_GUARD_BIN" --fingerprint)" \
    || ! source_fingerprint="$("$QUALIFICATION_GUARD_BIN" source-fingerprint "$ROOT")" \
    || [[ "$compiled_fingerprint" != "$QUALIFICATION_GUARD_FINGERPRINT" \
      || "$source_fingerprint" != "$QUALIFICATION_GUARD_FINGERPRINT" ]]; then
    echo "FATAL: trusted qualification archive guard source fingerprint changed" >&2
    return 1
  fi
}

run_qualification_candidate() {
  local status=0
  prepare_qualification_guard || return 2
  verify_qualification_guard || return 2
  "$@" || status=$?
  verify_qualification_guard || return 2
  return "$status"
}

host_target() {
  case "$(uname -s):$(uname -m)" in
    Linux:x86_64|Linux:amd64) echo "x86_64-unknown-linux-gnu" ;;
    Darwin:arm64|Darwin:aarch64) echo "aarch64-apple-darwin" ;;
    Darwin:x86_64|Darwin:amd64) echo "x86_64-apple-darwin" ;;
    *) return 1 ;;
  esac
}

prepare_fixture_consumer_manifest() {
  local repo="$1" binary="$2" binary_target="${3:-}" manifest="$1/.velnor/release-manifest.json"
  local version target commit digest host_digest fixture_digest fixture_char targets_json host
  if [ -e "$manifest" ] || [ -L "$manifest" ]; then
    [ -f "$manifest" ] && [ ! -L "$manifest" ]
    return $?
  fi
  if [ -L "$repo/.velnor" ] || { [ -e "$repo/.velnor" ] && [ ! -d "$repo/.velnor" ]; }; then
    return 1
  fi
  mkdir -p "$repo/.velnor" || return 1
  if ! version="$(run_qualification_candidate "$binary" --version | awk 'NR == 1 && NF == 2 && $1 == "velnor-actions" { print $2; next } { exit 1 } END { if (NR != 1) exit 1 }')" \
    || [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    return 1
  fi
  commit="$(printf '%040d' 0 | tr '0' 'b')"
  if [ -n "$binary_target" ]; then
    case "$binary_target" in
      x86_64-unknown-linux-gnu|aarch64-apple-darwin|x86_64-apple-darwin) host="$binary_target" ;;
      *) return 1 ;;
    esac
  elif ! host="$(host_target)"; then
    return 1
  fi
  if ! host_digest="$(file_sha256 "$binary")"; then
    return 1
  fi
  for fixture_char in a b c d e f 0 1; do
    digest="$(printf '%064d' 0 | tr '0' "$fixture_char")"
    if [ "$digest" != "$host_digest" ]; then
      fixture_digest="$digest"
      break
    fi
  done
  if [ -z "$fixture_digest" ]; then
    return 1
  fi
  targets_json=""
  for target in x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-apple-darwin; do
    if [ -n "$targets_json" ]; then targets_json+=","; fi
    if [ "$target" = "$host" ]; then
      digest="$host_digest"
    else
      digest="$fixture_digest"
    fi
    targets_json+="{\"target\":\"$target\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v$version/velnor-actions-$version-$target\",\"sha256\":\"$digest\"}"
  done
  printf '{"schema":1,"version":"%s","repository":"tailrocks/velnor-new","commit":"%s","targets":[%s]}\n' \
    "$version" "$commit" "$targets_json" >"$manifest"
}

validate_candidate_manifest() {
  local target version source_sha checkout_sha candidate_sha manifest_sha manifest_file_sha manifest_size
  local manifest_parent manifest_name physical_manifest_parent
  if [ ! -f "$CANDIDATE_MANIFEST" ] || [ -L "$CANDIDATE_MANIFEST" ]; then
    echo "FATAL: candidate manifest must be a regular non-symlink file: $CANDIDATE_MANIFEST" >&2
    exit 2
  fi
  manifest_parent="${CANDIDATE_MANIFEST%/*}"
  manifest_name="${CANDIDATE_MANIFEST##*/}"
  if [ -z "$manifest_parent" ]; then
    manifest_parent="/"
  fi
  if [ -z "$manifest_name" ] \
    || ! physical_manifest_parent="$(cd -- "$manifest_parent" 2>/dev/null && pwd -P)"; then
    echo "FATAL: could not resolve candidate manifest directory: $CANDIDATE_MANIFEST" >&2
    exit 2
  fi
  CANDIDATE_MANIFEST="$physical_manifest_parent/$manifest_name"
  if ! manifest_size="$(file_size_bytes "$CANDIDATE_MANIFEST")" \
    || (( manifest_size > MAX_CANDIDATE_MANIFEST_BYTES )); then
    echo "FATAL: candidate manifest must not exceed $MAX_CANDIDATE_MANIFEST_BYTES bytes" >&2
    exit 2
  fi
  if [[ ! "$CANDIDATE_MANIFEST_SHA256" =~ ^[0-9a-f]{64}$ ]]; then
    echo "FATAL: expected candidate manifest SHA-256 must be 64 lowercase hexadecimal characters" >&2
    exit 2
  fi
  if ! manifest_file_sha="$(file_sha256 "$CANDIDATE_MANIFEST")"; then
    echo "FATAL: could not compute candidate manifest SHA-256" >&2
    exit 2
  fi
  if [ "$manifest_file_sha" != "$CANDIDATE_MANIFEST_SHA256" ]; then
    echo "FATAL: candidate manifest bytes do not match expected SHA-256" >&2
    exit 2
  fi
  if ! jq -e . "$CANDIDATE_MANIFEST" >/dev/null 2>&1; then
    echo "FATAL: candidate manifest is malformed JSON" >&2
    exit 2
  fi
  if ! json_has_unique_keys "$CANDIDATE_MANIFEST"; then
    echo "FATAL: candidate manifest contains duplicate JSON keys" >&2
    exit 2
  fi
  if [ "${GITHUB_REPOSITORY-unset}" != "tailrocks/velnor-new" ]; then
    echo "FATAL: GITHUB_REPOSITORY must be tailrocks/velnor-new" >&2
    exit 2
  fi
  source_sha="${GITHUB_SHA:-}"
  if [[ ! "$source_sha" =~ ^[0-9a-f]{40}$ ]]; then
    echo "FATAL: GITHUB_SHA must be a lowercase 40-character source SHA" >&2
    exit 2
  fi
  if ! checkout_sha="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null)" || [ "$checkout_sha" != "$source_sha" ]; then
    echo "FATAL: checked-out source does not match GITHUB_SHA" >&2
    exit 2
  fi
  if ! manifest_sha="$(jq -er '.commit | strings' "$CANDIDATE_MANIFEST" 2>/dev/null)" \
    || [ "$manifest_sha" != "$source_sha" ]; then
    echo "FATAL: candidate manifest source does not match GITHUB_SHA" >&2
    exit 2
  fi
  if ! prepare_qualification_guard; then
    echo "FATAL: trusted qualification archive guard is unavailable" >&2
    exit 2
  fi
  if ! version="$(run_qualification_candidate "$BIN" --version | awk 'NR == 1 && NF == 2 && $1 == "velnor-actions" { print $2; next } { exit 1 } END { if (NR != 1) exit 1 }')"; then
    echo "FATAL: could not read candidate CLI version" >&2
    exit 2
  fi
  if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "FATAL: candidate CLI version is malformed: $version" >&2
    exit 2
  fi
  if ! jq -e --arg version "$version" --arg repository "$GITHUB_REPOSITORY" '
    .schema == 1 and .version == $version and .repository == $repository and
    (.targets | length) == 3 and
    [.targets[].target] == ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"] and
    all(.targets[];
      (.sha256 | type == "string" and test("^[0-9a-f]{64}$")) and
      .artifact == ("https://github.com/" + $repository + "/releases/download/v" +
        $version + "/velnor-actions-" + $version + "-" + .target))
  ' "$CANDIDATE_MANIFEST" >/dev/null 2>&1; then
    echo "FATAL: candidate manifest does not match the release schema or CLI version" >&2
    exit 2
  fi
  if ! target="$(host_target)"; then
    echo "FATAL: candidate qualification does not support host $(uname -s)/$(uname -m)" >&2
    exit 2
  fi
  if ! candidate_sha="$(file_sha256 "$BIN")"; then
    echo "FATAL: could not compute candidate CLI SHA-256" >&2
    exit 2
  fi
  manifest_sha="$(jq -er --arg target "$target" '[.targets[] | select(.target == $target) | .sha256] | if length == 1 then .[0] else error("missing or duplicate host target") end' "$CANDIDATE_MANIFEST" 2>/dev/null)" \
    || { echo "FATAL: candidate manifest has no unique digest for $target" >&2; exit 2; }
  if [ "$manifest_sha" != "$candidate_sha" ]; then
    echo "FATAL: candidate manifest digest does not match candidate CLI for $target" >&2
    exit 2
  fi
}

stage_candidate_manifest() {
  local repo="$1" context="$2" staged_sha
  if [ ! -f "$CANDIDATE_MANIFEST" ] || [ -L "$CANDIDATE_MANIFEST" ]; then
    echo "FATAL: candidate manifest must be a regular non-symlink file: $CANDIDATE_MANIFEST" >&2
    exit 2
  fi
  if ! mkdir -p "$repo/.velnor" \
    || ! cp "$CANDIDATE_MANIFEST" "$repo/.velnor/release-manifest.json"; then
    echo "FATAL: could not stage exact candidate manifest bytes for $context" >&2
    exit 2
  fi
  if ! staged_sha="$(file_sha256 "$repo/.velnor/release-manifest.json")" \
    || [ "$staged_sha" != "$CANDIDATE_MANIFEST_SHA256" ] \
    || ! cmp -s "$CANDIDATE_MANIFEST" "$repo/.velnor/release-manifest.json"; then
    echo "FATAL: could not stage exact candidate manifest bytes for $context" >&2
    exit 2
  fi
}

capture_release_case() {
  local case="$1" repo="$2" out="$3" preview="$3/preview"
  mkdir -p "$out" "$preview"
  (cd "$repo" && run_qualification_candidate "$BIN" generate --output-dir "$preview" >"$out/stdout.txt" 2>"$out/stderr.txt") \
    || {
      emit_candidate_stderr "$out/stderr.txt"
      echo "FATAL: release candidate generate failed for $case"
      exit 2
    }
  if [ ! -d "$preview/.github" ]; then
    echo "FATAL: release candidate emitted no workflows for $case"
    exit 2
  fi
  if ! validate_candidate_output_bindings "$preview"; then
    echo "FATAL: release candidate output bindings do not match candidate manifest for $case" >&2
    exit 2
  fi
  if ! normalize_candidate_digests "$preview"; then
    echo "FATAL: could not normalize candidate asset digests for $case"
    exit 2
  fi
  hash_tree "$preview" "$out/tree.sha256"
}

emit_candidate_stderr() {
  local path="$1"
  if [ ! -f "$path" ] || [ -L "$path" ]; then
    echo '[candidate stderr] unavailable: captured stderr is not a regular file' >&2
    return 0
  fi
  if [ ! -s "$path" ]; then
    echo '[candidate stderr] empty' >&2
    return 0
  fi
  echo '[candidate stderr] sanitized output follows (maximum 16384 bytes)' >&2
  LC_ALL=C dd if="$path" bs=16384 count=1 2>/dev/null \
    | LC_ALL=C tr -d '\000-\011\013-\037\177' \
    | LC_ALL=C awk '
      BEGIN { remaining = 16256; prefix = "[candidate stderr] " }
      {
        line = $0
        gsub(/##\[/, "# #[", line)
        line = prefix line
        if (length(line) >= remaining) {
          printf "%s", substr(line, 1, remaining)
          exit
        }
        print line
        remaining -= length(line) + 1
        if (remaining <= 0) exit
      }
    ' >&2 || true
  return 0
}

validate_candidate_output_bindings() {
  local preview="$1"
  prepare_qualification_guard || return 1
  verify_qualification_guard || return 1
  if ! "$QUALIFICATION_GUARD_BIN" qualification-output-bindings \
    "$CANDIDATE_MANIFEST" "$preview/.github"; then
    return 1
  fi
  verify_qualification_guard
}

normalize_manifest_digests() {
  local manifest="$1" preview="$2" digest file normalized placeholder commit commit_placeholder
  placeholder="$(printf '%064d' 0 | tr '0' 'a')"
  commit_placeholder="$(printf '%040d' 0 | tr '0' 'b')"
  if [ ! -d "$preview/.github" ] || [ ! -f "$manifest" ] || [ -L "$manifest" ]; then
    echo "FATAL: manifest normalization requires a regular manifest and generated .github tree" >&2
    return 1
  fi
  if ! commit="$(jq -er '.commit | strings' "$manifest" 2>/dev/null)" \
    || [[ ! "$commit" =~ ^[0-9a-f]{40}$ ]]; then
    echo "FATAL: manifest contains a malformed source commit" >&2
    return 1
  fi
  while IFS= read -r digest; do
    if [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
      echo "FATAL: manifest contains a malformed asset digest" >&2
      return 1
    fi
    while IFS= read -r file; do
      normalized="$file.normalized"
      if ! sed -e "s/$digest/$placeholder/g" "$file" >"$normalized" \
        || ! mv "$normalized" "$file"; then
        rm -f "$normalized"
        echo "FATAL: could not normalize candidate manifest digest in $file" >&2
        return 1
      fi
    done < <(find "$preview/.github" -type f -print)
  done < <(jq -r '.targets[].sha256' "$manifest")
  while IFS= read -r file; do
    normalized="$file.normalized"
    if ! sed -e "s/$commit/$commit_placeholder/g" "$file" >"$normalized" \
      || ! mv "$normalized" "$file"; then
      rm -f "$normalized"
      echo "FATAL: could not normalize candidate manifest commit in $file" >&2
      return 1
    fi
  done < <(find "$preview/.github" -type f -print)
}

normalize_candidate_digests() {
  normalize_manifest_digests "$CANDIDATE_MANIFEST" "$1"
}

capture_release_dogfood() {
  local out="$1" repo="$WORK/dogfood-repo" preview="$1/preview" diff_status candidate_tree
  mkdir -p "$repo" "$out" "$preview"
  if ! candidate_tree="$(candidate_tree_oid)"; then
    echo "FATAL: could not identify staged dogfood candidate tree"
    exit 2
  fi
  if ! git -C "$ROOT" archive --format=tar "$candidate_tree" | tar -xf - -C "$repo"; then
    echo "FATAL: could not stage indexed dogfood source"
    exit 2
  fi
  stage_candidate_manifest "$repo" dogfood
  local git_log="$out/git-setup.log"
  (
    cd "$repo" \
      && fixture_git init -q \
      && fixture_git remote add origin https://github.com/tailrocks/velnor-new.git \
      && fixture_git add -A \
      && fixture_git commit -qm "golden"
  ) >"$git_log" 2>&1 || { cat "$git_log" >&2; echo "FATAL: dogfood git setup failed"; exit 2; }
  rm -f "$git_log"
  # Warm the cargo cache for both lockful dogfood workspaces ahead of the
  # candidate's locked/offline discovery (Gate 1): the qualify runner
  # starts with a virgin cache, so without this fetch the candidate's
  # `cargo metadata --locked --offline` fails with preparation_incomplete.
  # Same HOME/mise environment as the candidate run below, so the same
  # cache is warmed. `--locked` keeps a lock rewrite from slipping in.
  for manifest in "$repo/Cargo.toml" "$repo/crates/velnor-runner/Cargo.toml"; do
    if ! mise --no-config --no-env --no-hooks exec rust@1.99.1 -- cargo fetch --locked --manifest-path "$manifest"; then
      echo "FATAL: could not fetch dogfood cargo sources for $manifest"
      exit 2
    fi
  done
  if ! (cd "$repo" && run_qualification_candidate "$BIN" generate --output-dir "$preview" >"$out/stdout.txt" 2>"$out/stderr.txt"); then
    echo "FATAL: release candidate dogfood generate failed"
    exit 2
  fi
  if [ ! -d "$preview/.github" ]; then
    echo "FATAL: release candidate dogfood emitted no workflows"
    exit 2
  fi
  if (cd "$repo" && diff -r .github "$preview/.github") >"$out/dogfood.diff" 2>&1; then
    echo "identical" >"$out/dogfood.verdict"
  else
    diff_status=$?
    if [ "$diff_status" -eq 1 ]; then
      cat "$out/dogfood.diff" >&2
      echo "FATAL: release candidate dogfood differs from committed .github"
      exit 2
    fi
    echo "FATAL: dogfood tree comparison failed (diff status $diff_status)"
    exit 2
  fi
  if ! link_identity "$repo/.github" >"$out/committed.links" \
    || ! link_identity "$preview/.github" >"$out/generated.links" \
    || ! diff "$out/committed.links" "$out/generated.links" >/dev/null; then
    echo "FATAL: release candidate dogfood symlink tree differs from committed .github"
    exit 2
  fi
  hash_tree "$preview" "$out/tree.sha256"
  note "release dogfood parity: all committed .github files match"
}
