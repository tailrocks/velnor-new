#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'paired-sdk-run: %s\n' "$1" >&2
  exit 1
}

[[ $# == 1 ]] || fail 'usage: paired-sdk-run.sh <artifact-directory>'
workspace="${GITHUB_WORKSPACE:-}"
source_sha="${GITHUB_SHA:-}"
runner_temp="${RUNNER_TEMP:-}"
expected_binary_sha="${VELNOR_PAIRED_SDK_BINARY_SHA256:-}"
[[ "$workspace" == /* && -d "$workspace" && ! -L "$workspace" ]] || fail 'GITHUB_WORKSPACE must be an absolute real directory'
[[ "$runner_temp" == /* && -d "$runner_temp" && ! -L "$runner_temp" ]] || fail 'RUNNER_TEMP must be an absolute real directory'
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'GITHUB_SHA must be a full lowercase commit id'
[[ "$expected_binary_sha" =~ ^[0-9a-f]{64}$ ]] || fail 'builder binary digest is required'
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || fail 'paired SDK run requires Linux x86_64'
[[ "${DOCKER_HOST:-}" == tcp://localhost:2375 ]] || fail 'DOCKER_HOST must be tcp://localhost:2375'
[[ ! ${TESTCONTAINERS_HOST_OVERRIDE+x} ]] || fail 'TESTCONTAINERS_HOST_OVERRIDE must be absent'
[[ ! ${DOCKER_CONTEXT+x} ]] || fail 'DOCKER_CONTEXT must be absent'

python_candidate="$(builtin type -P python3)" || fail 'python3 is required to validate artifacts'
[[ "$python_candidate" == /* && -f "$python_candidate" && -x "$python_candidate" ]] || fail 'python3 must resolve to an absolute executable'
python_bin="$("$python_candidate" -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$python_candidate")" || fail 'cannot resolve python3 executable'
[[ "$python_bin" == /* && -f "$python_bin" && ! -L "$python_bin" && -x "$python_bin" ]] || fail 'python3 must resolve to a regular executable'
resolve_executable() {
  local name="$1" candidate resolved
  candidate="$(builtin type -P "$name")" || fail "$name must resolve to an external executable"
  [[ "$candidate" == /* && -f "$candidate" && -x "$candidate" ]] || fail "$name must resolve through an absolute executable path"
  resolved="$("$python_bin" -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$candidate")" || fail "cannot resolve $name executable"
  [[ "$resolved" == /* && -f "$resolved" && ! -L "$resolved" && -x "$resolved" ]] || fail "$name must resolve to a regular executable"
  printf '%s\n' "$resolved"
}

workspace="$(cd "$workspace" && pwd -P)"
runner_temp="$(cd "$runner_temp" && pwd -P)"
cd "$workspace"
head="$(git -C "$workspace" rev-parse --verify HEAD)" || fail 'cannot resolve workspace HEAD'
[[ "$head" == "$source_sha" ]] || fail 'GITHUB_SHA does not match workspace HEAD'

fixture='fixtures/qualification/testcontainers-rust'
server='qualification/testcontainers-tls/server.mjs'
tracked_paths=(
  'mise.toml'
  'rust-toolchain.toml'
  "$fixture/Cargo.toml"
  "$fixture/Cargo.lock"
  "$server"
  'qualification/testcontainers/package.json'
  'qualification/testcontainers/package-lock.json'
  'qualification/testcontainers/tls.mjs'
  'scripts/qualification/paired-sdk-build.sh'
  'scripts/qualification/paired-sdk-run.sh'
  'scripts/qualification/paired-sdk-contract-test.sh'
)
assert_clean_source() {
  local status cargo_root config_dir name config relative
  local cargo_roots=(
    "$workspace"
    "$workspace/fixtures"
    "$workspace/fixtures/qualification"
    "$workspace/$fixture"
  )
  status="$(git -C "$workspace" status --porcelain --untracked-files=all)" || fail 'cannot inspect workspace source state'
  [[ -z "$status" ]] || fail 'workspace source tree must be clean'
  for cargo_root in "${cargo_roots[@]}"; do
    config_dir="$cargo_root/.cargo"
    [[ ! -L "$config_dir" ]] || fail 'Cargo configuration directory must not be a symlink'
    [[ -d "$config_dir" ]] || continue
    for name in config config.toml; do
      config="$config_dir/$name"
      [[ -e "$config" || -L "$config" ]] || continue
      [[ -f "$config" && ! -L "$config" ]] || fail 'Cargo configuration must be a regular non-symlink file'
      relative="${config#"$workspace"/}"
      git -C "$workspace" ls-files --error-unmatch -- "$relative" >/dev/null 2>&1 || fail 'untracked Cargo configuration is not allowed'
    done
  done
}
for relative in "${tracked_paths[@]}"; do
  [[ -f "$workspace/$relative" && ! -L "$workspace/$relative" ]] || fail "required input is not a regular file: $relative"
  git -C "$workspace" ls-files --error-unmatch -- "$relative" >/dev/null 2>&1 || fail "required input is not tracked: $relative"
done
git -C "$workspace" ls-files --error-unmatch -- "$fixture/src" >/dev/null 2>&1 || fail 'fixture Rust sources are not tracked'
assert_clean_source

artifact_dir="$1"
[[ "$artifact_dir" == /* && -d "$artifact_dir" && ! -L "$artifact_dir" ]] || fail 'artifact path must be an absolute real directory'
artifact_dir="$(cd "$artifact_dir" && pwd -P)"
case "$artifact_dir" in
  "$runner_temp"/*) ;;
  *) fail 'artifact directory must be under RUNNER_TEMP' ;;
esac
"$python_bin" - "$artifact_dir" <<'PY' || fail 'artifact directory shape, mode, or file types are invalid'
import os
import pathlib
import stat
import sys

root = pathlib.Path(sys.argv[1])
if stat.S_IMODE(root.lstat().st_mode) not in (0o700, 0o755):
    raise SystemExit(1)
if set(os.listdir(root)) != {"manifest.txt", "velnor-testcontainers-tls-fixture"}:
    raise SystemExit(1)
for name in ("manifest.txt", "velnor-testcontainers-tls-fixture"):
    path = root / name
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
        raise SystemExit(1)
    if name == "manifest.txt" and metadata.st_size > 512:
        raise SystemExit(1)
    if name == "velnor-testcontainers-tls-fixture" and not (0 < metadata.st_size <= 268435456):
        raise SystemExit(1)
manifest_mode = stat.S_IMODE((root / "manifest.txt").lstat().st_mode)
binary_mode = stat.S_IMODE((root / "velnor-testcontainers-tls-fixture").lstat().st_mode)
if manifest_mode != 0o644 or binary_mode not in (0o644, 0o755):
    raise SystemExit(1)
PY

manifest="$artifact_dir/manifest.txt"
binary="$artifact_dir/velnor-testcontainers-tls-fixture"
for file in "$manifest" "$binary"; do
  [[ -f "$file" && ! -L "$file" ]] || fail 'artifact entries must be regular non-symlink files'
done
sha256sum_executable="$(resolve_executable sha256sum)"
fixture_manifest_sum="$("$sha256sum_executable" -- "$workspace/$fixture/Cargo.toml")"
fixture_manifest_sha="${fixture_manifest_sum%% *}"
fixture_lock_sum="$("$sha256sum_executable" -- "$workspace/$fixture/Cargo.lock")"
fixture_lock_sha="${fixture_lock_sum%% *}"
server_sum="$("$sha256sum_executable" -- "$workspace/$server")"
server_sha="${server_sum%% *}"
for digest in "$fixture_manifest_sha" "$fixture_lock_sha" "$server_sha"; do
  [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || fail 'sha256sum returned an invalid source digest'
done
"$python_bin" - "$manifest" "$source_sha" "$fixture_manifest_sha" "$fixture_lock_sha" "$server_sha" "$expected_binary_sha" <<'PY' || fail 'artifact manifest is not the exact canonical seven-line format'
import pathlib
import sys

_, path, source, fixture_manifest, fixture_lock, server, binary = sys.argv
expected = (
    "schema=velnor-paired-sdk-artifact-v1\n"
    f"source_sha={source}\n"
    f"fixture_manifest_sha256={fixture_manifest}\n"
    f"fixture_lock_sha256={fixture_lock}\n"
    f"server_sha256={server}\n"
    f"binary_sha256={binary}\n"
    "target=x86_64-unknown-linux-gnu\n"
).encode("ascii")
if pathlib.Path(path).read_bytes() != expected:
    raise SystemExit(1)
PY
binary_sum="$("$sha256sum_executable" -- "$binary")"
actual_binary_sha="${binary_sum%% *}"
[[ "$actual_binary_sha" =~ ^[0-9a-f]{64}$ ]] || fail 'sha256sum returned an invalid binary digest'
[[ "$actual_binary_sha" == "$expected_binary_sha" ]] || fail 'artifact binary digest does not match independent builder output'

node_executable="$(resolve_executable node)"
npm_executable="$(resolve_executable npm)"
timeout_executable="$(resolve_executable timeout)"
env_executable="$(resolve_executable env)"
timeout_version="$("$timeout_executable" --version 2>/dev/null)" || fail 'cannot inspect GNU coreutils timeout'
timeout_version="${timeout_version%%$'\n'*}"
[[ "$timeout_version" == 'timeout (GNU coreutils)'* ]] || fail 'GNU coreutils timeout is required'

scratch=''
cleanup() {
  local status=$?
  if [[ -n "$scratch" && -d "$scratch" && ! -L "$scratch" ]]; then
    rm -rf "$scratch" || {
      printf '%s\n' 'paired-sdk-run: could not remove owned scratch directory' >&2
      status=1
    }
  fi
  trap - EXIT
  exit "$status"
}
trap cleanup EXIT
scratch="$(umask 077 && mktemp -d "$runner_temp/velnor-paired-sdk-run.XXXXXXXX")" || fail 'cannot create private run scratch'
chmod 700 "$scratch"
mkdir -m 700 "$scratch/certs" "$scratch/node" "$scratch/home" "$scratch/tmp" "$scratch/npm-cache" "$scratch/bin"
binary_copy="$scratch/bin/velnor-testcontainers-tls-fixture"
cp "$binary" "$binary_copy"
chmod 755 "$binary_copy"
node_root="$scratch/node"
mkdir -p -- "$node_root/qualification/testcontainers" "$node_root/qualification/testcontainers-tls"
cp "$workspace/qualification/testcontainers/package.json" "$node_root/qualification/testcontainers/package.json"
cp "$workspace/qualification/testcontainers/package-lock.json" "$node_root/qualification/testcontainers/package-lock.json"
cp "$workspace/qualification/testcontainers/tls.mjs" "$node_root/qualification/testcontainers/tls.mjs"
cp "$workspace/$server" "$node_root/qualification/testcontainers-tls/server.mjs"
child="$scratch/run-child.sh"
cat > "$child" <<'CHILD'
#!/usr/bin/env bash
set -euo pipefail
binary="$1"
certificates="$2"
node_root="$3"
node_executable="$4"
npm_executable="$5"
"$binary" certs "$certificates"
"$npm_executable" --prefix "$node_root/qualification/testcontainers" ci
"$node_executable" "$node_root/qualification/testcontainers/tls.mjs" "$certificates"
"$binary" sdk "$certificates"
CHILD
chmod 700 "$child"
clean_path="$("$python_bin" - "$node_executable" "$npm_executable" "$timeout_executable" <<'PY'
import os
import sys

paths = []
for executable in (*sys.argv[1:], "/usr/bin", "/bin"):
    directory = executable if executable.startswith("/") and os.path.isdir(executable) else os.path.dirname(executable)
    if not directory or not os.path.isabs(directory) or directory == ".":
        raise SystemExit("child PATH components must be absolute directories")
    if directory not in paths:
        paths.append(directory)
print(":".join(paths))
PY
)" || fail 'cannot build an absolute child PATH'
[[ "$clean_path" != :* && "$clean_path" != *: && "$clean_path" != *::* ]] || fail 'child PATH must not contain empty components'

set +e
"$env_executable" -i \
  PATH="$clean_path" \
  HOME="$scratch/home" \
  TMPDIR="$scratch/tmp" \
  NPM_CONFIG_CACHE="$scratch/npm-cache" \
  NPM_CONFIG_USERCONFIG=/dev/null \
  NPM_CONFIG_GLOBALCONFIG=/dev/null \
  DOCKER_HOST=tcp://localhost:2375 \
  "$timeout_executable" --signal=TERM --kill-after=10s 180s \
  /bin/bash "$child" \
  "$binary_copy" "$scratch/certs" "$node_root" "$node_executable" "$npm_executable"
status=$?
set -e
[[ "$status" == 0 ]] || fail "bounded paired SDK qualification failed with status $status"
printf '%s\n' 'paired-sdk-tls-ok'
