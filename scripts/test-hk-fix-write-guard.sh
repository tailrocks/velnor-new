#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "$0")/.." && pwd)
guard_script="$project_root/scripts/hk-fix-write-guard.sh"
hk_bin=${HK_BIN:-$(mise x hk -- which hk)}
if [[ "$($hk_bin --version)" != 'hk 2.8.0' ]]; then
  printf '%s\n' 'This test requires hk 2.8.0.' >&2
  exit 1
fi
printf 'Using %s\n' "$($hk_bin --version)"

tmp_root=$(mktemp -d "${TMPDIR:-/tmp}/hk-fix-write-guard.XXXXXX")
if [[ "${KEEP_HK_TEST_FIXTURE:-0}" != 1 ]]; then
  trap 'rm -rf "$tmp_root"' EXIT
fi
fixture="$tmp_root/repository"
mkdir -p "$fixture/src" "$fixture/docs" "$fixture/scripts" "$fixture/fakebin" "$fixture/no-hooks"
cp "$guard_script" "$fixture/scripts/hk-fix-write-guard.sh"
cp "$project_root/hk.pkl" "$fixture/hk.pkl"

cat >"$fixture/fakebin/cargo" <<'CARGO'
#!/usr/bin/env bash
set -euo pipefail
case "${FIX_MODE:-noop}" in
  selected)
    printf 'pub fn selected() -> u8 { 4 }\n' >src/lib.rs
    ;;
  unselected)
    printf 'pub fn worker() -> u8 { 2 }\n' >src/worker.rs
    ;;
  untracked_mode)
    chmod 700 docs/untracked-mode.txt
    ;;
  dash_symlink)
    unlink ./-n
    ln -s new-target ./-n
    ;;
  lockfile)
    printf 'version = 3\n' >Cargo.lock
    ;;
  failure)
    exit 23
    ;;
  noop)
    ;;
  *)
    printf '%s\n' 'unknown FIX_MODE' >&2
    exit 64
    ;;
esac
CARGO
chmod +x "$fixture/fakebin/cargo"
export PATH="$fixture/fakebin:$PATH"

git -C "$fixture" init -q
git -C "$fixture" config user.name 'hk write-scope test'
git -C "$fixture" config user.email 'hk-write-scope@example.invalid'
git -C "$fixture" config core.hooksPath "$fixture/no-hooks"
printf '[package]\nname = "hk-write-scope-test"\nversion = "0.1.0"\nedition = "2021"\n' >"$fixture/Cargo.toml"
printf 'pub fn selected() -> u8 { 1 }\n' >"$fixture/src/lib.rs"
printf 'pub fn worker() -> u8 { 1 }\n' >"$fixture/src/worker.rs"
printf 'version = 4\n' >"$fixture/Cargo.lock"
printf 'unrelated baseline\n' >"$fixture/docs/note.txt"
run_hook() {
  local mode=$1
  local step=$2
  FIX_MODE="$mode" "$hk_bin" --cd "$fixture" run pre-commit --staged --step "$step" --no-progress
}
expect_guard_failure() {
  local mode=$1
  local step=$2
  local output="$tmp_root/$mode-$step.log"
  if run_hook "$mode" "$step" >"$output" 2>&1; then
    printf '%s\n' "$step changed files, but hk did not report a failure." >&2
    exit 1
  fi
  if ! grep -Fq 'A Cargo fix changed repository files or the Git index.' "$output"; then
    cat "$output" >&2
    printf '%s\n' "$step did not report the file change." >&2
    exit 1
  fi
}
git -C "$fixture" add .
staged_before=$(git -C "$fixture" ls-files --stage -z | shasum -a 256 | awk '{print $1}')
run_hook noop cargo_clippy >/dev/null
[[ "$(git -C "$fixture" ls-files --stage -z | shasum -a 256 | awk '{print $1}')" == "$staged_before" ]]
GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C "$fixture" commit -qm baseline
git -C "$fixture" rev-parse --verify HEAD >/dev/null

index_hash() {
  git -C "$fixture" ls-files --stage -z | hash_stream
}
status_hash() {
  git -C "$fixture" status --porcelain=v1 --untracked-files=all -z | hash_stream
}
hash_stream() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 | awk '{print $1}'
  else
    sha256sum | awk '{print $1}'
  fi
}
stage_partial_selected() {
  printf 'pub fn selected() -> u8 { 2 }\n' >"$fixture/src/lib.rs"
  git -C "$fixture" add src/lib.rs
  printf 'pub fn selected() -> u8 { 3 }\n' >"$fixture/src/lib.rs"
}
stage_partial_selected
staged_before=$(index_hash)
expect_guard_failure selected cargo_clippy
[[ "$(index_hash)" == "$staged_before" ]]
[[ "$(git -C "$fixture" show :src/lib.rs)" == 'pub fn selected() -> u8 { 2 }' ]]

git -C "$fixture" reset -q --hard HEAD
stage_partial_selected
printf 'pre-existing unrelated edit\n' >"$fixture/docs/note.txt"
printf 'pre-existing untracked edit\n' >"$fixture/docs/untracked-note.txt"
status_before=$(status_hash)
staged_before=$(index_hash)
run_hook noop cargo_clippy >/dev/null
[[ "$(status_hash)" == "$status_before" ]]
[[ "$(index_hash)" == "$staged_before" ]]
[[ "$(cat "$fixture/docs/note.txt")" == 'pre-existing unrelated edit' ]]
[[ "$(cat "$fixture/docs/untracked-note.txt")" == 'pre-existing untracked edit' ]]

git -C "$fixture" reset -q --hard HEAD
stage_partial_selected
staged_before=$(index_hash)
expect_guard_failure unselected cargo_clippy
[[ "$(index_hash)" == "$staged_before" ]]
[[ "$(cat "$fixture/src/worker.rs")" == 'pub fn worker() -> u8 { 2 }' ]]

git -C "$fixture" reset -q --hard HEAD
printf 'untracked mode fixture\n' >"$fixture/docs/untracked-mode.txt"
chmod 600 "$fixture/docs/untracked-mode.txt"
stage_partial_selected
staged_before=$(index_hash)
expect_guard_failure untracked_mode cargo_clippy
[[ "$(index_hash)" == "$staged_before" ]]
[[ -x "$fixture/docs/untracked-mode.txt" ]]
rm "$fixture/docs/untracked-mode.txt"

git -C "$fixture" reset -q --hard HEAD
ln -s old-target "$fixture/-n"
stage_partial_selected
staged_before=$(index_hash)
expect_guard_failure dash_symlink cargo_clippy
[[ "$(index_hash)" == "$staged_before" ]]
[[ "$(readlink "$fixture/-n")" == 'new-target' ]]
unlink "$fixture/-n"

git -C "$fixture" reset -q --hard HEAD
stage_partial_selected
staged_before=$(index_hash)
expect_guard_failure lockfile cargo_clippy
[[ "$(index_hash)" == "$staged_before" ]]
[[ "$(cat "$fixture/Cargo.lock")" == 'version = 3' ]]

git -C "$fixture" reset -q --hard HEAD
stage_partial_selected
staged_before=$(index_hash)
expect_guard_failure selected cargo_fmt
[[ "$(index_hash)" == "$staged_before" ]]

git -C "$fixture" reset -q --hard HEAD
staged_before=$(index_hash)
set +e
FIX_MODE=failure bash -c 'cd "$1" && bash "$2" cargo clippy --manifest-path Cargo.toml --fix --allow-dirty --allow-staged --quiet' _ "$fixture" "$guard_script"
failure_status=$?
set -e
[[ $failure_status -eq 23 ]]
[[ "$(index_hash)" == "$staged_before" ]]

hash_failure_bin="$tmp_root/hash-failure-bin"
mkdir -p "$hash_failure_bin"
real_shasum=$(command -v shasum)
cat >"$hash_failure_bin/shasum" <<'SHASUM'
#!/usr/bin/env bash
if [[ -n "${FAIL_HASH_AFTER_MARKER:-}" && -e "$FAIL_HASH_AFTER_MARKER" ]]; then
  exit 19
fi
if [[ "${FAIL_HASH_ALWAYS:-0}" == 1 ]]; then
  exit 19
fi
if [[ "${FAIL_HASH_EMPTY:-0}" == 1 ]]; then
  exit 0
fi
exec "$REAL_SHASUM" "$@"
SHASUM
chmod +x "$hash_failure_bin/shasum"
command_marker="$tmp_root/hash-failure-command-ran"
set +e
(cd "$fixture" && PATH="$hash_failure_bin:$PATH" REAL_SHASUM="$real_shasum" FAIL_HASH_ALWAYS=1 \
  bash "$guard_script" bash -c 'touch "$1"' _ "$command_marker") >/dev/null 2>&1
failure_status=$?
set -e
if [[ $failure_status -ne 19 ]]; then
  printf '%s\n' 'The script did not return exit status 19 after the hash command failed.' >&2
  exit 1
fi
if [[ -e "$command_marker" ]]; then
  printf '%s\n' 'The command ran after the first Git state check failed.' >&2
  exit 1
fi

command_marker="$tmp_root/invalid-hash-command-ran"
set +e
(cd "$fixture" && PATH="$hash_failure_bin:$PATH" REAL_SHASUM="$real_shasum" FAIL_HASH_EMPTY=1 \
  bash "$guard_script" bash -c 'touch "$1"' _ "$command_marker") >/dev/null 2>&1
failure_status=$?
set -e
[[ $failure_status -eq 70 ]]
if [[ -e "$command_marker" ]]; then
  printf '%s\n' 'The command ran with invalid hash output.' >&2
  exit 1
fi

git_failure_bin="$tmp_root/git-failure-bin"
mkdir -p "$git_failure_bin"
real_git=$(command -v git)
cat >"$git_failure_bin/git" <<'GIT'
#!/usr/bin/env bash
if [[ "$1" == rev-parse && "$2" == --show-toplevel ]]; then
  exec "$REAL_GIT" "$@"
fi
exit 29
GIT
chmod +x "$git_failure_bin/git"
command_marker="$tmp_root/git-failure-command-ran"
set +e
(cd "$fixture" && PATH="$git_failure_bin:$PATH" REAL_GIT="$real_git" \
  bash "$guard_script" bash -c 'touch "$1"' _ "$command_marker") >/dev/null 2>&1
failure_status=$?
set -e
[[ $failure_status -eq 29 ]]
if [[ -e "$command_marker" ]]; then
  printf '%s\n' 'The command ran after the Git state check failed.' >&2
  exit 1
fi

command_marker="$tmp_root/hash-failure-after-command-ran"
set +e
(cd "$fixture" && PATH="$hash_failure_bin:$PATH" REAL_SHASUM="$real_shasum" FAIL_HASH_AFTER_MARKER="$command_marker" \
  bash "$guard_script" bash -c 'touch "$1"; exit 23' _ "$command_marker") >/dev/null 2>&1
failure_status=$?
set -e
[[ $failure_status -eq 23 ]]

printf '%s\n' 'PASS: The test checked file changes, hash failures, and Git failures. It also checked existing edits and command exit codes.'
