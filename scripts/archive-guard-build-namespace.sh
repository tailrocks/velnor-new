#!/usr/bin/env bash

archive_guard_source_fingerprint() {
  local fingerprint executable="${ARCHIVE_GUARD_BOOTSTRAP_BIN:-}"
  [[ -n "$executable" && "$executable" == /* && -x "$executable" && ! -L "$executable" ]] \
    || fail 'native archive guard bootstrap is unavailable'
  env -i HOME="$HOME" PATH=/usr/bin:/bin TMPDIR="$run_tmp" \
    "$executable" source-fingerprint "$repository" \
    >"$run_tmp/source-fingerprint.stdout" 2>"$run_tmp/source-fingerprint.stderr" \
    || fail 'cannot compute native archive guard source fingerprint'
  [[ ! -s "$run_tmp/source-fingerprint.stderr" ]] \
    || fail 'native source fingerprint wrote unexpected diagnostics'
  IFS= read -r fingerprint <"$run_tmp/source-fingerprint.stdout" \
    || fail 'native source fingerprint output is incomplete'
  [[ "$fingerprint" =~ ^[0-9a-f]{64}$ ]] \
    || fail 'archive guard source fingerprint is malformed'
  [[ "$(wc -c <"$run_tmp/source-fingerprint.stdout" | tr -d ' ')" == 65 ]] \
    || fail 'native source fingerprint output has unexpected bytes'
  printf '%s' "$fingerprint"
}

archive_guard_target_identity() {
  local rustc_verbose rustc_details rustc_release rustc_commit rustc_host llvm_version
  local cargo_identity platform_name platform_machine
  rustc_verbose="$(run_mise exec rust@1.99.1 -- rustc -vV)" \
    || fail 'cannot inspect the pinned Rust compiler'
  rustc_details="$(awk -F ': ' '
    $1 == "release" { release = $2; release_count++ }
    $1 == "commit-hash" { commit = $2; commit_count++ }
    $1 == "host" { host = $2; host_count++ }
    $1 == "LLVM version" { llvm = $2; llvm_count++ }
    END {
      if (release_count != 1 || commit_count != 1 || host_count != 1 || llvm_count != 1) exit 1
      printf "%s\t%s\t%s\t%s\n", release, commit, host, llvm
    }
  ' <<<"$rustc_verbose")" || fail 'cannot identify the pinned Rust compiler'
  IFS=$'\t' read -r rustc_release rustc_commit rustc_host llvm_version <<<"$rustc_details"
  cargo_identity="${cargo_version#cargo }"
  cargo_identity="${cargo_identity// /-}"
  cargo_identity="${cargo_identity//(/}"
  cargo_identity="${cargo_identity//)/}"
  platform_name="$(uname -s)"
  platform_machine="$(uname -m)"
  [[ "$rustc_release" == 1.99.1 && "$rustc_commit" =~ ^[A-Za-z0-9._-]+$ \
    && "$rustc_host" =~ ^[A-Za-z0-9._-]+$ && "$llvm_version" =~ ^[A-Za-z0-9._-]+$ \
    && "$cargo_identity" =~ ^[A-Za-z0-9._-]+$ && "$platform_name" =~ ^[A-Za-z0-9._-]+$ \
    && "$platform_machine" =~ ^[A-Za-z0-9._-]+$ ]] \
    || fail 'Rust, Cargo, or platform identity is malformed'
  printf '%s-%s-%s-rustc-%s-%s-llvm-%s-cargo-%s' "$platform_name" "$platform_machine" \
    "$rustc_host" "$rustc_release" "$rustc_commit" "$llvm_version" "$cargo_identity"
}

archive_guard_check_fingerprint() {
  local executable="$1" expected="$2"
  if ! env -i HOME="$HOME" PATH=/usr/bin:/bin TMPDIR="$run_tmp" \
    "$executable" --fingerprint >"$run_tmp/fingerprint.stdout" 2>"$run_tmp/fingerprint.stderr"; then
    fail 'archive guard fingerprint command failed'
  fi
  printf '%s\n' "$expected" >"$run_tmp/fingerprint.expected"
  cmp -s "$run_tmp/fingerprint.stdout" "$run_tmp/fingerprint.expected" \
    && [[ ! -s "$run_tmp/fingerprint.stderr" ]] \
    || fail 'compiled archive guard fingerprint differs from admitted source'
}
