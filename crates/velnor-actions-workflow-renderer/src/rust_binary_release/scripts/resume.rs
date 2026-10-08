//! Shell block for validating and resuming an existing release draft.

pub(super) const DISCOVER_DRAFT: &str = r#"existing_releases="$(gh_api --paginate --slurp "repos/$GITHUB_REPOSITORY/releases?per_page=100")" || fail 'cannot inventory existing releases with publisher token'
matching_releases="$(jq -ce --arg tag "$tag" '
  if type != "array" or (all(.[]; type == "array") | not)
    or (all(.[][]; type == "object" and (.tag_name | type == "string")
      and (.id | type == "number" and . == floor and . > 0)
      and (.draft | type == "boolean")) | not)
  then error("release inventory shape is invalid")
  else [.[][] | select(.tag_name == $tag)]
  end
' <<<"$existing_releases")" || fail 'existing release inventory is malformed'
matching_release_count="$(jq -er 'length' <<<"$matching_releases")" || fail 'existing release match count is malformed'
case "$matching_release_count" in
  0) resume_release_id='' ;;
  1)
    matching_draft="$(jq -er '.[0].draft | select(type == "boolean")' <<<"$matching_releases")" || fail 'existing release draft state is malformed'
    [[ "$matching_draft" == true ]] || fail 'selected release tag already has a published release'
    resume_release_id="$(jq -er '.[0].id | select(type == "number" and . == floor and . > 0)' <<<"$matching_releases")" || fail 'existing draft release ID is malformed'
    ;;
  *) fail 'multiple releases use the selected tag' ;;
esac"#;

pub(super) const PUBLISH_RESUME: &str = r#"[[ "$resume_release_id" =~ ^[0-9]+$ ]] || fail 'resume release ID is malformed'
linux_digest="$(sha256sum "$linux_name" | cut -d ' ' -f 1)" || fail 'cannot hash Linux asset'
macos_digest="$(sha256sum "$macos_name" | cut -d ' ' -f 1)" || fail 'cannot hash macOS asset'
sums_digest="$(sha256sum SHA256SUMS | cut -d ' ' -f 1)" || fail 'cannot hash checksum asset'
linux_size="$(wc -c < "$linux_name" | tr -d ' ')" || fail 'cannot size Linux asset'
macos_size="$(wc -c < "$macos_name" | tr -d ' ')" || fail 'cannot size macOS asset'
sums_size="$(wc -c < SHA256SUMS | tr -d ' ')" || fail 'cannot size checksum asset'
expected_assets="$(jq -cn \
  --arg linux "$linux_name" --arg linux_digest "sha256:$linux_digest" --argjson linux_size "$linux_size" \
  --arg macos "$macos_name" --arg macos_digest "sha256:$macos_digest" --argjson macos_size "$macos_size" \
  --arg sums SHA256SUMS --arg sums_digest "sha256:$sums_digest" --argjson sums_size "$sums_size" \
  '[{name:$linux,digest:$linux_digest,size:$linux_size},{name:$macos,digest:$macos_digest,size:$macos_size},{name:$sums,digest:$sums_digest,size:$sums_size}]')" || fail 'cannot prepare expected release assets'

release_metadata_matches() {
  jq -e --argjson id "$resume_release_id" --arg tag "$tag" --argjson prerelease "$is_prerelease" \
    --arg body "Automated binary release for $tag." \
    '.id == $id and .tag_name == $tag and .name == $tag and .body == $body and .draft == true and .prerelease == $prerelease' \
    >/dev/null <<<"$1"
}
release_assets_match_expected() {
  jq -e --argjson expected "$expected_assets" \
    '(.assets | type == "array") and ([.assets[].name] | length == (unique | length)) and all(.assets[]; . as $asset | any($expected[]; .name == $asset.name and .digest == $asset.digest and .size == $asset.size))' \
    >/dev/null <<<"$1"
}
release_assets_are_complete() {
  jq -e --argjson expected "$expected_assets" \
    '. as $release | ([.assets[].name] | sort) == ($expected | map(.name) | sort) and all($expected[]; . as $asset | any($release.assets[]; .name == $asset.name and .digest == $asset.digest and .size == $asset.size))' \
    >/dev/null <<<"$1"
}

release_json="$(gh_api "repos/$GITHUB_REPOSITORY/releases/$resume_release_id")" || fail 'selected draft release is missing or unreadable'
release_metadata_matches "$release_json" || fail 'selected draft metadata does not match the release'
release_assets_match_expected "$release_json" || fail 'selected draft has unexpected or mismatched assets'
missing_assets="$(jq -r --argjson expected "$expected_assets" '. as $release | $expected[] as $asset | select([$release.assets[].name] | index($asset.name) == null) | $asset.name' <<<"$release_json")" || fail 'cannot inspect selected draft assets'
while IFS= read -r missing_asset; do
  [[ -n "$missing_asset" ]] || continue
  @@GH_PREFIX@@ release upload "$tag" "$missing_asset" --repo "$GITHUB_REPOSITORY" || fail "cannot upload missing draft asset: $missing_asset"
done <<<"$missing_assets"
release_json="$(gh_api "repos/$GITHUB_REPOSITORY/releases/$resume_release_id")" || fail 'cannot re-read selected draft'
release_metadata_matches "$release_json" || fail 'selected draft metadata changed during resume'
release_assets_are_complete "$release_json" || fail 'resumed draft assets do not match the expected checksums'
gh_api --method PATCH "repos/$GITHUB_REPOSITORY/releases/$resume_release_id" -F draft=false >/dev/null || fail 'cannot publish the verified draft'
published_json="$(gh_api "repos/$GITHUB_REPOSITORY/releases/$resume_release_id")" || fail 'cannot verify the published release'
jq -e --argjson id "$resume_release_id" --arg tag "$tag" --argjson prerelease "$is_prerelease" \
  --arg body "Automated binary release for $tag." \
  '.id == $id and .tag_name == $tag and .name == $tag and .body == $body and .draft == false and .prerelease == $prerelease' \
  >/dev/null <<<"$published_json" || fail 'published release metadata failed verification'
release_assets_are_complete "$published_json" || fail 'published release assets failed verification'"#;
