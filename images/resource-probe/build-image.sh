#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

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

readonly release_validator="${VELNOR_RESOURCE_PROBE_RELEASE_BIN:?missing native resource-probe release validator}"
[[ "$release_validator" == /* && -f "$release_validator" && -x "$release_validator" && ! -L "$release_validator" ]] \
  || { printf 'native resource-probe release validator must be an absolute executable regular file\n' >&2; exit 1; }

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
if ! docker run --rm --pull=never --platform linux/amd64 --cgroupns=host --pid=host --network none --read-only --cap-drop=ALL --security-opt no-new-privileges:true --user 65532:65532 --mount "type=bind,source=$smoke_root,target=/velnor/docker-root,readonly" --mount "type=bind,source=/sys/fs/cgroup,target=/sys/fs/cgroup,readonly" "$image" > "$smoke_output" 2> "$smoke_error"; then
  cat "$smoke_error" >&2
  exit 1
fi
if [[ -s "$smoke_error" ]]; then
  cat "$smoke_error" >&2
  exit 1
fi
"$release_validator" validate-smoke "$smoke_output" "$smoke_root"
docker image save --platform linux/amd64 --output "$archive" "$image"
test -s "$archive"
docker image load --platform linux/amd64 --input "$archive"
docker image inspect --platform linux/amd64 "$image" > "$inspect_json"
"$release_validator" write-manifest "$inspect_json" "$archive" RESOURCE_PROBE_MANIFEST.json

if docker image inspect --platform linux/amd64 velnor-runner:linux-amd64 >/dev/null 2>&1 \
  && docker image inspect --platform linux/amd64 velnor-dind:linux-amd64 >/dev/null 2>&1; then
  if [[ ! -x "$target_dir/x86_64-unknown-linux-musl/release/velnor-storage-init" || ! -x "$target_dir/x86_64-unknown-linux-musl/release/velnor-job-start-hook" ]]; then
    if [[ -n "${VELNOR_TOOL_RUST-}" ]] && command -v mise >/dev/null 2>&1; then
      mise --no-config --no-env --no-hooks exec "rust@${VELNOR_TOOL_RUST}" -- cargo build --manifest-path crates/velnor-runner/Cargo.toml --locked --release --target x86_64-unknown-linux-musl -p velnor-resource-probe --bin velnor-storage-init --bin velnor-job-start-hook
    else
      cargo build --manifest-path crates/velnor-runner/Cargo.toml --locked --release --target x86_64-unknown-linux-musl -p velnor-resource-probe --bin velnor-storage-init --bin velnor-job-start-hook
    fi
  fi
  stage_root="$workdir/stages"
  mkdir -m 0755 \
    "$stage_root" \
    "$stage_root/resource-probe" \
    "$stage_root/storage-init" \
    "$stage_root/job-start-hook" \
    "$stage_root/job-started-hook"
  cp "$context/resource-probe" "$stage_root/resource-probe/resource-probe"
  cp "$target_dir/x86_64-unknown-linux-musl/release/velnor-storage-init" "$stage_root/storage-init/storage-init"
  cp "$target_dir/x86_64-unknown-linux-musl/release/velnor-job-start-hook" "$stage_root/job-start-hook/job-start-hook"
  printf '#!/bin/sh\nset -eu\nexec /velnor/job-start-hook "$@"\n' > "$stage_root/job-started-hook/velnor-job-start-hook.sh"
  chmod 0555 \
    "$stage_root/resource-probe/resource-probe" \
    "$stage_root/storage-init/storage-init" \
    "$stage_root/job-start-hook/job-start-hook" \
    "$stage_root/job-started-hook/velnor-job-start-hook.sh"
  for helper_bin in "$stage_root/storage-init/storage-init" "$stage_root/job-start-hook/job-start-hook"; do
    helper_desc="$(file -b "$helper_bin")"
    case "$helper_desc" in
      *'ELF 64-bit LSB'*x86-64*static*) ;;
      *) printf 'worker helper is not a static ELF64 x86-64 executable: %s\n' "$helper_desc" >&2; exit 1 ;;
    esac
  done
  export BUILDX_NO_DEFAULT_OCI_ARTIFACT=1
  docker buildx build \
    --provenance=mode=max \
    --platform linux/amd64 \
    --build-arg "VELNOR_SOURCE_SHA=$source_sha" \
    --build-context "resource_probe=$stage_root/resource-probe" \
    --build-context "storage_init=$stage_root/storage-init" \
    --build-context "job_start_hook=$stage_root/job-start-hook" \
    --build-context "job_started_hook=$stage_root/job-started-hook" \
    --tag velnor-runner:linux-amd64 \
    --load \
    images/runner/ubuntu-26.04
  docker buildx build \
    --provenance=mode=max \
    --platform linux/amd64 \
    --build-arg "VELNOR_SOURCE_SHA=$source_sha" \
    --build-context "resource_probe=$stage_root/resource-probe" \
    --tag velnor-dind:linux-amd64 \
    --load \
    images/dind
fi

printf 'validated linux/amd64 resource probe image %s\n' "$source_sha"
