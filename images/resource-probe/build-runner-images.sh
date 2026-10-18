#!/usr/bin/env bash
set -euo pipefail

readonly runner_image='velnor-runner:linux-amd64'
readonly dind_image='velnor-dind:linux-amd64'
readonly target_dir="${CARGO_TARGET_DIR:-crates/velnor-runner/target}"
readonly source_revision="${VELNOR_SOURCE_REVISION:?missing source revision}"
readonly binary="$target_dir/x86_64-unknown-linux-musl/release/velnor-resource-probe"

[[ "$source_revision" =~ ^[0-9a-f]{40}$ ]]
[[ -f "$binary" && ! -L "$binary" ]]

file_description="$(file -b "$binary")"
case "$file_description" in
  *'ELF 64-bit LSB'*x86-64*static*) ;;
  *) printf 'resource probe is not a static ELF64 x86-64 executable: %s\n' "$file_description" >&2; exit 1 ;;
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
mkdir -m 0755 "$context"
trap 'rm -rf "$workdir"' EXIT

cp "$binary" "$context/resource-probe"
chmod 0555 "$context/resource-probe"
expected_sha256="$(python3 - "$context/resource-probe" <<'PY'
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

printf 'Runner and DinD images contain the same static resource probe from source %s (%s)\n' \
  "$source_revision" "$expected_sha256"
