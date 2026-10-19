#!/usr/bin/env bash
set -euo pipefail

readonly runner_image='velnor-runner:linux-amd64'
readonly dind_image='velnor-dind:linux-amd64'
readonly target_dir="${CARGO_TARGET_DIR:-crates/velnor-runner/target}"
readonly source_revision="${VELNOR_SOURCE_REVISION:?missing source revision}"
readonly binary="$target_dir/x86_64-unknown-linux-musl/release/velnor-resource-probe"
readonly storage_init_binary="$target_dir/x86_64-unknown-linux-musl/release/velnor-storage-init"

[[ "$source_revision" =~ ^[0-9a-f]{40}$ ]]
checked_out_revision="$(git rev-parse HEAD)"
[[ "$checked_out_revision" == "$source_revision" ]]
git diff --quiet HEAD -- || { printf 'The source tree differs from its release commit.\n' >&2; exit 1; }
[[ -z "$(git ls-files --others --exclude-standard)" ]] || {
  printf 'The source tree has untracked files.\n' >&2
  exit 1
}
[[ -f "$binary" && ! -L "$binary" ]]

mise --no-config --no-env --no-hooks exec rust@1.99.1 -- cargo build \
  --locked \
  --manifest-path crates/velnor-runner/Cargo.toml \
  --package velnor-resource-probe \
  --bin velnor-storage-init \
  --release \
  --target x86_64-unknown-linux-musl
[[ "$(git rev-parse HEAD)" == "$source_revision" ]]
git diff --quiet HEAD -- || { printf 'The source tree changed during the build.\n' >&2; exit 1; }
[[ -z "$(git ls-files --others --exclude-standard)" ]] || {
  printf 'The source tree has untracked files.\n' >&2
  exit 1
}
[[ -f "$storage_init_binary" && ! -L "$storage_init_binary" ]]

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
mkdir -m 0755 "$context" "$runner_context"
trap 'rm -rf "$workdir"' EXIT

cp "$binary" "$context/resource-probe"
chmod 0555 "$context/resource-probe"
cp "$storage_init_binary" "$runner_context/storage-init"
chmod 0555 "$runner_context/storage-init"
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

actual_storage_init_sha256="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/sha256sum "$runner_image" /velnor/storage-init)"
actual_storage_init_sha256="${actual_storage_init_sha256%% *}"
[[ "$actual_storage_init_sha256" == "$storage_init_sha256" ]]
storage_init_mode="$(docker run "${runtime_args[@]}" \
  --entrypoint /usr/bin/stat "$runner_image" -c %a /velnor/storage-init)"
[[ "$storage_init_mode" == 555 ]]

printf 'Runner and DinD images contain the same static resource probe from source %s (%s)\n' \
  "$source_revision" "$expected_sha256"
printf 'Runner image contains the static storage initializer from source %s (%s, mode %s)\n' \
  "$source_revision" "$actual_storage_init_sha256" "$storage_init_mode"
