# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
: "${base:?}" "${repo:?}" "${sha:?}"
[[ "$sha" == "$GITHUB_SHA" && "$sha" == "$GITHUB_WORKFLOW_SHA" ]]
__POLICY__
ready=false
for ((retry=0;retry<300;retry++)); do
  if published=$(list_runs publish.yml); then ready=true; break; fi
  sleep 10
done
[[ "$ready" == true ]]
publish_id=$(cut -f1 <<< "$published")
publish_attempt=$(cut -f2 <<< "$published")
ci=$(list_runs ci.yml)
ci_id=$(cut -f1 <<< "$ci")
ci_attempt=$(cut -f2 <<< "$ci")
validate_required "$ci_id" "$ci_attempt"
fetch_publisher_jobs
root="$RUNNER_TEMP/runner-candidates"
trusted="$RUNNER_TEMP/velnor/bin/velnor-actions-__VERSION__"
[[ -x "$trusted" && ! -L "$trusted" ]]
"$trusted" image-maintenance admit-runner-images --root "$GITHUB_WORKSPACE" --source-commit "$sha" \
  --runner-archive "$root/runner.oci.tar" --dind-archive "$root/dind.oci.tar" \
  --probe-archive "$root/resource-probe.oci.tar" --descriptors-output "$root/descriptors.json"
mkdir -m 700 "$base/docker"
export DOCKER_CONFIG="$base/docker"
registry_auth=$(printf '%s:%s' "$GITHUB_ACTOR" "$GH_TOKEN" | base64 | tr -d '\n')
printf '{"auths":{"ghcr.io":{"auth":"%s"}}}\n' "$registry_auth" > "$DOCKER_CONFIG/config.json"
unset registry_auth
trap 'rm -rf "$DOCKER_CONFIG"' EXIT
for role in runner dind resource-probe; do
  profile=${role//-/_}
  repository="ghcr.io/tailrocks/velnor-$role"
  artifact="velnor-image-publication-$role-$publish_id-$publish_attempt"
  artifact_id=$(find_artifact "$publish_id" "$artifact")
  mkdir -m 700 "$base/$role"
  download_artifact "$artifact_id" "$base/$role" "velnor-image-publication-$role.jsonl"
  journal="$base/$role/velnor-image-publication-$role.jsonl"
  record="$root/$role-image-publication.json"
  digest=$(validate_journal "$journal" "$record")
  validate_publisher_jobs "$base/jobs.json"
  (ulimit -f 32768; gh attestation verify "oci://$repository@$digest" --bundle-from-oci \
    --repo "$repo" --source-ref refs/heads/main --source-digest "$sha" \
    --signer-workflow "$repo/.github/workflows/publish.yml" --signer-digest "$sha" \
    --predicate-type https://slsa.dev/provenance/v1 --format json > "$base/$role/verified.json")
  validate_digest_attestation "$base/$role/verified.json"
  jq -e --arg profile "$profile" --arg digest "$digest" '.images[$profile].registry_digest == $digest' "$root/descriptors.json" >/dev/null
 done
[[ "$(list_runs publish.yml)" == "$published" && "$(list_runs ci.yml)" == "$ci" ]]
[[ "$(gh api "repos/$repo/commits/main" --jq .sha)" == "$sha" ]]
target="$(cd -- "${CARGO_TARGET_DIR:-crates/velnor-runner/target}" && pwd -P)"
formatter="$target/x86_64-unknown-linux-gnu/debug/velnor-resource-probe-release"
[[ -x "$formatter" && ! -L "$formatter" ]]
rm -rf "$DOCKER_CONFIG"
(
set -- "$formatter" write-runner-image-manifest "$root/descriptors.json" .velnor/runner.lock RUNNER_IMAGE_MANIFEST.json
__SCRUB_EXEC__
)
sha256sum RUNNER_IMAGE_MANIFEST.json LICENSE NOTICE > SHA256SUMS
