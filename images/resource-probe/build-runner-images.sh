#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: %s [--prebuilt-binaries]\n' "$0" >&2
}

prebuilt_binaries=0
if [[ "$#" -gt 1 ]]; then
  usage
  exit 2
fi
if [[ "$#" -eq 1 ]]; then
  case "$1" in
    --prebuilt-binaries) prebuilt_binaries=1 ;;
    *)
      usage
      exit 2
      ;;
  esac
fi
readonly prebuilt_binaries

readonly runner_image='velnor-runner:linux-amd64'
readonly dind_image='velnor-dind:linux-amd64'
readonly target_dir="${CARGO_TARGET_DIR:-crates/velnor-runner/target}"
readonly runner_version="$(python3 - .velnor/runner.lock <<'PY'
import pathlib
import sys
import tomllib

lock = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
version = lock["runner_version"]
if not isinstance(version, str) or not version:
    raise SystemExit("runner version is missing from .velnor/runner.lock")
print(version)
PY
)"
readonly source_revision="${VELNOR_SOURCE_REVISION:?missing source revision}"
readonly binary="$target_dir/x86_64-unknown-linux-musl/release/velnor-resource-probe"
readonly storage_init_binary="$target_dir/x86_64-unknown-linux-musl/release/velnor-storage-init"
readonly job_start_hook_binary="$target_dir/x86_64-unknown-linux-musl/release/velnor-job-start-hook"

[[ "$source_revision" =~ ^[0-9a-f]{40}$ ]]
checked_out_revision="$(git rev-parse HEAD)"
[[ "$checked_out_revision" == "$source_revision" ]]
git diff --quiet HEAD -- || { printf 'The source tree differs from its release commit.\n' >&2; exit 1; }
[[ -z "$(git ls-files --others --exclude-standard)" ]] || {
  printf 'The source tree has untracked files.\n' >&2
  exit 1
}
[[ -f "$binary" && ! -L "$binary" ]]

python3 -B images/resource-probe/test_verify_runner_listener.py

if [[ "$prebuilt_binaries" -eq 0 ]]; then
  project_rust_version="$(python3 - mise.toml <<'PY'
import pathlib
import re
import sys
import tomllib

config = tomllib.loads(pathlib.Path(sys.argv[1]).read_text())
tools = config.get("tools", {})
rust = tools.get("rust") if isinstance(tools, dict) else None
if isinstance(rust, dict):
    rust = rust.get("version")
if not isinstance(rust, str) or re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", rust) is None:
    raise SystemExit("mise.toml must declare an exact tools.rust version")
print(rust)
PY
  )"
  mise --no-config --no-env --no-hooks exec "rust@$project_rust_version" -- cargo build \
    --locked \
    --manifest-path crates/velnor-runner/Cargo.toml \
    --package velnor-resource-probe \
    --bin velnor-storage-init \
    --bin velnor-job-start-hook \
    --release \
    --target x86_64-unknown-linux-musl
fi
[[ "$(git rev-parse HEAD)" == "$source_revision" ]]
git diff --quiet HEAD -- || { printf 'The source tree changed during the build.\n' >&2; exit 1; }
[[ -z "$(git ls-files --others --exclude-standard)" ]] || {
  printf 'The source tree has untracked files.\n' >&2
  exit 1
}
[[ -f "$storage_init_binary" && ! -L "$storage_init_binary" ]]
[[ -f "$job_start_hook_binary" && ! -L "$job_start_hook_binary" ]]

file_description="$(file -b "$binary")"
case "$file_description" in
  *'ELF 64-bit LSB'*x86-64*static*) ;;
  *) printf 'resource probe is not a static ELF64 x86-64 executable: %s\n' "$file_description" >&2; exit 1 ;;
esac

storage_init_description="$(file -b "$storage_init_binary")"
case "$storage_init_description" in
  *'ELF 64-bit LSB'*x86-64*static*) ;;
  *) printf 'storage initializer is not a static ELF64 x86-64 executable: %s\n' "$storage_init_description" >&2; exit 1 ;;
esac

job_start_hook_description="$(file -b "$job_start_hook_binary")"
case "$job_start_hook_description" in
  *'ELF 64-bit LSB'*x86-64*static*) ;;
  *) printf 'job-start hook is not a static ELF64 x86-64 executable: %s\n' "$job_start_hook_description" >&2; exit 1 ;;
esac

python3 - "$binary" "$source_revision" <<'PY'
import pathlib
import sys

binary = pathlib.Path(sys.argv[1]).read_bytes()
revision = sys.argv[2].encode("ascii")
if revision not in binary:
    raise SystemExit("resource probe does not contain its source revision")
PY

workdir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/velnor-runner-image.XXXXXXXX")"
readonly workdir
readonly context="$workdir/resource-probe"
readonly runner_context="$workdir/storage-init"
readonly hook_context="$workdir/job-start-hook"
readonly wrapper_context="$workdir/job-started-hook"
mkdir -m 0755 "$context" "$runner_context" "$hook_context" "$wrapper_context"
trap 'rm -rf "$workdir"' EXIT

cp "$binary" "$context/resource-probe"
chmod 0555 "$context/resource-probe"
cp "$storage_init_binary" "$runner_context/storage-init"
chmod 0555 "$runner_context/storage-init"
cp "$job_start_hook_binary" "$hook_context/job-start-hook"
chmod 0555 "$hook_context/job-start-hook"
cp images/runner/ubuntu-26.04/job-started-hook.sh "$wrapper_context/job-started-hook.sh"
chmod 0555 "$wrapper_context/job-started-hook.sh"
expected_sha256="$(python3 - "$context/resource-probe" <<'PY'
import hashlib
import pathlib
import sys

print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
)"
storage_init_sha256="$(python3 - "$runner_context/storage-init" <<'PY'
import hashlib
import pathlib
import sys

print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
)"
job_start_hook_sha256="$(python3 - "$hook_context/job-start-hook" <<'PY'
import hashlib
import pathlib
import sys

print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
)"
job_started_wrapper_sha256="$(python3 - images/runner/ubuntu-26.04/job-started-hook.sh <<'PY'
import hashlib
import pathlib
import sys

print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
)"
readonly -a runtime_args=(
  --rm
  --pull=never
  --platform linux/amd64
  --network none
  --read-only
  --cap-drop=ALL
  --security-opt no-new-privileges:true
  --user 65532:65532
)

docker buildx build \
  --platform linux/amd64 \
  --resource memory=20g \
  --resource cpu-quota=400000 \
  --build-context "resource_probe=$context" \
  --build-context "storage_init=$runner_context" \
  --build-context "job_start_hook=$hook_context" \
  --build-context "job_started_hook=$wrapper_context" \
  --load \
  --tag "$runner_image" \
  images/runner/ubuntu-26.04

docker buildx build \
  --platform linux/amd64 \
  --build-context "resource_probe=$context" \
  --load \
  --tag "$dind_image" \
  images/dind

for image in "$runner_image" "$dind_image"; do
  architecture="$(docker image inspect --format '{{.Architecture}}' "$image")"
  [[ "$architecture" == amd64 ]]
  image_id="$(docker image inspect --format '{{.Id}}' "$image")"
  [[ "$image_id" =~ ^sha256:[0-9a-f]{64}$ ]]
  guest_arch="$(docker run "${runtime_args[@]}" \
    --entrypoint /usr/bin/uname "$image" -m)"
  [[ "$guest_arch" == x86_64 ]]
  output="$(docker run "${runtime_args[@]}" \
    --entrypoint /usr/bin/sha256sum "$image" /velnor/resource-probe)"
  actual_sha256="${output%% *}"
  [[ "$actual_sha256" == "$expected_sha256" ]]
  printf '%s image_id=%s image_arch=%s guest_arch=%s resource_probe_sha256=%s\n' \
    "$image" "$image_id" "$architecture" "$guest_arch" "$actual_sha256"
done

docker run --interactive "${runtime_args[@]}" \
  --tmpfs /opt/runner/_diag:rw,nosuid,nodev,noexec,size=16m,uid=65532,gid=65532 \
  --entrypoint /usr/bin/python3 "$runner_image" - "$runner_version" \
  < images/resource-probe/verify_runner_listener.py

actual_storage_init_sha256="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/sha256sum "$runner_image" /velnor/storage-init)"
actual_storage_init_sha256="${actual_storage_init_sha256%% *}"
[[ "$actual_storage_init_sha256" == "$storage_init_sha256" ]]
storage_init_mode="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/stat "$runner_image" -c %a /velnor/storage-init)"
[[ "$storage_init_mode" == 555 ]]
actual_job_start_hook_sha256="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/sha256sum "$runner_image" /velnor/job-start-hook)"
actual_job_start_hook_sha256="${actual_job_start_hook_sha256%% *}"
[[ "$actual_job_start_hook_sha256" == "$job_start_hook_sha256" ]]
job_start_hook_mode="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/stat "$runner_image" -c %a /velnor/job-start-hook)"
[[ "$job_start_hook_mode" == 555 ]]
job_start_hook_owner="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/stat "$runner_image" -c %u:%g /velnor/job-start-hook)"
[[ "$job_start_hook_owner" == 0:0 ]]
actual_wrapper_sha256="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/sha256sum "$runner_image" /usr/local/bin/velnor-job-start-hook)"
actual_wrapper_sha256="${actual_wrapper_sha256%% *}"
[[ "$actual_wrapper_sha256" == "$job_started_wrapper_sha256" ]]
wrapper_mode="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/stat "$runner_image" -c %a /usr/local/bin/velnor-job-start-hook)"
[[ "$wrapper_mode" == 555 ]]
wrapper_owner="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/stat "$runner_image" -c %u:%g /usr/local/bin/velnor-job-start-hook)"
[[ "$wrapper_owner" == 0:0 ]]

printf 'Runner and DinD images contain the same static resource probe from source %s (%s)\n' \
  "$source_revision" "$expected_sha256"
printf 'Runner image contains the static storage initializer from source %s (%s, mode %s)\n' \
  "$source_revision" "$actual_storage_init_sha256" "$storage_init_mode"
printf 'Runner image contains the static job-start hook from source %s (%s, mode %s)\n' \
  "$source_revision" "$actual_job_start_hook_sha256" "$job_start_hook_mode"
printf 'Runner image contains the fixed job-start hook wrapper (%s, mode %s)\n' \
  "$actual_wrapper_sha256" "$wrapper_mode"
