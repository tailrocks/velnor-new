#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'paired-sdk-build: %s\n' "$1" >&2
  exit 1
}

[[ $# == 1 ]] || fail 'usage: paired-sdk-build.sh <empty-artifact-directory>'

workspace="${GITHUB_WORKSPACE:-}"
source_sha="${GITHUB_SHA:-}"
runner_temp="${RUNNER_TEMP:-}"
[[ "$workspace" == /* && -d "$workspace" && ! -L "$workspace" ]] || fail 'GITHUB_WORKSPACE must be an absolute real directory'
[[ "$runner_temp" == /* && -d "$runner_temp" && ! -L "$runner_temp" ]] || fail 'RUNNER_TEMP must be an absolute real directory'
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'GITHUB_SHA must be a full lowercase commit id'
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || fail 'build requires a Linux x86_64 runner'
python_candidate="$(builtin type -P python3)" || fail 'python3 is required to parse build output'
[[ "$python_candidate" == /* && -f "$python_candidate" && -x "$python_candidate" ]] || fail 'python3 must resolve through an absolute executable path'
python_bin="$("$python_candidate" -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$python_candidate")" || fail 'cannot resolve python3 executable'
[[ "$python_bin" == /* && -f "$python_bin" && ! -L "$python_bin" && -x "$python_bin" ]] || fail 'python3 must resolve to a regular executable'

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
[[ "$artifact_dir" == /* ]] || fail 'artifact directory must be absolute'
artifact_parent="$(dirname "$artifact_dir")"
artifact_leaf="$(basename "$artifact_dir")"
[[ "$artifact_leaf" != . && "$artifact_leaf" != .. ]] || fail 'artifact directory must name a child directory'
[[ -d "$artifact_parent" && ! -L "$artifact_parent" ]] || fail 'artifact parent must be a real directory'
artifact_parent="$(cd "$artifact_parent" && pwd -P)"
artifact_dir="$artifact_parent/$artifact_leaf"
case "$artifact_dir" in
  "$runner_temp"/*) ;;
  *) fail 'artifact directory must be under RUNNER_TEMP' ;;
esac
artifact_created=0
artifact_started=0
build_tmp=''
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$build_tmp" && -d "$build_tmp" && ! -L "$build_tmp" ]]; then
    rm -rf "$build_tmp" || status=1
  fi
  if [[ "$status" != 0 && "$artifact_started" == 1 && -n "$artifact_dir" && -d "$artifact_dir" && ! -L "$artifact_dir" ]]; then
    for name in manifest.txt velnor-testcontainers-tls-fixture; do
      entry="$artifact_dir/$name"
      if [[ -L "$entry" || -f "$entry" ]]; then
        rm -f "$entry" || status=1
      elif [[ -e "$entry" ]]; then
        status=1
      fi
    done
    if [[ "$artifact_created" == 1 ]]; then
      rmdir "$artifact_dir" || status=1
    fi
  fi
  exit "$status"
}
trap cleanup EXIT
if [[ -e "$artifact_dir" || -L "$artifact_dir" ]]; then
  [[ -d "$artifact_dir" && ! -L "$artifact_dir" ]] || fail 'artifact path must be a real directory'
  [[ "$("$python_bin" -c 'import pathlib, stat, sys; print(format(stat.S_IMODE(pathlib.Path(sys.argv[1]).lstat().st_mode), "o"))' "$artifact_dir")" == 700 ]] || fail 'existing artifact directory must have mode 0700'
  shopt -s nullglob dotglob
  entries=("$artifact_dir"/*)
  ((${#entries[@]} == 0)) || fail 'artifact directory must be empty'
  artifact_started=1
else
  (umask 077 && mkdir -m 700 "$artifact_dir") || fail 'cannot create private artifact directory'
  artifact_created=1
  artifact_started=1
fi

build_tmp="$(umask 077 && mktemp -d "$runner_temp/velnor-paired-sdk-build.XXXXXXXX")" || fail 'cannot create private build scratch'
chmod 700 "$build_tmp"
artifact_log="$build_tmp/cargo-artifacts.jsonl"

manifest="$workspace/$fixture/Cargo.toml"
lockfile="$workspace/$fixture/Cargo.lock"
server_file="$workspace/$server"
env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN \
  -u ACTIONS_ID_TOKEN_REQUEST_URL \
  -u ACTIONS_RUNTIME_TOKEN \
  -u GITHUB_OUTPUT \
  -u GITHUB_ENV \
  -u GITHUB_PATH \
  -u GITHUB_STEP_SUMMARY \
  -u GITHUB_TOKEN \
  -u MISE_GITHUB_TOKEN \
  -u GH_TOKEN \
  -u GH_ENTERPRISE_TOKEN \
  -u GH_HOST \
  -u GH_CONFIG_DIR \
  -u NODE_AUTH_TOKEN \
  -u NPM_TOKEN \
  -u DOCKER_AUTH_CONFIG \
  -u TESTCONTAINERS_HOST_OVERRIDE \
  -u DOCKER_CONTEXT \
  -u AWS_ACCESS_KEY_ID \
  -u AWS_SECRET_ACCESS_KEY \
  -u AWS_SESSION_TOKEN \
  -u AZURE_CLIENT_ID \
  -u AZURE_CLIENT_SECRET \
  -u AZURE_TENANT_ID \
  -u GOOGLE_APPLICATION_CREDENTIALS \
  -u SSH_AUTH_SOCK \
  mise --no-config --no-env --no-hooks exec -- mbx build \
  --locked --release \
  --manifest-path "$manifest" \
  --target x86_64-unknown-linux-gnu \
  --message-format=json-render-diagnostics \
  >"$artifact_log"

head_after_build="$(git -C "$workspace" rev-parse --verify HEAD)" || fail 'cannot recheck workspace HEAD after build'
[[ "$head_after_build" == "$source_sha" ]] || fail 'workspace HEAD changed during build'
assert_clean_source

binary_source="$("$python_bin" - "$artifact_log" "$manifest" "$workspace/$fixture/src/main.rs" <<'PY'
import json
import pathlib
import stat
import sys

expected = "velnor-testcontainers-tls-fixture"
expected_manifest = pathlib.Path(sys.argv[2]).resolve(strict=True)
expected_source = pathlib.Path(sys.argv[3]).resolve(strict=True)
matches = []
for line_number, line in enumerate(pathlib.Path(sys.argv[1]).read_text().splitlines(), 1):
    if not line:
        raise SystemExit(f"empty compiler output line {line_number}")
    event = json.loads(line)
    if event.get("reason") != "compiler-artifact":
        continue
    target = event.get("target", {})
    if target.get("name") != expected or "bin" not in target.get("kind", []):
        continue
    manifest_path = event.get("manifest_path")
    source_path = target.get("src_path")
    if not isinstance(manifest_path, str) or not pathlib.Path(manifest_path).is_absolute():
        raise SystemExit("fixture artifact must report an absolute manifest path")
    if not isinstance(source_path, str) or not pathlib.Path(source_path).is_absolute():
        raise SystemExit("fixture artifact must report an absolute source path")
    if pathlib.Path(manifest_path).resolve(strict=True) != expected_manifest:
        raise SystemExit("fixture artifact manifest path does not match the selected package")
    if pathlib.Path(source_path).resolve(strict=True) != expected_source:
        raise SystemExit("fixture artifact source path does not match the selected binary")
    executable = event.get("executable")
    if not isinstance(executable, str) or not pathlib.Path(executable).is_absolute():
        raise SystemExit("fixture executable path must be absolute")
    matches.append(pathlib.Path(executable))

if len(matches) != 1:
    raise SystemExit("compiler output must contain exactly one fixture executable")
path = matches[0]
metadata = path.lstat()
if not stat.S_ISREG(metadata.st_mode) or path.is_symlink() or not metadata.st_mode & 0o111:
    raise SystemExit("fixture executable must be a regular executable file")
with path.open("rb") as binary:
    header = binary.read(20)
if len(header) < 20 or header[:4] != b"\x7fELF" or header[4:6] != b"\x02\x01" or header[18:20] != b"\x3e\x00":
    raise SystemExit("fixture executable must be an ELF x86-64 binary")
print(path)
PY
)" || fail 'could not select a unique ELF x86_64 fixture executable from Cargo output'

binary_name='velnor-testcontainers-tls-fixture'
cp "$binary_source" "$artifact_dir/$binary_name"
chmod 755 "$artifact_dir/$binary_name"
fixture_sha="$(sha256sum -- "$manifest" | awk '{print $1}')"
lock_sha="$(sha256sum -- "$lockfile" | awk '{print $1}')"
server_sha="$(sha256sum -- "$server_file" | awk '{print $1}')"
binary_sha="$(sha256sum -- "$artifact_dir/$binary_name" | awk '{print $1}')"
[[ "$fixture_sha" =~ ^[0-9a-f]{64}$ && "$lock_sha" =~ ^[0-9a-f]{64}$ && "$server_sha" =~ ^[0-9a-f]{64}$ && "$binary_sha" =~ ^[0-9a-f]{64}$ ]] || fail 'sha256sum returned an invalid digest'
{
  printf 'schema=velnor-paired-sdk-artifact-v1\n'
  printf 'source_sha=%s\n' "$source_sha"
  printf 'fixture_manifest_sha256=%s\n' "$fixture_sha"
  printf 'fixture_lock_sha256=%s\n' "$lock_sha"
  printf 'server_sha256=%s\n' "$server_sha"
  printf 'binary_sha256=%s\n' "$binary_sha"
  printf 'target=x86_64-unknown-linux-gnu\n'
} > "$artifact_dir/manifest.txt"
chmod 644 "$artifact_dir/manifest.txt"
"$python_bin" - "$artifact_dir" <<'PY' || fail 'built artifacts do not match the fixed two-file shape'
import os
import pathlib
import stat
import sys

root = pathlib.Path(sys.argv[1])
if set(os.listdir(root)) != {"manifest.txt", "velnor-testcontainers-tls-fixture"}:
    raise SystemExit(1)
for name, mode in (("manifest.txt", 0o644), ("velnor-testcontainers-tls-fixture", 0o755)):
    path = root / name
    metadata = path.lstat()
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
        raise SystemExit(1)
    if stat.S_IMODE(metadata.st_mode) != mode or metadata.st_size == 0:
        raise SystemExit(1)
PY
printf 'binary_sha256=%s\n' "$binary_sha"
