#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/common.sh"

version="${VELNOR_RELEASE_VERSION:?VELNOR_RELEASE_VERSION is required}"
tag="${VELNOR_RELEASE_TAG:?VELNOR_RELEASE_TAG is required}"
repository="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
commit="${GITHUB_SHA:?GITHUB_SHA is required}"
workflow_ref="${GITHUB_WORKFLOW_REF:?GITHUB_WORKFLOW_REF is required}"
workflow_sha="${GITHUB_WORKFLOW_SHA:?GITHUB_WORKFLOW_SHA is required}"
token="${GH_TOKEN:?GH_TOKEN is required}"
run_id="${VELNOR_BUILD_RUN_ID:?VELNOR_BUILD_RUN_ID is required}"
run_attempt="${VELNOR_BUILD_RUN_ATTEMPT:?VELNOR_BUILD_RUN_ATTEMPT is required}"
repository_id="${VELNOR_REPOSITORY_ID:?VELNOR_REPOSITORY_ID is required}"
expected_branch="${VELNOR_EXPECTED_DEFAULT_BRANCH:?VELNOR_EXPECTED_DEFAULT_BRANCH is required}"
expected_workflow_sha="${VELNOR_EXPECTED_WORKFLOW_SHA:?VELNOR_EXPECTED_WORKFLOW_SHA is required}"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail 'invalid release version'
[[ "$tag" == "v$version" ]] || fail 'release tag does not match version'
[[ "$repository" == "$VELNOR_RELEASE_REPOSITORY" ]] || fail 'unexpected repository'
is_sha "$commit" && is_sha "$workflow_sha" || fail 'source or workflow SHA is malformed'
[[ "$workflow_sha" == "$commit" && "$expected_workflow_sha" == "$commit" ]] \
  || fail 'release source and workflow identity differ'
[[ -n "$token" ]] || fail 'GitHub token is empty'
[[ "$run_id" =~ ^[1-9][0-9]*$ && "$run_attempt" =~ ^[1-9][0-9]*$ ]] \
  || fail 'workflow run identity is malformed'
[[ "$run_id" == "${GITHUB_RUN_ID-}" && "$run_attempt" == "${GITHUB_RUN_ATTEMPT-}" ]] \
  || fail 'artifact run identity differs from publisher run'
[[ "$repository_id" =~ ^[1-9][0-9]*$ && "$repository_id" == "${GITHUB_REPOSITORY_ID-}" ]] \
  || fail 'repository identity is malformed'
[[ "$workflow_ref" == "$repository/.github/workflows/generator-release.yml@refs/heads/$expected_branch" ]] \
  || fail 'workflow ref differs from validated default branch'

workspace_version="$(awk '
  /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
  /^\[/ { in_workspace_package = 0 }
  in_workspace_package && /^version[[:space:]]*=/ {
    gsub(/"/, "", $3)
    print $3
    exit
  }
' Cargo.toml)"
[[ "$workspace_version" == "$version" ]] || fail 'release version differs from workspace package version'

linux_binary="velnor-actions-${version}-x86_64-unknown-linux-gnu"
linux_sidecar="${linux_binary}.sha256"
macos_binary="velnor-actions-${version}-aarch64-apple-darwin"
macos_sidecar="${macos_binary}.sha256"
manifest_name='velnor-actions-release-manifest.json'
linux_bundle="velnor-actions-${version}-linux-provenance.sigstore.json"
macos_bundle="velnor-actions-${version}-macos-provenance.sigstore.json"
manifest_bundle="velnor-actions-${version}-manifest-provenance.sigstore.json"
artifact_names=(
  "${VELNOR_LINUX_ARTIFACT_NAME:?Linux artifact name is required}"
  "${VELNOR_LINUX_BUNDLE_NAME:?Linux bundle name is required}"
  "${VELNOR_MACOS_ARTIFACT_NAME:?macOS artifact name is required}"
  "${VELNOR_MACOS_BUNDLE_NAME:?macOS bundle name is required}"
  "${VELNOR_MANIFEST_ARTIFACT_NAME:?manifest artifact name is required}"
  "${VELNOR_MANIFEST_BUNDLE_NAME:?manifest bundle name is required}"
)
expected_artifact_names=(
  "generator-${version}-linux-assets-${run_id}-${run_attempt}"
  "generator-${version}-linux-provenance-${run_id}-${run_attempt}"
  "generator-${version}-macos-assets-${run_id}-${run_attempt}"
  "generator-${version}-macos-provenance-${run_id}-${run_attempt}"
  "generator-${version}-manifest-assets-${run_id}-${run_attempt}"
  "generator-${version}-manifest-provenance-${run_id}-${run_attempt}"
)
artifact_ids=(
  "${VELNOR_LINUX_ARTIFACT_ID:?Linux artifact ID is required}"
  "${VELNOR_LINUX_BUNDLE_ID:?Linux bundle ID is required}"
  "${VELNOR_MACOS_ARTIFACT_ID:?macOS artifact ID is required}"
  "${VELNOR_MACOS_BUNDLE_ID:?macOS bundle ID is required}"
  "${VELNOR_MANIFEST_ARTIFACT_ID:?manifest artifact ID is required}"
  "${VELNOR_MANIFEST_BUNDLE_ID:?manifest bundle ID is required}"
)
artifact_digests=(
  "${VELNOR_LINUX_ARTIFACT_DIGEST:?Linux artifact digest is required}"
  "${VELNOR_LINUX_BUNDLE_DIGEST:?Linux bundle digest is required}"
  "${VELNOR_MACOS_ARTIFACT_DIGEST:?macOS artifact digest is required}"
  "${VELNOR_MACOS_BUNDLE_DIGEST:?macOS bundle digest is required}"
  "${VELNOR_MANIFEST_ARTIFACT_DIGEST:?manifest artifact digest is required}"
  "${VELNOR_MANIFEST_BUNDLE_DIGEST:?manifest bundle digest is required}"
)

for index in "${!artifact_ids[@]}"; do
  [[ "${artifact_ids[$index]}" =~ ^[1-9][0-9]*$ ]] \
    || fail 'artifact ID output is malformed'
  [[ "${artifact_names[$index]}" == "${expected_artifact_names[$index]}" ]] \
    || fail 'artifact name output does not match this run'
  is_hex_digest "${artifact_digests[$index]}" \
    || fail 'artifact digest output is malformed'
  for ((earlier = 0; earlier < index; earlier++)); do
    [[ "${artifact_ids[$index]}" != "${artifact_ids[$earlier]}" ]] \
      || fail 'artifact IDs are not unique'
  done
done

work_dir="$(mktemp -d)" || fail 'could not create private release scratch'
trap 'rm -rf -- "$work_dir"' EXIT
for index in "${!artifact_ids[@]}"; do
  artifact_metadata="$work_dir/artifact-${artifact_ids[$index]}.json"
  api_get "actions/artifacts/${artifact_ids[$index]}" "$artifact_metadata"
  [[ "$API_STATUS" == '200' ]] || fail "artifact API returned HTTP $API_STATUS"
  jq -e \
    --arg id "${artifact_ids[$index]}" \
    --arg name "${expected_artifact_names[$index]}" \
    --arg digest "sha256:${artifact_digests[$index]}" \
    --argjson run_id "$run_id" \
    --argjson repository_id "$repository_id" \
    --arg source_sha "$commit" \
    --arg branch "$expected_branch" \
    'type == "object"
      and .id == ($id | tonumber)
      and .name == $name
      and .digest == $digest
      and (.size_in_bytes | type == "number" and . > 0)
      and .expired == false
      and .workflow_run.id == $run_id
      and .workflow_run.repository_id == $repository_id
      and .workflow_run.head_repository_id == $repository_id
      and .workflow_run.head_sha == $source_sha
      and .workflow_run.head_branch == $branch' \
    "$artifact_metadata" >/dev/null \
    || fail "artifact ID, digest, run, source, name, or byte metadata mismatch: ${artifact_names[$index]}"
done

verify_exact_asset_tree() {
  local expected actual path
  local -a entries
  expected="$(printf '%s\n' \
    "$linux_binary" "$linux_sidecar" "$macos_binary" "$macos_sidecar" \
    "$manifest_name" "$linux_bundle" "$macos_bundle" "$manifest_bundle" \
    | LC_ALL=C sort)"
  [[ -d assets && ! -L assets ]] || fail 'downloaded asset directory is missing or unsafe'
  shopt -s nullglob dotglob
  entries=(assets/*)
  shopt -u nullglob dotglob
  [[ "${#entries[@]}" -eq 8 ]] || fail 'downloaded artifact set does not contain eight files'
  for path in "${entries[@]}"; do
    [[ -f "$path" && ! -L "$path" ]] || fail 'downloaded artifact contains a directory or symlink'
  done
  actual="$(for path in "${entries[@]}"; do printf '%s\n' "${path##*/}"; done | LC_ALL=C sort)"
  [[ "$actual" == "$expected" ]] || fail 'downloaded artifact filenames do not match release plan'
}

verify_exact_asset_tree
linux_sha="$(verify_sidecar assets "$linux_sidecar" "$linux_binary")"
macos_sha="$(verify_sidecar assets "$macos_sidecar" "$macos_binary")"
verify_bundle "assets/$linux_bundle"
verify_bundle "assets/$macos_bundle"
verify_bundle "assets/$manifest_bundle"

expected_manifest="$(jq -cnS \
  --arg version "$version" \
  --arg repository "$repository" \
  --arg commit "$commit" \
  --arg tag "$tag" \
  --arg linux_target 'x86_64-unknown-linux-gnu' \
  --arg linux_binary "$linux_binary" \
  --arg linux_sha "$linux_sha" \
  --arg macos_target 'aarch64-apple-darwin' \
  --arg macos_binary "$macos_binary" \
  --arg macos_sha "$macos_sha" \
  '{schema:1,version:$version,repository:$repository,commit:$commit,targets:[{target:$linux_target,artifact:("https://github.com/"+$repository+"/releases/download/"+$tag+"/"+$linux_binary),sha256:$linux_sha},{target:$macos_target,artifact:("https://github.com/"+$repository+"/releases/download/"+$tag+"/"+$macos_binary),sha256:$macos_sha}]}')" \
  || fail 'could not encode expected release manifest'
[[ "$(cat "assets/$manifest_name")" == "$expected_manifest" ]] \
  || fail 'manifest differs from exact versioned targets, source, URLs, or bytes'

verify_attested_subject() {
  local subject="$1" bundle="$2"
  pinned_gh attestation verify "assets/$subject" \
    --repo "$repository" \
    --signer-workflow "$repository/.github/workflows/generator-release.yml" \
    --signer-digest "$workflow_sha" \
    --source-digest "$commit" \
    --source-ref "refs/heads/$expected_branch" \
    --bundle "assets/$bundle" \
    --deny-self-hosted-runners \
    --format json > "$work_dir/attestation.json" \
    || fail "provenance verification failed: $subject"
  jq -e 'type == "array" and length > 0' "$work_dir/attestation.json" >/dev/null \
    || fail "provenance verification output is malformed: $subject"
}

verify_attested_subject "$linux_binary" "$linux_bundle"
verify_attested_subject "$linux_sidecar" "$linux_bundle"
verify_attested_subject "$macos_binary" "$macos_bundle"
verify_attested_subject "$macos_sidecar" "$macos_bundle"
verify_attested_subject "$manifest_name" "$manifest_bundle"

source_check_output="$work_dir/source.out"
GITHUB_OUTPUT="$source_check_output" GH_TOKEN="$token" \
  bash scripts/generator-release/check-source.sh \
  || fail 'source and Required CI eligibility changed before publication'
validated_branch="$(sed -n 's/^default_branch=//p' "$source_check_output")"
validated_workflow_sha="$(sed -n 's/^workflow_sha=//p' "$source_check_output")"
[[ "$validated_branch" == "$expected_branch" && "$validated_workflow_sha" == "$commit" ]] \
  || fail 'default branch or workflow identity changed before publication'
require_absent "git/ref/tags/$tag" "$work_dir/tag-existing.json"
require_absent "releases/tags/$tag" "$work_dir/release-existing.json"

tag_response="$work_dir/tag-create.json"
if ! pinned_gh api --method POST "repos/$repository/git/refs" \
  -f "ref=refs/tags/$tag" -f "sha=$commit" > "$tag_response"; then
  fail 'tag creation result is uncertain; do not retry this version'
fi
jq -e --arg ref "refs/tags/$tag" --arg sha "$commit" \
  '.ref == $ref and .object.sha == $sha and .object.type == "commit"' \
  "$tag_response" >/dev/null \
  || fail 'tag creation response did not bind the exact source commit; do not retry'

if ! pinned_gh release create "$tag" \
  --repo "$repository" \
  --verify-tag \
  --target "$commit" \
  --title "$tag" \
  --latest \
  --notes "Velnor Actions ${version} built from ${commit}." \
  "assets/$linux_binary" "assets/$linux_sidecar" \
  "assets/$macos_binary" "assets/$macos_sidecar" \
  "assets/$manifest_name" "assets/$linux_bundle" \
  "assets/$macos_bundle" "assets/$manifest_bundle" \
  > "$work_dir/release-create.out"; then
  fail 'release creation or upload result is uncertain; do not retry this version'
fi

tag_verify="$work_dir/tag.json"
require_api_object "git/ref/tags/$tag" "$tag_verify"
jq -e --arg ref "refs/tags/$tag" --arg sha "$commit" \
  '.ref == $ref and .object.sha == $sha and .object.type == "commit"' \
  "$tag_verify" >/dev/null || fail 'published tag does not resolve to the exact source commit'

release_json="$work_dir/release.json"
require_api_object "releases/tags/$tag" "$release_json"
jq -e --arg tag "$tag" \
  '.tag_name == $tag and .draft == false and .prerelease == false and (.assets | type == "array" and length == 8)' \
  "$release_json" >/dev/null || fail 'published release metadata is incomplete or inconsistent'

release_names=(
  "$linux_binary" "$linux_sidecar" "$macos_binary" "$macos_sidecar"
  "$manifest_name" "$linux_bundle" "$macos_bundle" "$manifest_bundle"
)
for name in "${release_names[@]}"; do
  case "$name" in
    "$linux_binary") path="assets/$linux_binary" ;;
    "$linux_sidecar") path="assets/$linux_sidecar" ;;
    "$macos_binary") path="assets/$macos_binary" ;;
    "$macos_sidecar") path="assets/$macos_sidecar" ;;
    "$manifest_name") path="assets/$manifest_name" ;;
    "$linux_bundle") path="assets/$linux_bundle" ;;
    "$macos_bundle") path="assets/$macos_bundle" ;;
    "$manifest_bundle") path="assets/$manifest_bundle" ;;
    *) fail 'unexpected release asset name' ;;
  esac
  digest="$(file_digest "$path")"
  size="$(file_size "$path")"
  jq -e --arg name "$name" --arg digest "sha256:$digest" --argjson size "$size" \
    '[.assets[] | select(.name == $name)] as $matches
      | ($matches | length) == 1
        and $matches[0].size == $size
        and $matches[0].digest == $digest' \
    "$release_json" >/dev/null \
    || fail "published asset name, exact bytes, or digest mismatch: $name"
done
