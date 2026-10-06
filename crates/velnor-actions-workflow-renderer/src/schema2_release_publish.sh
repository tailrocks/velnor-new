set -euo pipefail

@ELIGIBILITY_SCRIPT@

readonly release_repository='@REPOSITORY@'
readonly release_workflow_path='@WORKFLOW_PATH@'
readonly release_source_sha="$VELNOR_SOURCE_SHA"
readonly release_authority_sha="$VELNOR_WORKFLOW_AUTHORITY_SHA"
readonly release_ci_run_id="$VELNOR_CI_RUN_ID"
readonly release_ci_attempt="$VELNOR_CI_ATTEMPT"
readonly release_action="$VELNOR_RELEASE_ACTION"
readonly tag_prefix='@TAG_PREFIX@'
readonly fixed_tag='@FIXED_TAG@'
if [[ -n "$fixed_tag" ]]; then
  release_tag="$fixed_tag"
else
  release_tag="${tag_prefix}-${release_source_sha}"
fi
readonly release_tag
readonly expected_assets='@ASSET_NAMES_JSON@'
readonly release_asset_paths=(@ASSET_PATHS@)

family_fail() {
  printf 'release publication (@LABEL@): %s\n' "$1" >&2
  exit 1
}

[[ "$release_source_sha" == "$GITHUB_SHA" ]] || family_fail 'source output differs from event source'
[[ "$release_authority_sha" == "$release_source_sha" ]] || family_fail 'workflow authority differs from source'
[[ "$release_ci_run_id" =~ ^[0-9]+$ ]] || family_fail 'initial CI run ID is malformed'
[[ "$release_ci_attempt" =~ ^[0-9]+$ ]] || family_fail 'initial CI attempt is malformed'
[[ -n "$GH_TOKEN" ]] || family_fail 'GitHub token is missing'

@GH_FUNCTION@
family_gh() { gh "$@"; }

check_eligibility_identity() {
  local output actual_source actual_authority actual_run actual_attempt
  output="$(mktemp)"
  if ! (GITHUB_OUTPUT="$output"; release_eligibility); then
    rm -f "$output"
    return 1
  fi
  actual_source="$(sed -n 's/^source_sha=//p' "$output")"
  actual_authority="$(sed -n 's/^workflow_authority_sha=//p' "$output")"
  actual_run="$(sed -n 's/^ci_run_id=//p' "$output")"
  actual_attempt="$(sed -n 's/^ci_attempt=//p' "$output")"
  rm -f "$output"
  [[ "$actual_source" == "$release_source_sha" ]] || family_fail 'source changed after initial gate'
  [[ "$actual_authority" == "$release_authority_sha" ]] || family_fail 'workflow authority changed after initial gate'
  [[ "$actual_run" == "$release_ci_run_id" ]] || family_fail 'latest CI run changed after initial gate'
  [[ "$actual_attempt" == "$release_ci_attempt" ]] || family_fail 'latest CI attempt changed after initial gate'
}

assert_tag_target() {
  local reference tag_object object_type object_sha
  reference="$(family_gh api "repos/$release_repository/git/ref/tags/$release_tag")"
  object_type="$(jq -er '.object.type' <<<"$reference")"
  object_sha="$(jq -er '.object.sha' <<<"$reference")"
  if [[ "$object_type" == tag ]]; then
    tag_object="$(family_gh api "repos/$release_repository/git/tags/$object_sha")"
    object_type="$(jq -er '.object.type' <<<"$tag_object")"
    object_sha="$(jq -er '.object.sha' <<<"$tag_object")"
  fi
  [[ "$object_type" == commit && "$object_sha" == "$release_source_sha" ]] || family_fail 'release tag does not resolve to the exact source commit'
}

if [[ "$release_action" == complete ]]; then
  check_eligibility_identity
  existing_output="$(mktemp)"
  if ! GITHUB_OUTPUT="$existing_output" \
    VELNOR_SOURCE_SHA="$release_source_sha" \
    VELNOR_WORKFLOW_AUTHORITY_SHA="$release_authority_sha" \
    bash -s <<'VELNOR_PREPARE'
@PREPARE_SCRIPT@
VELNOR_PREPARE
  then
    rm -f "$existing_output"
    family_fail 'already-published release failed idempotent revalidation'
  fi
  grep -Fqx 'action=complete' "$existing_output" || family_fail 'release is no longer complete'
  rm -f "$existing_output"
  exit 0
fi
[[ "$release_action" == build ]] || family_fail 'unknown preparation action'

releases="$(family_gh api --paginate --slurp "repos/$release_repository/releases?per_page=100")"
existing="$(jq -r --arg tag "$release_tag" '[.[][]? | select(.tag_name == $tag)] | length' <<<"$releases")"
[[ "$existing" == 0 ]] || family_fail 'release appeared after preparation; no overwrite is allowed'
refs="$(family_gh api --paginate --slurp "repos/$release_repository/git/matching-refs/tags/$release_tag?per_page=100")"
exact_ref="$(jq -r --arg ref "refs/tags/$release_tag" '[.[][]? | select(.ref == $ref)] | length' <<<"$refs")"
[[ "$exact_ref" == 0 ]] || family_fail 'tag appeared after preparation; no overwrite is allowed'

@CHECKSUM_COMMAND@
for asset in "${release_asset_paths[@]}"; do
  family_gh attestation verify "$asset" \
    --repo "$release_repository" \
    --source-ref refs/heads/main \
    --source-digest "$release_source_sha" \
    --signer-workflow "$release_repository/$release_workflow_path" \
    --signer-digest "$release_authority_sha" >/dev/null
done

check_eligibility_identity
family_gh api \
  -X POST \
  -f "ref=refs/tags/$release_tag" \
  -f "sha=$release_source_sha" \
  "repos/$release_repository/git/refs" >/dev/null
assert_tag_target
family_gh release create "$release_tag" \
  --repo "$release_repository" \
  --target "$release_source_sha" \
  --title "$release_tag" \
  --notes "@LABEL@ built from $release_source_sha." \
  --latest=false \
  --verify-tag \
  --draft
assert_tag_target
family_gh release upload "$release_tag" @ASSET_PATHS@ --repo "$release_repository"
check_eligibility_identity
family_gh release edit "$release_tag" --draft=false --verify-tag --repo "$release_repository"

published="$(family_gh api "repos/$release_repository/releases/tags/$release_tag")"
jq -e \
  --arg tag "$release_tag" \
  --arg sha "$release_source_sha" \
  --argjson expected "$expected_assets" \
  '.tag_name == $tag
   and .target_commitish == $sha
   and .draft == false
   and .prerelease == false
   and .immutable == true
   and ([.assets[].name] | sort) == ($expected | sort)
   and all(.assets[];
     .state == "uploaded"
     and (.size | type == "number" and . > 0)
     and (.digest | type == "string" and test("^sha256:[0-9a-f]{64}$")))' \
  <<<"$published" >/dev/null || family_fail 'published release is not the exact immutable family release'
assert_tag_target
family_gh release verify "$release_tag" --repo "$release_repository" >/dev/null
@ASSET_VERIFY_COMMANDS@
