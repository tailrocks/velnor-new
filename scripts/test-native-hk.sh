#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "$0")/.." && pwd -P)
hk_bin=${HK_BIN:-$(mise x hk -- which hk)}
if [[ "$("$hk_bin" --version)" != 'hk 2.8.0' ]]; then
  printf '%s\n' 'This test requires hk 2.8.0.' >&2
  exit 1
fi

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/hk native.XXXXXX")
trap 'rm -rf "$tmp_root"' EXIT
fixture="$tmp_root/repository with spaces"
mock_bin="$tmp_root/mock-bin"
missing_guard_bin="$tmp_root/missing-guard-bin"
mkdir -p "$fixture/crates/root member/src" \
  "$fixture/crates/velnor-runner/runner member/src" \
  "$fixture/fixtures/broken/src" \
  "$fixture/docs/proposed/opentofu-goldens/broken/src" \
  "$fixture/home" "$fixture/config" "$mock_bin" "$missing_guard_bin"
cp "$project_root/hk.pkl" "$fixture/hk.pkl"
git -C "$fixture" init -q
git -C "$fixture" config user.name 'native hk test'
git -C "$fixture" config user.email 'native-hk@example.invalid'

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
printf 'pub fn root_value() -> u8 { 1 }\n' >"$fixture/crates/root member/src/lib.rs"
printf 'pub fn nested_value() -> u8 { 1 }\n' >"$fixture/crates/velnor-runner/runner member/src/lib.rs"
printf 'this fixture is excluded\n' >"$fixture/fixtures/broken/src/lib.rs"
printf 'this golden fixture is excluded\n' >"$fixture/docs/proposed/opentofu-goldens/broken/src/lib.rs"
printf '[licenses]\nallow = ["MIT"]\n' >"$fixture/deny.toml"
printf '[licenses]\nallow = ["MIT"]\n' >"$fixture/crates/velnor-runner/deny.toml"

cat >"$mock_bin/rust-build-guard" <<'GUARD'
#!/usr/bin/env bash
set -euo pipefail
{
  printf 'call\ncwd=%s\n' "$PWD"
  for arg in "$@"; do printf 'arg=%s\n' "$arg"; done
} >>"$HK_GUARD_LOG"
[[ ${1:-} == -- ]] || exit 64
if [[ ${FAKE_GUARD_EXIT:-0} != 0 ]]; then
  exit "$FAKE_GUARD_EXIT"
fi
shift
exec "$@"
GUARD
cat >"$mock_bin/mise" <<'MISE'
#!/usr/bin/env bash
set -euo pipefail
{
  printf 'call\ncwd=%s\n' "$PWD"
  for arg in "$@"; do printf 'arg=%s\n' "$arg"; done
} >>"$HK_MISE_LOG"
[[ ${1:-} == exec && ${2:-} == -- && ${3:-} == cargo ]] || exit 65
shift 2
exec "$@"
MISE
cat >"$mock_bin/cargo" <<'CARGO'
#!/usr/bin/env bash
set -euo pipefail
{
  printf 'call\ncwd=%s\n' "$PWD"
  for arg in "$@"; do printf 'arg=%s\n' "$arg"; done
} >>"$HK_CARGO_LOG"
if [[ ${FAKE_CARGO_EXIT:-0} != 0 ]]; then
  exit "$FAKE_CARGO_EXIT"
fi
if [[ ${1:-} == fix && ${FAKE_CARGO_FIX_CHANGE:-0} == 1 ]]; then
  printf 'pub fn fixed_value() -> u8 { 2 }\n' >src/lib.rs
fi
CARGO
cat >"$mock_bin/cargo-deny" <<'DENY'
#!/usr/bin/env bash
set -euo pipefail
{
  printf 'call\ncwd=%s\n' "$PWD"
  for arg in "$@"; do printf 'arg=%s\n' "$arg"; done
} >>"$HK_DENY_LOG"
DENY
chmod +x "$mock_bin/rust-build-guard" "$mock_bin/mise" "$mock_bin/cargo" "$mock_bin/cargo-deny"
ln -s "$mock_bin/mise" "$missing_guard_bin/mise"
ln -s "$mock_bin/cargo" "$missing_guard_bin/cargo"

fixture=$(cd -P "$fixture" && pwd -P)
git -C "$fixture" add .
GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C "$fixture" commit -qm baseline

guard_log="$tmp_root/guard.log"
mise_log="$tmp_root/mise.log"
cargo_log="$tmp_root/cargo.log"
deny_log="$tmp_root/deny.log"
clear_logs() { : >"$guard_log"; : >"$mise_log"; : >"$cargo_log"; : >"$deny_log"; }
base_path="$mock_bin:/usr/bin:/bin:/usr/sbin:/sbin"
run_hk() {
  env -i HOME="$fixture/home" XDG_CONFIG_HOME="$fixture/config" \
    TMPDIR="${TMPDIR:-/tmp}" HK_MISE=0 HK_GUARD_LOG="$guard_log" \
    HK_MISE_LOG="$mise_log" HK_CARGO_LOG="$cargo_log" HK_DENY_LOG="$deny_log" \
    FAKE_CARGO_EXIT="${FAKE_CARGO_EXIT:-0}" \
    FAKE_GUARD_EXIT="${FAKE_GUARD_EXIT:-0}" \
    FAKE_CARGO_FIX_CHANGE="${FAKE_CARGO_FIX_CHANGE:-0}" \
    PATH="${HK_TEST_PATH:-$base_path}" \
    "$hk_bin" --cd "$fixture" "$@"
}
assert_count() {
  local log=$1 expected=$2 actual
  actual=$(grep -c '^call$' "$log" || true)
  [[ "$actual" == "$expected" ]] || {
    printf 'expected %s calls in %s, got %s\n' "$expected" "$log" "$actual" >&2
    cat "$log" >&2
    exit 1
  }
}
assert_log_has() {
  local log=$1 line=$2
  grep -Fqx -- "$line" "$log" || {
    printf 'missing log line: %s in %s\n' "$line" "$log" >&2
    cat "$log" >&2
    exit 1
  }
}
assert_guarded_prefix() {
  local expected_calls=$1
  assert_count "$guard_log" "$expected_calls"
  assert_count "$mise_log" "$expected_calls"
  assert_count "$cargo_log" "$expected_calls"
  assert_log_has "$guard_log" 'arg=--'
  assert_log_has "$guard_log" 'arg=mise'
  assert_log_has "$guard_log" 'arg=exec'
  assert_log_has "$guard_log" 'arg=cargo'
  assert_log_has "$mise_log" 'arg=exec'
  assert_log_has "$mise_log" 'arg=--'
  assert_log_has "$mise_log" 'arg=cargo'
}
assert_workspaces() {
  local log=$1
  assert_log_has "$log" "cwd=$fixture/crates/root member"
  assert_log_has "$log" "cwd=$fixture/crates/velnor-runner/runner member"
  if grep -Fq 'fixtures/broken' "$log" || grep -Fq 'opentofu-goldens' "$log"; then
    printf '%s\n' 'excluded fixture trees reached a Cargo command' >&2
    cat "$log" >&2
    exit 1
  fi
}
working_tree_hash() {
  git -C "$fixture" diff --no-ext-diff --binary -- | shasum -a 256 | awk '{print $1}'
}
index_hash() {
  git -C "$fixture" ls-files --stage -z | shasum -a 256 | awk '{print $1}'
}

# Native hk selects the root and an independent nested runner workspace, under a repository path with spaces.
clear_logs
worktree_before=$(working_tree_hash)
index_before=$(index_hash)
run_hk check --all --step cargo_fmt --no-progress
assert_guarded_prefix 2
assert_workspaces "$guard_log"
assert_log_has "$cargo_log" 'arg=--all'
assert_log_has "$cargo_log" 'arg=--check'
if grep -Fqx 'arg=--manifest-path' "$cargo_log"; then
  printf '%s\n' 'fmt workspace command redundantly passed --manifest-path' >&2
  cat "$cargo_log" >&2
  exit 1
fi
[[ $(working_tree_hash) == "$worktree_before" ]]
[[ $(index_hash) == "$index_before" ]]

# Explicit fmt fix mode runs Cargo directly from each selected workspace.
clear_logs
run_hk fix --all --step cargo_fmt --no-stage --no-progress
assert_guarded_prefix 2
assert_log_has "$cargo_log" 'arg=fmt'
assert_log_has "$cargo_log" 'arg=--all'
if grep -Fqx 'arg=--check' "$cargo_log" || grep -Fqx 'arg=--manifest-path' "$cargo_log"; then
  printf '%s\n' 'fmt fix used check-only or redundant manifest arguments' >&2
  cat "$cargo_log" >&2
  exit 1
fi

# Check and Clippy keep locked/quiet flags and pass failures through guard, Mise, and hk.
for step in cargo_check cargo_clippy; do
  clear_logs
  run_hk check --all --step "$step" --no-progress
  assert_guarded_prefix 2
  assert_workspaces "$cargo_log"
  assert_log_has "$cargo_log" 'arg=--locked'
  assert_log_has "$cargo_log" 'arg=--quiet'
  if grep -Fqx 'arg=--manifest-path' "$cargo_log"; then
    printf '%s\n' 'workspace command redundantly passed --manifest-path' >&2
    cat "$cargo_log" >&2
    exit 1
  fi
done
clear_logs
run_hk fix --all --step cargo_clippy --no-stage --no-progress
assert_guarded_prefix 2
assert_log_has "$cargo_log" 'arg=clippy'
assert_log_has "$cargo_log" 'arg=--fix'
assert_log_has "$cargo_log" 'arg=--allow-dirty'
assert_log_has "$cargo_log" 'arg=--allow-staged'
assert_log_has "$cargo_log" 'arg=--locked'
assert_log_has "$cargo_log" 'arg=--quiet'
clear_logs
if FAKE_CARGO_EXIT=43 run_hk check --all --step cargo_check --no-progress \
  2>"$tmp_root/failed-check.log"; then
  printf '%s\n' 'fake Cargo failure unexpectedly passed through hk' >&2
  exit 1
fi

# An explicit hk fix may edit source, but it must not stage those edits.
clear_logs
index_before=$(index_hash)
FAKE_CARGO_FIX_CHANGE=1 run_hk fix --all --step cargo_check --no-stage --no-progress
assert_guarded_prefix 2
assert_log_has "$cargo_log" 'arg=fix'
assert_log_has "$cargo_log" 'arg=--locked'
assert_log_has "$cargo_log" 'arg=--allow-dirty'
assert_log_has "$cargo_log" 'arg=--allow-staged'
[[ "$(cat "$fixture/crates/root member/src/lib.rs")" == 'pub fn fixed_value() -> u8 { 2 }' ]]
[[ "$(cat "$fixture/crates/velnor-runner/runner member/src/lib.rs")" == 'pub fn fixed_value() -> u8 { 2 }' ]]
[[ $(index_hash) == "$index_before" ]]
clear_logs
run_hk check --all --step cargo_check --no-progress
assert_guarded_prefix 2

# A missing native guard fails closed before Mise or Cargo starts.
clear_logs
if HK_TEST_PATH="$missing_guard_bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  run_hk check --all --step cargo_check --no-progress \
  2>"$tmp_root/missing-guard.log"; then
  printf '%s\n' 'Cargo command unexpectedly ran without rust-build-guard' >&2
  exit 1
fi
[[ ! -s "$mise_log" && ! -s "$cargo_log" ]]

# A guard that starts but fails must also stop before Mise or Cargo.
clear_logs
if FAKE_GUARD_EXIT=42 run_hk check --all --step cargo_check --no-progress \
  2>"$tmp_root/guard-failure.log"; then
  printf '%s\n' 'Cargo command unexpectedly passed through a failing rust-build-guard' >&2
  exit 1
fi
assert_count "$guard_log" 2
[[ ! -s "$mise_log" && ! -s "$cargo_log" ]]

# cargo-deny is standalone, so it bypasses the Cargo guard/Mise prefix.
clear_logs
run_hk check --all --step cargo_deny --no-progress
assert_count "$deny_log" 4
assert_count "$guard_log" 0
assert_count "$mise_log" 0
assert_log_has "$deny_log" "cwd=$fixture/crates/root member"
assert_log_has "$deny_log" "cwd=$fixture"
assert_log_has "$deny_log" "cwd=$fixture/crates/velnor-runner"
assert_log_has "$deny_log" "cwd=$fixture/crates/velnor-runner/runner member"
assert_log_has "$deny_log" 'arg=--locked'
assert_log_has "$deny_log" 'arg=--workspace'
assert_log_has "$deny_log" 'arg=check'

printf '%s\n' 'native hk Cargo builtin tests passed'
