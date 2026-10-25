#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/hk-cargo.XXXXXX")
trap 'rm -rf "$tmp_root"' EXIT

fixture="$tmp_root/repo"
mock_bin="$tmp_root/mock-bin"
bad_bin="$tmp_root/bad-bin"
mkdir -p "$fixture/scripts" "$fixture/crates/velnor-runner/crates/member" \
  "$mock_bin" "$bad_bin" "$fixture/crates/velnor-runner"
cp "$repo_root/scripts/hk-cargo.sh" "$fixture/scripts/hk-cargo.sh"
git -C "$fixture" init -q
fixture=$(cd -P "$fixture" && pwd -P)
printf '%s\n' '[workspace]' >"$fixture/Cargo.toml"
printf '%s\n' '[workspace]' >"$fixture/crates/velnor-runner/Cargo.toml"
printf '%s\n' '[package]' >"$fixture/crates/velnor-runner/crates/member/Cargo.toml"
printf '%s\n' '[licenses]' >"$fixture/deny.toml"
printf '%s\n' '[licenses]' >"$fixture/crates/velnor-runner/deny.toml"

cat >"$mock_bin/mise" <<'MOCK_MISE'
#!/usr/bin/env bash
set -euo pipefail
{
  printf 'cwd=%s\n' "$PWD"
  printf 'arg=%s\n' "$@"
} >>"$HK_MISE_LOG"
MOCK_MISE
cat >"$mock_bin/rust-build-guard" <<'MOCK_GUARD'
#!/usr/bin/env bash
set -euo pipefail
{
  printf 'cwd=%s\n' "$PWD"
  printf 'arg=%s\n' "$@"
} >>"$HK_GUARD_LOG"
shift
exec "$@"
MOCK_GUARD
chmod +x "$mock_bin/mise" "$mock_bin/rust-build-guard"
printf '%s\n' '#!/bin/sh' 'exit 0' >"$bad_bin/rust-build-guard"
chmod 644 "$bad_bin/rust-build-guard"

mise_log="$tmp_root/mise.log"
guard_log="$tmp_root/guard.log"
clear_logs() {
  : >"$mise_log"
  : >"$guard_log"
}

run_operation() {
  local manifest=$1 operation=$2
  (
    cd "$fixture"
    PATH="$mock_bin:$PATH" HK_MISE_LOG="$mise_log" HK_GUARD_LOG="$guard_log" \
      bash scripts/hk-cargo.sh "$manifest" "$operation"
  )
}

assert_log_has() {
  local log=$1 line=$2
  grep -Fqx "$line" "$log" || {
    printf 'missing log line: %s in %s\n' "$line" "$log" >&2
    exit 1
  }
}

assert_guarded_cargo() {
  local subcommand=$1
  assert_log_has "$guard_log" 'arg=--'
  assert_log_has "$guard_log" 'arg=mise'
  assert_log_has "$guard_log" 'arg=exec'
  assert_log_has "$guard_log" 'arg=cargo'
  assert_log_has "$guard_log" "arg=$subcommand"
  assert_log_has "$mise_log" 'arg=exec'
  assert_log_has "$mise_log" 'arg=cargo'
  assert_log_has "$mise_log" "arg=$subcommand"
}

for operation in fmt-check fmt-files fmt-fix; do
  clear_logs
  run_operation Cargo.toml "$operation"
  assert_guarded_cargo fmt
  assert_log_has "$mise_log" 'arg=--all'
  if [[ "$operation" != fmt-fix ]]; then
    assert_log_has "$mise_log" 'arg=--check'
  fi
  if [[ "$operation" == fmt-files ]]; then
    assert_log_has "$mise_log" 'arg=--files-with-diff'
  fi
done

for operation in check check-fix clippy clippy-fix; do
  clear_logs
  run_operation Cargo.toml "$operation"
  case "$operation" in
    check) expected_command=check ;;
    check-fix) expected_command=fix ;;
    *) expected_command=clippy ;;
  esac
  assert_guarded_cargo "$expected_command"
  assert_log_has "$mise_log" 'arg=--locked'
  assert_log_has "$mise_log" 'arg=--quiet'
  if [[ "$operation" == check-fix || "$operation" == clippy-fix ]]; then
    assert_log_has "$mise_log" 'arg=--allow-dirty'
    assert_log_has "$mise_log" 'arg=--allow-staged'
  fi
  [[ "$operation" != clippy-fix ]] || assert_log_has "$mise_log" 'arg=--fix'
done

clear_logs
run_operation crates/velnor-runner/crates/member/Cargo.toml deny
assert_log_has "$mise_log" "cwd=$fixture/crates/velnor-runner/crates/member"
assert_log_has "$mise_log" 'arg=cargo-deny@0.20.2'
assert_log_has "$mise_log" "arg=$fixture/crates/velnor-runner/deny.toml"
assert_log_has "$mise_log" 'arg=--workspace'
assert_log_has "$mise_log" 'arg=check'
[[ ! -s "$guard_log" ]] || {
  printf '%s\n' 'deny must not route through rust-build-guard' >&2
  exit 1
}

clear_logs
if run_operation Cargo.toml unsupported >/dev/null 2>&1; then
  printf '%s\n' 'unknown operation unexpectedly succeeded' >&2
  exit 1
fi
if bash "$fixture/scripts/hk-cargo.sh" Cargo.toml check extra >/dev/null 2>&1; then
  printf '%s\n' 'extra operation argument unexpectedly succeeded' >&2
  exit 1
fi
[[ ! -s "$mise_log" ]] || {
  printf '%s\n' 'invalid operation reached mise' >&2
  exit 1
}

outside="$tmp_root/outside"
mkdir -p "$outside"
printf '%s\n' '[workspace]' >"$outside/Cargo.toml"
ln -s "$outside/Cargo.toml" "$fixture/linked-Cargo.toml"
mkdir -p "$fixture/linked"
ln -s "$outside/Cargo.toml" "$fixture/linked/Cargo.toml"
ln -s "$outside" "$fixture/escaped"
for invalid_manifest in "$outside/Cargo.toml" linked/Cargo.toml escaped/Cargo.toml; do
  clear_logs
  if run_operation "$invalid_manifest" check >/dev/null 2>&1; then
    printf 'invalid manifest unexpectedly succeeded: %s\n' "$invalid_manifest" >&2
    exit 1
  fi
  [[ ! -s "$mise_log" ]] || {
    printf 'invalid manifest reached mise: %s\n' "$invalid_manifest" >&2
    exit 1
  }
done

clear_logs
if (
  cd "$fixture"
  PATH="$bad_bin:$mock_bin:$PATH" HK_MISE_LOG="$mise_log" HK_GUARD_LOG="$guard_log" \
    bash scripts/hk-cargo.sh Cargo.toml check >/dev/null 2>&1
); then
  printf '%s\n' 'invalid installed guard unexpectedly succeeded' >&2
  exit 1
fi
[[ ! -s "$mise_log" ]] || {
  printf '%s\n' 'invalid guard allowed Cargo routing' >&2
  exit 1
}

ln -sf "$tmp_root/missing-guard-target" "$bad_bin/rust-build-guard"
clear_logs
if (
  cd "$fixture"
  PATH="$bad_bin:$mock_bin:$PATH" HK_MISE_LOG="$mise_log" HK_GUARD_LOG="$guard_log" \
    bash scripts/hk-cargo.sh Cargo.toml check >/dev/null 2>&1
); then
  printf '%s\n' 'dangling installed guard unexpectedly succeeded' >&2
  exit 1
fi
[[ ! -s "$mise_log" ]] || {
  printf '%s\n' 'dangling guard allowed Cargo routing' >&2
  exit 1
}

printf '%s\n' 'hk-cargo shell mock tests passed'
