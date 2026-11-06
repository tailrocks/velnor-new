#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
: "${GITHUB_WORKSPACE:?}" "${GITHUB_SHA:?}" "${GITHUB_EVENT_NAME:?}" "${RUNNER_TEMP:?}" "${VELNOR_NATIVE_IMAGE_ACTIONS:?}"
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]]
root="$GITHUB_WORKSPACE"
base="$RUNNER_TEMP/velnor-ci-image-candidates"
[[ ! -e "$base" ]]
umask 077
mkdir -m 700 "$base" "$base/job" "$base/images" "$base/probe-context"
"$VELNOR_NATIVE_IMAGE_ACTIONS" image-maintenance install-buildx --root "$root" --runner-temp "$RUNNER_TEMP"
export DOCKER_CONFIG="$RUNNER_TEMP/.velnor-job-image-candidate/docker-config"
main_push=false
if [[ "$GITHUB_EVENT_NAME" == push && "${GITHUB_REF:-}" == refs/heads/main ]]; then
  main_push=true
  "$VELNOR_NATIVE_IMAGE_ACTIONS" image-maintenance check-job-source --root "$root" --source-commit "$GITHUB_SHA" --source-closure-path "$base/job/job-image-native-source-closure.json"
  "$VELNOR_NATIVE_IMAGE_ACTIONS" image-maintenance build-job-candidate --root "$root" --source-commit "$GITHUB_SHA" --runner-temp "$RUNNER_TEMP" --archive-path "$base/job/job-image.oci.tar" --record-path "$base/job/job-image-candidate.json" --source-closure-path "$base/job/job-image-native-source-closure.json"
else
  docker buildx build --platform linux/amd64 --file "$root/images/job/ubuntu-26.04/Dockerfile" --build-arg "VELNOR_SOURCE_SHA=$GITHUB_SHA" --provenance=false --output "type=oci,dest=$base/job/job-image.oci.tar,compression=gzip,force-compression=true" "$root/images/job/ubuntu-26.04"
fi
for role in runner dind; do
  if [[ "$role" == runner ]]; then context="$root/images/runner/ubuntu-26.04"; else context="$root/images/dind"; fi
  docker buildx build --platform linux/amd64 --file "$context/Dockerfile" --build-arg "VELNOR_SOURCE_SHA=$GITHUB_SHA" --provenance=false --output "type=oci,dest=$base/images/$role.oci.tar,compression=gzip,force-compression=true" "$context"
done
probe_target="${CARGO_TARGET_DIR:-$root/crates/velnor-runner/target}"
probe_binary="$probe_target/x86_64-unknown-linux-musl/release/velnor-resource-probe"
[[ -f "$probe_binary" && ! -L "$probe_binary" ]]
file_description="$(file -b "$probe_binary")"
case "$file_description" in *'ELF 64-bit LSB'*x86-64*static*) ;; *) exit 1 ;; esac
cp "$probe_binary" "$base/probe-context/resource-probe"
cp "$root/images/resource-probe/Dockerfile" "$base/probe-context/Dockerfile"
chmod 0555 "$base/probe-context/resource-probe"
docker buildx build --platform linux/amd64 --network none --file "$base/probe-context/Dockerfile" --build-arg "SOURCE_SHA=$GITHUB_SHA" --provenance=false --output "type=oci,dest=$base/images/resource-probe.oci.tar,compression=gzip,force-compression=true" "$base/probe-context"
if [[ "$main_push" == true ]]; then
  "$VELNOR_NATIVE_IMAGE_ACTIONS" image-maintenance record-runner-candidates --root "$root" --source-commit "$GITHUB_SHA" --runner-archive "$base/images/runner.oci.tar" --dind-archive "$base/images/dind.oci.tar" --probe-archive "$base/images/resource-probe.oci.tar" --output-directory "$base/images"
else
  "$VELNOR_NATIVE_IMAGE_ACTIONS" image-maintenance admit-runner-images --root "$root" --source-commit "$GITHUB_SHA" --runner-archive "$base/images/runner.oci.tar" --dind-archive "$base/images/dind.oci.tar" --probe-archive "$base/images/resource-probe.oci.tar" --descriptors-output "$base/descriptors.json"
fi
