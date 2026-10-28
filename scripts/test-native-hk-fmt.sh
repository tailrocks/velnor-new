#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

project_root=$(cd "$(dirname "$0")/.." && pwd -P)
if [[ -z "${HK_BIN:-}" || "$HK_BIN" != /* || ! -x "$HK_BIN" ]]; then
  printf '%s\n' 'Set HK_BIN to an installed absolute hk 2.8.0 executable.' >&2
  exit 1
fi
hk_bin=$HK_BIN
if [[ "$("$hk_bin" --version)" != 'hk 2.8.0' ]]; then
  printf '%s\n' 'This test requires hk 2.8.0.' >&2
  exit 1
fi
export CARGO_NET_OFFLINE=true MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false
export MISE_OFFLINE=true HK_PKL_OFFLINE=true

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/hk native fmt.XXXXXX")
tmp_root=$(cd "$tmp_root" && pwd -P)
test_phase='fixture setup'
cleanup() {
  status=$1
  if [[ $status -ne 0 ]]; then
    printf 'native hk fixture failed during: %s\n' "$test_phase" >&2
    for log in root-deny.log nested-deny.log guard-deny.log; do
      if [[ -s "$tmp_root/$log" ]]; then
        printf '%s\n' "--- $log ---" >&2
        cat "$tmp_root/$log" >&2
      fi
    done
  fi
  rm -rf "$tmp_root"
  exit "$status"
}
trap 'cleanup $?' EXIT
fixture="$tmp_root/repository with spaces"
mkdir -p "$fixture/crates/root member/src" \
  "$fixture/crates/velnor-runner/runner member/src"
cp "$project_root/hk.pkl" "$project_root/mise.toml" "$project_root/mise.lock" \
  "$project_root/rust-toolchain.toml" "$project_root/.mise-version" "$fixture/"
cp -R "$project_root/scripts" "$fixture/scripts"
export MISE_TRUSTED_CONFIG_PATHS="$fixture"
git -C "$fixture" init -q -b main
git -C "$fixture" config user.name 'native hk fmt test'
git -C "$fixture" config user.email 'native-hk-fmt@example.invalid'

cat >"$fixture/Cargo.toml" <<'TOML'
[workspace]
members = ["crates/root member"]
exclude = ["crates/velnor-runner"]
resolver = "3"
TOML
cat >"$fixture/crates/root member/Cargo.toml" <<'TOML'
[package]
name = "root-member-fixture"
version = "0.1.0"
edition = "2024"
TOML
cat >"$fixture/crates/velnor-runner/Cargo.toml" <<'TOML'
[workspace]
members = ["runner member"]
resolver = "3"
TOML
cat >"$fixture/crates/velnor-runner/runner member/Cargo.toml" <<'TOML'
[package]
name = "runner-member-fixture"
version = "0.1.0"
edition = "2024"
TOML
cat >"$fixture/deny.toml" <<'TOML'
[licenses]
allow = ["MIT"]
TOML
cat >"$fixture/crates/velnor-runner/deny.toml" <<'TOML'
[licenses]
allow = ["MIT"]
TOML
printf 'pub fn root_value()->u8{1}\n' >"$fixture/crates/root member/src/lib.rs"
printf 'pub fn nested_value()->u8{1}\n' >"$fixture/crates/velnor-runner/runner member/src/lib.rs"
root_source="$fixture/crates/root member/src/lib.rs"
nested_source="$fixture/crates/velnor-runner/runner member/src/lib.rs"
cp "$root_source" "$tmp_root/root-unformatted.rs"
cp "$nested_source" "$tmp_root/nested-unformatted.rs"
git -C "$fixture" add .
staged_tree=$(git -C "$fixture" write-tree)

"$hk_bin" --cd "$fixture" fix --all --step cargo_fmt --no-stage --no-progress
if cmp -s "$root_source" "$tmp_root/root-unformatted.rs" ||
  cmp -s "$nested_source" "$tmp_root/nested-unformatted.rs"; then
  printf '%s\n' 'hk did not format both workspace sources.' >&2
  exit 1
fi
cp "$root_source" "$tmp_root/root-formatted.rs"
cp "$nested_source" "$tmp_root/nested-formatted.rs"
[[ "$(git -C "$fixture" write-tree)" == "$staged_tree" ]]
"$hk_bin" --cd "$fixture" check --all --step cargo_fmt --no-progress
cmp -s "$root_source" "$tmp_root/root-formatted.rs"
cmp -s "$nested_source" "$tmp_root/nested-formatted.rs"
[[ "$(git -C "$fixture" write-tree)" == "$staged_tree" ]]

printf 'pub fn nested_value( -> u8 { 1 }\n' >"$nested_source"
cp "$nested_source" "$tmp_root/nested-invalid.rs"
if "$hk_bin" --cd "$fixture" check --all --step cargo_fmt --no-progress >"$tmp_root/invalid-rust.log" 2>&1; then
  printf '%s\n' 'hk accepted invalid Rust.' >&2
  exit 1
fi
if ! grep -Fq 'runner member/src/lib.rs' "$tmp_root/invalid-rust.log"; then
  cat "$tmp_root/invalid-rust.log" >&2
  printf '%s\n' 'hk failure did not identify the invalid nested Rust source.' >&2
  exit 1
fi
if ! grep -Eq 'error: (expected|unexpected|unclosed|mismatched|failed to parse|this file contains an unclosed delimiter)' "$tmp_root/invalid-rust.log"; then
  cat "$tmp_root/invalid-rust.log" >&2
  printf '%s\n' 'hk failure did not report a Rust parse diagnostic.' >&2
  exit 1
fi
cmp -s "$root_source" "$tmp_root/root-formatted.rs"
cmp -s "$nested_source" "$tmp_root/nested-invalid.rs"
[[ "$(git -C "$fixture" write-tree)" == "$staged_tree" ]]

cp "$tmp_root/root-formatted.rs" "$root_source"
cp "$tmp_root/nested-formatted.rs" "$nested_source"
nested_manifest="$fixture/crates/velnor-runner/runner member/Cargo.toml"
mv "$nested_manifest" "$tmp_root/nested-member-Cargo.toml"
ln -s "$tmp_root/nested-member-Cargo.toml" "$nested_manifest"
if "$hk_bin" --cd "$fixture" check --all --step cargo_fmt --no-progress >"$tmp_root/symlink-manifest.log" 2>&1; then
  printf '%s\n' 'hk accepted a symlink workspace manifest.' >&2
  exit 1
fi
if ! grep -Fq "hk-workspace-admission: expected a regular Cargo.toml inside this repository: $nested_manifest" "$tmp_root/symlink-manifest.log"; then
  cat "$tmp_root/symlink-manifest.log" >&2
  printf '%s\n' 'hk did not reject the symlink workspace manifest at admission.' >&2
  exit 1
fi
cmp -s "$root_source" "$tmp_root/root-formatted.rs"
cmp -s "$nested_source" "$tmp_root/nested-formatted.rs"
[[ "$(git -C "$fixture" write-tree)" == "$staged_tree" ]]

rm "$nested_manifest"
mv "$tmp_root/nested-member-Cargo.toml" "$nested_manifest"
cat >"$fixture/Cargo.lock" <<'LOCK'
version = 4

[[package]]
name = "root-member-fixture"
version = "0.1.0"
LOCK
cat >"$fixture/crates/velnor-runner/Cargo.lock" <<'LOCK'
version = 4

[[package]]
name = "runner-member-fixture"
version = "0.1.0"
LOCK
git -C "$fixture" add Cargo.lock crates/velnor-runner/Cargo.lock
deny_staged_tree=$(git -C "$fixture" write-tree)
cp "$fixture/Cargo.lock" "$tmp_root/root-before-deny.lock"
cp "$fixture/crates/velnor-runner/Cargo.lock" "$tmp_root/nested-before-deny.lock"
deny_bin="$tmp_root/deny external tools"
deny_scans="$tmp_root/deny scan results"
mkdir -p "$deny_bin" "$deny_scans"
cat >"$deny_bin/rust-build-guard" <<'SH'
#!/bin/sh
set -eu
if [ "${RUST_BUILD_GUARD_MODE:-allow}" = deny ]; then
  printf '%s\n' 'fixture guard denied cargo deny' >&2
  exit 125
fi
if [ "$#" -eq 9 ] && [ "$1" = -- ]; then
  shift
fi
if [ "$#" -ne 8 ] || [ "$1" != mise ] || [ "$2" != exec ] ||
  [ "$3" != -- ] || [ "$4" != cargo ] || [ "$5" != deny ] ||
  [ "$6" != --locked ] || [ "$7" != --workspace ] || [ "$8" != check ]; then
  printf '%s\n' 'fixture guard rejected command' >&2
  exit 125
fi
exec "$@"
SH
cat >"$deny_bin/mise" <<'SH'
#!/bin/sh
set -eu
if [ "$#" -ne 7 ] || [ "$1" != exec ] || [ "$2" != -- ] ||
  [ "$3" != cargo ] || [ "$4" != deny ] || [ "$5" != --locked ] ||
  [ "$6" != --workspace ] || [ "$7" != check ]; then
  printf '%s\n' 'fixture Mise rejected command' >&2
  exit 125
fi
shift 2
exec "$@"
SH
cat >"$deny_bin/cargo" <<'SH'
#!/bin/sh
set -eu
if [ "$#" -ne 4 ] || [ "$1" != deny ] || [ "$2" != --locked ] ||
  [ "$3" != --workspace ] || [ "$4" != check ]; then
  printf '%s\n' 'fixture Cargo rejected command' >&2
  exit 125
fi
case "$PWD" in
  "$DENY_ROOT") scan=root ;;
  "$DENY_NESTED") scan=nested ;;
  *) printf '%s\n' 'fixture Cargo received an unknown workspace' >&2; exit 125 ;;
esac
: >"$DENY_SCAN_DIR/$scan"
printf 'cargo deny scan passed: %s\n' "$scan"
SH
cat >"$deny_bin/cargo-deny" <<'SH'
#!/bin/sh
set -eu
if [ "$#" -ne 3 ] || [ "$1" != --locked ] ||
  [ "$2" != --workspace ] || [ "$3" != check ]; then
  printf '%s\n' 'fixture cargo-deny rejected command' >&2
  exit 125
fi
case "$PWD" in
  "$DENY_ROOT"|"$DENY_ROOT/crates/root member") scan=root ;;
  "$DENY_NESTED"|"$DENY_NESTED/runner member") scan=nested ;;
  *) printf '%s\n' 'fixture cargo-deny received an unknown workspace' >&2; exit 125 ;;
esac
: >"$DENY_SCAN_DIR/$scan"
printf 'cargo deny scan passed: %s\n' "$scan"
SH
chmod +x "$deny_bin/rust-build-guard" "$deny_bin/mise" "$deny_bin/cargo" "$deny_bin/cargo-deny"
export DENY_ROOT="$fixture" DENY_NESTED="$fixture/crates/velnor-runner"
export DENY_SCAN_DIR="$deny_scans"
export PATH="$deny_bin:$PATH"
test_phase='root cargo_deny check'
"$hk_bin" --cd "$fixture" check --all --step cargo_deny --no-progress >"$tmp_root/root-deny.log" 2>&1
test_phase='nested cargo_deny check'
"$hk_bin" --cd "$fixture/crates/velnor-runner" check --all --step cargo_deny --no-progress >"$tmp_root/nested-deny.log" 2>&1
[[ -f "$deny_scans/root" && -f "$deny_scans/nested" ]]
grep -Fq 'cargo deny scan passed: root' "$tmp_root/root-deny.log"
grep -Fq 'cargo deny scan passed: nested' "$tmp_root/nested-deny.log"
cmp -s "$fixture/Cargo.lock" "$tmp_root/root-before-deny.lock"
cmp -s "$fixture/crates/velnor-runner/Cargo.lock" "$tmp_root/nested-before-deny.lock"
[[ "$(git -C "$fixture" write-tree)" == "$deny_staged_tree" ]]

rm "$deny_scans/root" "$deny_scans/nested"
test_phase='guard-denied cargo_deny check'
if RUST_BUILD_GUARD_MODE=deny "$hk_bin" --cd "$fixture" check --all --step cargo_deny --no-progress >"$tmp_root/guard-deny.log" 2>&1; then
  if [[ -f "$deny_scans/root" ]]; then
    printf '%s\n' 'hk cargo_deny bypassed the installed guard denial.' >&2
  else
    cat "$tmp_root/guard-deny.log" >&2
    printf '%s\n' 'hk unexpectedly accepted the guard-denied cargo_deny command.' >&2
  fi
  exit 1
fi
if ! grep -Fq 'fixture guard denied cargo deny' "$tmp_root/guard-deny.log"; then
  cat "$tmp_root/guard-deny.log" >&2
  printf '%s\n' 'hk cargo_deny did not expose the guard denial.' >&2
  exit 1
fi
[[ ! -e "$deny_scans/root" && ! -e "$deny_scans/nested" ]]
cmp -s "$fixture/Cargo.lock" "$tmp_root/root-before-deny.lock"
cmp -s "$fixture/crates/velnor-runner/Cargo.lock" "$tmp_root/nested-before-deny.lock"
[[ "$(git -C "$fixture" write-tree)" == "$deny_staged_tree" ]]

printf '%s\n' 'native hk fmt behavior passed'
