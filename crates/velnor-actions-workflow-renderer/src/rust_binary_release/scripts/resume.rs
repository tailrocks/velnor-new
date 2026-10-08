//! Shell block for refusing to alter an existing release.

pub(super) const REJECT_EXISTING: &str = r#"existing_releases="$(gh_api --paginate --slurp "repos/$GITHUB_REPOSITORY/releases?per_page=100")" || fail 'cannot inventory existing releases with publisher token'
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
  0) ;;
  1)
    matching_draft="$(jq -er '.[0].draft | select(type == "boolean")' <<<"$matching_releases")" || fail 'existing release draft state is malformed'
    if [[ "$matching_draft" == true ]]; then
      fail 'selected release tag already has a draft release; reconcile or remove it manually'
    fi
    fail 'selected release tag already has a published release'
    ;;
  *) fail 'multiple releases use the selected tag' ;;
esac"#;
