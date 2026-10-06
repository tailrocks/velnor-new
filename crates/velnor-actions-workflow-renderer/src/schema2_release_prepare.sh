set -euo pipefail

readonly repository='@REPOSITORY@'
readonly source_sha="$VELNOR_SOURCE_SHA"
readonly authority_sha="$VELNOR_WORKFLOW_AUTHORITY_SHA"
readonly tag_prefix='@TAG_PREFIX@'
readonly fixed_tag='@FIXED_TAG@'
if [[ -n "$fixed_tag" ]]; then
  prepare_tag="$fixed_tag"
else
  prepare_tag="${tag_prefix}-${source_sha}"
fi
readonly prepare_tag
readonly prepare_expected_assets='@ASSET_NAMES_JSON@'

fail() {
  printf 'release reconciliation (@LABEL@): %s\n' "$1" >&2
  exit 1
}

[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'source SHA is malformed'
[[ "$authority_sha" == "$source_sha" ]] || fail 'workflow authority differs from source'
[[ -n "$GH_TOKEN" ]] || fail 'read-only GitHub token is missing'

@GH_FUNCTION@

assert_tag_target() {
  local reference tag_object object_type object_sha
  reference="$(gh api "repos/$repository/git/ref/tags/$prepare_tag")" \
    || fail 'release tag lookup failed'
  object_type="$(jq -er '.object.type' <<<"$reference")" \
    || fail 'release tag response is invalid'
  object_sha="$(jq -er '.object.sha' <<<"$reference")" \
    || fail 'release tag response is invalid'
  if [[ "$object_type" == tag ]]; then
    tag_object="$(gh api "repos/$repository/git/tags/$object_sha")" \
      || fail 'annotated release tag lookup failed'
    object_type="$(jq -er '.object.type' <<<"$tag_object")" \
      || fail 'annotated release tag response is invalid'
    object_sha="$(jq -er '.object.sha' <<<"$tag_object")" \
      || fail 'annotated release tag response is invalid'
  fi
  [[ "$object_type" == commit && "$object_sha" == "$source_sha" ]] || fail 'tag does not resolve to the exact source commit'
}

releases="$(gh api --paginate --slurp "repos/$repository/releases?per_page=100")" \
  || fail 'release list request failed'
matches="$(jq -c --arg tag "$prepare_tag" '[.[][]? | select(.tag_name == $tag)]' <<<"$releases")" \
  || fail 'release list response is invalid'
count="$(jq -r 'length' <<<"$matches")" || fail 'release list match count is invalid'
case "$count" in
  0)
    refs="$(gh api --paginate --slurp "repos/$repository/git/matching-refs/tags/$prepare_tag?per_page=100")" \
      || fail 'matching release tag lookup failed'
    exact_ref="$(jq -r --arg ref "refs/tags/$prepare_tag" '[.[][]? | select(.ref == $ref)] | length' <<<"$refs")" \
      || fail 'matching release tag response is invalid'
    [[ "$exact_ref" == 0 ]] || fail 'tag exists without a published release; draft or orphan requires reconciliation'
    printf 'action=build\n' >> "$GITHUB_OUTPUT"
    exit 0
    ;;
  1) ;;
  *) fail 'multiple releases use the exact source tag' ;;
esac

jq -e \
  --arg tag "$prepare_tag" \
  --arg sha "$source_sha" \
  --argjson expected "$prepare_expected_assets" \
  '.[0] as $release
   | $release.tag_name == $tag
     and $release.target_commitish == $sha
     and $release.draft == false
     and $release.prerelease == false
     and $release.immutable == true
     and ([$release.assets[].name] | sort) == ($expected | sort)
     and all($release.assets[];
       .state == "uploaded"
       and (.size | type == "number" and . > 0)
       and (.digest | type == "string" and test("^sha256:[0-9a-f]{64}$")))' \
  <<<"$matches" >/dev/null || fail 'existing release metadata, immutability, or asset set does not match'
assert_tag_target

temp_dir="$(mktemp -d)" || fail 'could not create release verification directory'
trap 'rm -rf "$temp_dir"' EXIT
gh release verify "$prepare_tag" --repo "$repository" >/dev/null \
  || fail 'release signature verification failed'
@DOWNLOAD_COMMANDS@
@CHECKSUM_COMMAND@
@ASSET_VERIFY_COMMANDS@
printf 'action=complete\n' >> "$GITHUB_OUTPUT"
