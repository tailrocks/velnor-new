# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -eu
: "${GH_TOKEN:?}" "${VELNOR_IMAGE_SOURCE_SHA:?}" "${IMAGE_PROFILE:?}" "${IMAGE_REPOSITORY:?}"
repo=tailrocks/velnor-new
sha=$VELNOR_IMAGE_SOURCE_SHA
case "$sha" in *[!0-9a-f]*|'') exit 1;; esac
[ "${#sha}" -eq 40 ]
base="$RUNNER_TEMP/velnor-native-image-$IMAGE_PROFILE"
[ ! -e "$base" ]
umask 077
mkdir -m 700 "$base"
__VELNOR_CI_IMAGE_LOOKUP__
__VELNOR_CI_IMAGE_DOWNLOAD__
ready=false
retry=0
while [ "$retry" -lt 300 ]; do
  if ci=$(list_runs ci.yml) && {
    if [ "$IMAGE_PROFILE" = job ]; then
      attestor=$(list_runs job-image-candidate.yml)
    else
      qualifier=$(list_runs runner-image-qualification.yml)
    fi
  }; then
    ready=true
    break
  fi
  retry=$((retry + 1))
  sleep 10
done
[ "$ready" = true ]
ci_id=$(printf '%s\n' "$ci" | cut -f1)
ci_attempt=$(printf '%s\n' "$ci" | cut -f2)
validate_required "$ci_id" "$ci_attempt"
if [ "$IMAGE_PROFILE" = job ]; then
  attestor_id=$(printf '%s\n' "$attestor" | cut -f1)
  attestor_attempt=$(printf '%s\n' "$attestor" | cut -f2)
else
  qualifier_id=$(printf '%s\n' "$qualifier" | cut -f1)
  qualifier_attempt=$(printf '%s\n' "$qualifier" | cut -f2)
fi
case "$IMAGE_PROFILE" in
  job) artifact="velnor-job-image-candidate-$ci_attempt"; archive=job-image.oci.tar; record=job-image-candidate.json ;;
  runner) artifact="image-assets-$ci_attempt"; archive=runner.oci.tar; record=runner-image-publication.json ;;
  dind) artifact="image-assets-$ci_attempt"; archive=dind.oci.tar; record=dind-image-publication.json ;;
  resource-probe) artifact="image-assets-$ci_attempt"; archive=resource-probe.oci.tar; record=resource-probe-image-publication.json ;;
  *) exit 1 ;;
esac
artifact_id=$(find_artifact "$ci_id" "$artifact")
mkdir -m 700 "$base/candidate"
if [ "$IMAGE_PROFILE" = job ]; then
  download_artifact "$artifact_id" "$base/candidate" job-image.oci.tar job-image-candidate.json job-image-native-source-closure.json
else
  download_artifact "$artifact_id" "$base/candidate" runner.oci.tar dind.oci.tar resource-probe.oci.tar runner-image-publication.json dind-image-publication.json resource-probe-image-publication.json
fi
if [ "$IMAGE_PROFILE" != job ]; then
  proof_artifact="velnor-runner-image-qualification-$qualifier_attempt"
  proof_id=$(find_artifact "$qualifier_id" "$proof_artifact")
  mkdir -m 700 "$base/qualification"
  download_artifact "$proof_id" "$base/qualification" RUNNER_IMAGE_QUALIFICATION.json
fi
mkdir -m 700 "$base/docker" "$base/scratch"
export DOCKER_CONFIG="$base/docker"
registry_auth=$(printf '%s:%s' "$GITHUB_ACTOR" "$GH_TOKEN" | base64 | tr -d '\n')
printf '{"auths":{"ghcr.io":{"auth":"%s"}}}\n' "$registry_auth" > "$DOCKER_CONFIG/config.json"
unset registry_auth
chmod 600 "$DOCKER_CONFIG/config.json"
printf 'DOCKER_CONFIG=%s\n' "$DOCKER_CONFIG" >> "$GITHUB_ENV"
receipt="$RUNNER_TEMP/velnor-image-publication-$IMAGE_PROFILE.jsonl"
set -- image-maintenance publish-image --profile "$IMAGE_PROFILE" --source-commit "$sha" \
  --producer-run-id "$ci_id" --producer-run-attempt "$ci_attempt" --artifact-id "$artifact_id" \
  --archive-path "$base/candidate/$archive" --record-path "$base/candidate/$record" \
  --private-scratch "$base/scratch" --receipt-output "$receipt"
if [ "$IMAGE_PROFILE" = job ]; then
  set -- "$@" --job-attestation-run-id "$attestor_id" --job-attestation-run-attempt "$attestor_attempt"
else
  set -- "$@" --qualification-proof "$base/qualification/RUNNER_IMAGE_QUALIFICATION.json"
fi
"$VELNOR_TRUSTED_ACTIONS" "$@"
receipt_profile=$IMAGE_PROFILE
[ "$receipt_profile" != resource-probe ] || receipt_profile=resource_probe
digest=$(jq -ser --arg sha "$sha" --arg profile "$receipt_profile" --arg repo "$IMAGE_REPOSITORY" \
  'select(length == 2) | .[1] | select(.schema == "velnor.image-publication.v1" and .status == "verified" and .source_commit == $sha and .profile == $profile and .repository == $repo) | .registry_digest | select(test("^sha256:[0-9a-f]{64}$"))' "$receipt")
printf 'digest=%s\n' "$digest" >> "$GITHUB_OUTPUT"
