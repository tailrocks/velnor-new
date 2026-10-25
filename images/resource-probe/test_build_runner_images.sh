#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
real_python="$(command -v python3)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/velnor-runner-image-script-test.XXXXXXXX")"
readonly repo_root real_python test_root
readonly sandbox="$test_root/workspace"
readonly stub_dir="$test_root/stubs"
readonly source_revision='0123456789abcdef0123456789abcdef01234567'
readonly binary_dir="$sandbox/crates/velnor-runner/target/x86_64-unknown-linux-musl/release"
readonly mise_log="$test_root/mise.log"
readonly docker_log="$test_root/docker.log"
readonly git_log="$test_root/git.log"
readonly tool_log="$test_root/tool.log"

trap 'rm -rf "$test_root"' EXIT

mkdir -p \
  "$sandbox/.velnor" \
  "$sandbox/images/resource-probe" \
  "$sandbox/images/runner/ubuntu-26.04" \
  "$binary_dir" \
  "$stub_dir" \
  "$test_root/tmp"
cp "$repo_root/images/resource-probe/build-runner-images.sh" \
  "$sandbox/images/resource-probe/build-runner-images.sh"
cp "$repo_root/images/resource-probe/verify_runner_listener.py" \
  "$sandbox/images/resource-probe/verify_runner_listener.py"
cp "$repo_root/images/runner/ubuntu-26.04/job-started-hook.sh" \
  "$sandbox/images/runner/ubuntu-26.04/job-started-hook.sh"
cat > "$sandbox/.velnor/runner.lock" <<'TOML'
runner_version = "2.338.0"
TOML

printf 'resource probe source %s\n' "$source_revision" > "$binary_dir/velnor-resource-probe"

cat > "$stub_dir/git" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$GIT_LOG"
case "${1:-}" in
  rev-parse)
    [[ "${2:-}" == HEAD ]]
    printf '%s\n' "$VELNOR_SOURCE_REVISION"
    ;;
  diff|ls-files)
    ;;
  *)
    printf 'unexpected git command: %s\n' "$*" >&2
    exit 97
    ;;
esac
STUB

cat > "$stub_dir/python3" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == '-B' && "${2:-}" == 'images/resource-probe/test_verify_runner_listener.py' ]]; then
  printf 'listener-test\n' >> "$TOOL_LOG"
  exit 0
fi
exec "$REAL_PYTHON" "$@"
STUB

cat > "$stub_dir/mise" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$MISE_LOG"
mkdir -p "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release"
printf 'storage init fixture\n' > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/velnor-storage-init"
printf 'job start hook fixture\n' > "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/velnor-job-start-hook"
STUB

cat > "$stub_dir/file" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf 'ELF 64-bit LSB executable, x86-64, version 1 (SYSV), statically linked\n'
STUB

cat > "$stub_dir/docker" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$DOCKER_LOG"
case "${1:-}" in
  buildx)
    exit 0
    ;;
  image)
    [[ "${2:-}" == inspect && "${3:-}" == --format ]]
    case "${4:-}" in
      '{{.Architecture}}') printf 'amd64\n' ;;
      '{{.Id}}') printf 'sha256:%064d\n' 0 ;;
      *) exit 97 ;;
    esac
    ;;
  run)
    args=("$@")
    entrypoint=''
    stat_format=''
    for ((index = 0; index < ${#args[@]}; index += 1)); do
      case "${args[$index]}" in
        --entrypoint) entrypoint="${args[$((index + 1))]}" ;;
        -c) stat_format="${args[$((index + 1))]}" ;;
      esac
    done
    last_arg="${args[$((${#args[@]} - 1))]}"
    case "$entrypoint" in
      /usr/bin/uname)
        printf 'x86_64\n'
        ;;
      /usr/bin/sha256sum)
        case "$last_arg" in
          /velnor/resource-probe) expected_hash="$EXPECTED_PROBE_SHA256" ;;
          /velnor/storage-init) expected_hash="$EXPECTED_STORAGE_SHA256" ;;
          /velnor/job-start-hook) expected_hash="$EXPECTED_HOOK_SHA256" ;;
          /usr/local/bin/velnor-job-start-hook) expected_hash="$EXPECTED_WRAPPER_SHA256" ;;
          *) exit 97 ;;
        esac
        printf '%s  %s\n' "$expected_hash" "$last_arg"
        ;;
      /usr/bin/stat)
        case "$stat_format" in
          %a) printf '555\n' ;;
          %u:%g) printf '0:0\n' ;;
          *) exit 97 ;;
        esac
        ;;
      /usr/bin/python3)
        cat > /dev/null
        ;;
      *)
        printf 'unexpected docker entrypoint: %s\n' "$entrypoint" >&2
        exit 97
        ;;
    esac
    ;;
  *)
    printf 'unexpected docker command: %s\n' "$*" >&2
    exit 97
    ;;
esac
STUB

chmod 0755 "$stub_dir/git" "$stub_dir/python3" "$stub_dir/mise" "$stub_dir/file" "$stub_dir/docker"

export PATH="$stub_dir:$PATH"
export REAL_PYTHON="$real_python"
export VELNOR_SOURCE_REVISION="$source_revision"
export CARGO_TARGET_DIR="$sandbox/crates/velnor-runner/target"
export RUNNER_TEMP="$test_root/tmp"
export MISE_LOG="$mise_log"
export DOCKER_LOG="$docker_log"
export GIT_LOG="$git_log"
export TOOL_LOG="$tool_log"

sha256() {
  shasum -a 256 "$1" | cut -d ' ' -f 1
}
sha256_text() {
  printf '%s\n' "$1" | shasum -a 256 | cut -d ' ' -f 1
}
EXPECTED_PROBE_SHA256="$(sha256 "$binary_dir/velnor-resource-probe")"
EXPECTED_WRAPPER_SHA256="$(sha256 "$sandbox/images/runner/ubuntu-26.04/job-started-hook.sh")"
EXPECTED_STORAGE_SHA256="$(sha256_text 'storage init fixture')"
EXPECTED_HOOK_SHA256="$(sha256_text 'job start hook fixture')"
export EXPECTED_PROBE_SHA256 EXPECTED_WRAPPER_SHA256
export EXPECTED_STORAGE_SHA256 EXPECTED_HOOK_SHA256

run_script() {
  (
    cd "$sandbox"
    bash images/resource-probe/build-runner-images.sh "$@"
  )
}

run_script_without_revision() {
  (
    cd "$sandbox"
    env -u VELNOR_SOURCE_REVISION bash images/resource-probe/build-runner-images.sh "$@"
  )
}

assert_no_tool_calls() {
  [[ ! -e "$GIT_LOG" && ! -e "$MISE_LOG" && ! -e "$DOCKER_LOG" && ! -e "$TOOL_LOG" ]]
}

assert_invalid_arguments_fail_early() {
  local output_path="$test_root/invalid-arguments.out"
  local status
  if run_script_without_revision "$@" > "$output_path" 2>&1; then
    printf 'invalid arguments unexpectedly succeeded: %s\n' "$*" >&2
    exit 1
  else
    status=$?
  fi
  [[ "$status" -eq 2 ]]
  grep -Fq 'Usage:' "$output_path"
  assert_no_tool_calls
}

assert_invalid_arguments_fail_early --unknown
assert_invalid_arguments_fail_early --prebuilt-binaries extra

if ! run_script; then
  printf 'default image build script mode failed under stubs\n' >&2
  exit 1
fi
[[ "$(wc -l < "$MISE_LOG" | tr -d '[:space:]')" -eq 1 ]]
grep -Fq 'cargo build' "$MISE_LOG"
grep -Fq -- '--bin velnor-storage-init' "$MISE_LOG"
grep -Fq -- '--bin velnor-job-start-hook' "$MISE_LOG"
[[ "$(wc -l < "$DOCKER_LOG" | tr -d '[:space:]')" -gt 2 ]]

rm "$MISE_LOG" "$DOCKER_LOG" "$GIT_LOG" "$TOOL_LOG"
if ! run_script --prebuilt-binaries; then
  printf 'prebuilt image build script mode failed under stubs\n' >&2
  exit 1
fi
[[ ! -e "$MISE_LOG" ]]
[[ "$(wc -l < "$DOCKER_LOG" | tr -d '[:space:]')" -gt 2 ]]

rm "$binary_dir/velnor-storage-init" "$binary_dir/velnor-job-start-hook"
rm "$DOCKER_LOG" "$GIT_LOG" "$TOOL_LOG"
if run_script --prebuilt-binaries > "$test_root/missing-helpers.out" 2>&1; then
  printf 'prebuilt mode unexpectedly succeeded without helper binaries\n' >&2
  exit 1
fi
[[ ! -e "$DOCKER_LOG" && ! -e "$MISE_LOG" ]]

printf 'storage init fixture\n' > "$binary_dir/velnor-storage-init"
rm -f "$DOCKER_LOG" "$MISE_LOG"
if run_script --prebuilt-binaries > "$test_root/missing-job-start-hook.out" 2>&1; then
  printf 'prebuilt mode unexpectedly succeeded without the job-start hook\n' >&2
  exit 1
fi
[[ ! -e "$DOCKER_LOG" && ! -e "$MISE_LOG" ]]

printf 'runner image build argument and prebuilt-binary checks passed\n'
