# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
root="$RUNNER_TEMP/runner-candidates"
ids=()
for role in runner dind resource-probe; do
  case "$role" in
    runner) record=runner-image-publication.json ;;
    dind) record=dind-image-publication.json ;;
    resource-probe) record=resource-probe-image-publication.json ;;
  esac
  manifest=$(jq -er '.oci.platform_manifest_digest' "$root/$record")
  config=$(jq -er '.oci.image_configuration_digest' "$root/$record")
  [[ "$manifest" =~ ^sha256:[0-9a-f]{64}$ && "$config" =~ ^sha256:[0-9a-f]{64}$ ]]
  docker image load --input "$root/$role.oci.tar"
  if [[ "$(docker image inspect --format '{{.Id}}' "$manifest" 2>/dev/null || true)" == "$manifest" ]]; then
    id=$manifest
  else
    [[ "$(docker image inspect --format '{{.Id}}' "$config")" == "$config" ]]
    id=$config
  fi
  ids+=("$id")
done
target="$(cd -- "${CARGO_TARGET_DIR:-crates/velnor-runner/target}" && pwd -P)"
qualifier="$target/x86_64-unknown-linux-gnu/debug/velnor-resource-probe-release"
test -f "$qualifier" && test ! -L "$qualifier"
"$qualifier" qualify-runner-images "$GITHUB_WORKSPACE" "$GITHUB_SHA" "${ids[0]}" "${ids[1]}" "${ids[2]}" /var/run/docker.sock "$root/runtime.json"
"$VELNOR_NATIVE_IMAGE_ACTIONS" image-maintenance write-runner-qualification \
  --root "$GITHUB_WORKSPACE" --source-commit "$GITHUB_SHA" \
  --runner-archive "$root/runner.oci.tar" --runner-record "$root/runner-image-publication.json" \
  --dind-archive "$root/dind.oci.tar" --dind-record "$root/dind-image-publication.json" \
  --probe-archive "$root/resource-probe.oci.tar" --probe-record "$root/resource-probe-image-publication.json" \
  --runtime-result "$root/runtime.json" --producer-run-id "$PRODUCER_RUN_ID" \
  --producer-run-attempt "$PRODUCER_RUN_ATTEMPT" --qualifier-job-id "$QUALIFIER_JOB_ID" \
  --output "$root/RUNNER_IMAGE_QUALIFICATION.json"
