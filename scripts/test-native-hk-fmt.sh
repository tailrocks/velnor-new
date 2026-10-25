#!/usr/bin/env bash
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
trap 'rm -rf "$tmp_root"' EXIT
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

printf '%s\n' 'native hk fmt behavior passed'
