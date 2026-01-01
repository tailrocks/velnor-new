#!/usr/bin/env bash
set -euo pipefail

readonly image='velnor-resource-probe:linux-amd64'
readonly archive='velnor-resource-probe-linux-amd64.tar'
readonly target_dir="${CARGO_TARGET_DIR:-crates/velnor-runner/target}"
readonly source_sha="${VELNOR_SOURCE_SHA:?missing exact source SHA}"
readonly authority_sha="${VELNOR_WORKFLOW_AUTHORITY_SHA:?missing workflow authority SHA}"

[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]]
[[ "$authority_sha" =~ ^[0-9a-f]{40}$ ]]
[[ "${GITHUB_REPOSITORY:?missing repository}" == tailrocks/velnor-new ]]
[[ "${GITHUB_REF:?missing source ref}" == refs/heads/main ]]
[[ "${GITHUB_SHA:?missing workflow SHA}" == "$source_sha" ]]

python3 -B images/resource-probe/test_write_manifest.py

workdir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/resource-probe-build.XXXXXXXX")"
readonly workdir
readonly context="$workdir/context"
readonly smoke_root="$workdir/docker-root"
readonly smoke_output="$workdir/smoke-output.jsonl"
readonly smoke_error="$workdir/smoke-error.log"
readonly inspect_json="$workdir/loaded-inspect.json"
mkdir -m 0755 "$context" "$smoke_root"
trap 'rm -rf "$workdir"' EXIT

cp "$target_dir/x86_64-unknown-linux-musl/release/velnor-resource-probe" "$context/resource-probe"
cp images/resource-probe/Dockerfile "$context/Dockerfile"
chmod 0555 "$context/resource-probe"
file_description="$(file -b "$context/resource-probe")"
case "$file_description" in
  *'ELF 64-bit LSB'*x86-64*static*) ;;
  *) printf 'resource probe is not a static ELF64 x86-64 executable: %s\n' "$file_description" >&2; exit 1 ;;
esac

docker build --platform linux/amd64 --network none --build-arg "SOURCE_SHA=$source_sha" --tag "$image" "$context"
chmod 0755 "$smoke_root"
docker run --rm --pull=never --platform linux/amd64 --network none --read-only --cap-drop=ALL --security-opt no-new-privileges:true --user 65532:65532 --mount "type=bind,source=$smoke_root,target=/velnor/docker-root,readonly" "$image" > "$smoke_output" 2> "$smoke_error"
test ! -s "$smoke_error"
python3 images/resource-probe/validate_smoke.py "$smoke_output" "$smoke_root"
docker image save --platform linux/amd64 --output "$archive" "$image"
test -s "$archive"
docker image load --platform linux/amd64 --input "$archive"
docker image inspect --platform linux/amd64 "$image" > "$inspect_json"
python3 images/resource-probe/write_manifest.py "$inspect_json" "$archive" RESOURCE_PROBE_MANIFEST.json

printf 'validated linux/amd64 resource probe image %s\n' "$source_sha"
