//! Archive validation and token-bearing publication scripts.

pub(super) fn prepare_assets(package: &str, binary: &str) -> String {
    let template = r#"set -euo pipefail

fail() { printf 'binary release asset validation: %s\n' "$1" >&2; exit 1; }
source_sha="${SOURCE_SHA-}"
tag="${RELEASE_TAG-}"
version="${RELEASE_VERSION-}"
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'source SHA is malformed'
[[ "$tag" == "@@PACKAGE@@-v$version" ]] || fail 'tag and version disagree'

[[ -d assets/incoming-linux && -d assets/incoming-macos ]] || fail 'one or more build artifacts are missing'
linux_name="@@BINARY@@-${RELEASE_VERSION}-x86_64-unknown-linux-gnu.tar.gz"
macos_name="@@BINARY@@-${RELEASE_VERSION}-aarch64-apple-darwin.tar.gz"
linux_source="assets/incoming-linux/$linux_name"
macos_source="assets/incoming-macos/$macos_name"
[[ -f "$linux_source" && ! -L "$linux_source" && -s "$linux_source" ]] || fail 'Linux artifact is missing, empty, or not a regular file'
[[ -f "$macos_source" && ! -L "$macos_source" && -s "$macos_source" ]] || fail 'macOS artifact is missing, empty, or not a regular file'
[[ "$(find assets/incoming-linux -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')" == 1 ]] || fail 'Linux artifact contains an unexpected file set'
[[ "$(find assets/incoming-macos -mindepth 1 -maxdepth 1 | wc -l | tr -d ' ')" == 1 ]] || fail 'macOS artifact contains an unexpected file set'
[[ -z "$(find assets/incoming-linux assets/incoming-macos ! -type d ! -type f -print)" ]] || fail 'artifact contains a non-regular file'
tar -tzf "$linux_source" | awk -v name='@@BINARY@@' 'NR == 1 && $0 == name { valid = 1 } END { if (NR != 1 || !valid) exit 1 }' || fail 'Linux archive has an unexpected file set'
tar -tzf "$macos_source" | awk -v name='@@BINARY@@' 'NR == 1 && $0 == name { valid = 1 } END { if (NR != 1 || !valid) exit 1 }' || fail 'macOS archive has an unexpected file set'
tar -tvzf "$linux_source" | awk -v name='@@BINARY@@' 'NR == 1 && substr($0, 1, 1) == "-" && substr($0, 2, 9) == "rwxr-xr-x" && $NF == name { valid = 1 } END { if (NR != 1 || !valid) exit 1 }' || fail 'Linux archive binary mode or type is invalid'
tar -tvzf "$macos_source" | awk -v name='@@BINARY@@' 'NR == 1 && substr($0, 1, 1) == "-" && substr($0, 2, 9) == "rwxr-xr-x" && $NF == name { valid = 1 } END { if (NR != 1 || !valid) exit 1 }' || fail 'macOS archive binary mode or type is invalid'
mkdir -p assets
cp -- "$linux_source" "assets/$linux_name"
cp -- "$macos_source" "assets/$macos_name"
cd assets
sha256sum "$linux_name" "$macos_name" > SHA256SUMS
sha256sum --check SHA256SUMS || fail 'checksum verification failed'
"#;
    template
        .replace("@@PACKAGE@@", package)
        .replace("@@BINARY@@", binary)
}

const PUBLISH_TEMPLATE: &str = r#"set -euo pipefail

fail() { printf 'binary release publish: %s\n' "$1" >&2; exit 1; }
gh_api() { GH_TOKEN="${GH_TOKEN-}" @@GH_PREFIX@@ api "$@"; }
source_sha="${SOURCE_SHA-}"
default_sha="${DEFAULT_SHA-}"
tag="${RELEASE_TAG-}"
version="${RELEASE_VERSION-}"
resume_release_id="${RESUME_RELEASE_ID-}"
repository="${GITHUB_REPOSITORY-}"
[[ "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail 'repository identity is malformed'
[[ "$source_sha" =~ ^[0-9a-f]{40}$ && "$default_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'source or default-branch SHA is malformed'
[[ "$tag" == "@@PACKAGE@@-v$version" ]] || fail 'release tag and version disagree'
linux_name="@@BINARY@@-${RELEASE_VERSION}-x86_64-unknown-linux-gnu.tar.gz"
macos_name="@@BINARY@@-${RELEASE_VERSION}-aarch64-apple-darwin.tar.gz"
[[ -f assets/SHA256SUMS && ! -L assets/SHA256SUMS ]] || fail 'prepared checksum file is missing or not regular'
cd assets
prepared_sums="$(cat SHA256SUMS)" || fail 'cannot read prepared checksums'
actual_sums="$(sha256sum "$linux_name" "$macos_name")" || fail 'cannot calculate asset checksums'
[[ "$prepared_sums" == "$actual_sums" ]] || fail 'prepared checksums do not match the release assets'

remote_ref="$(gh_api "repos/$repository/git/ref/tags/$tag")" || fail 'remote tag is missing or unreadable'
[[ "$(jq -er '.ref' <<<"$remote_ref")" == "refs/tags/$tag" ]] || fail 'GitHub returned a different tag ref'
object_type="$(jq -er '.object.type' <<<"$remote_ref")" || fail 'remote tag object type is malformed'
object_sha="$(jq -er '.object.sha' <<<"$remote_ref")" || fail 'remote tag object SHA is malformed'
[[ "$object_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'remote tag object SHA is malformed'
case "$object_type" in
  commit) remote_source_sha="$object_sha" ;;
  tag)
    tag_object="$(gh_api "repos/$repository/git/tags/$object_sha")" || fail 'annotated tag object is unreadable'
    [[ "$(jq -er '.object.type' <<<"$tag_object")" == commit ]] || fail 'nested or non-commit tags are unsupported'
    remote_source_sha="$(jq -er '.object.sha' <<<"$tag_object")" || fail 'annotated tag target is malformed'
    [[ "$remote_source_sha" =~ ^[0-9a-f]{40}$ ]] || fail 'annotated tag target SHA is malformed'
    ;;
  *) fail 'remote tag does not point to a commit or annotated tag' ;;
esac
[[ "$remote_source_sha" == "$source_sha" ]] || fail 'remote tag moved after verification'
comparison="$(gh_api "repos/$repository/compare/$source_sha...$default_sha")" || fail 'cannot recheck source reachability from captured default branch'
comparison_status="$(jq -er '.status' <<<"$comparison")" || fail 'default-branch comparison is malformed'
[[ "$comparison_status" == ahead || "$comparison_status" == identical ]] || fail 'release source is no longer reachable from captured default branch'

linux_digest="$(sha256sum "$linux_name" | cut -d ' ' -f 1)" || fail 'cannot hash Linux asset'
macos_digest="$(sha256sum "$macos_name" | cut -d ' ' -f 1)" || fail 'cannot hash macOS asset'
sums_digest="$(sha256sum SHA256SUMS | cut -d ' ' -f 1)" || fail 'cannot hash checksum asset'
linux_size="$(stat -c '%s' "$linux_name")" || fail 'cannot size Linux asset'
macos_size="$(stat -c '%s' "$macos_name")" || fail 'cannot size macOS asset'
sums_size="$(stat -c '%s' SHA256SUMS)" || fail 'cannot size checksum asset'
expected_assets="$(jq -cn \
  --arg linux "$linux_name" --arg linux_digest "sha256:$linux_digest" --argjson linux_size "$linux_size" \
  --arg macos "$macos_name" --arg macos_digest "sha256:$macos_digest" --argjson macos_size "$macos_size" \
  --arg sums SHA256SUMS --arg sums_digest "sha256:$sums_digest" --argjson sums_size "$sums_size" \
  '[{name:$linux,digest:$linux_digest,size:$linux_size},{name:$macos,digest:$macos_digest,size:$macos_size},{name:$sums,digest:$sums_digest,size:$sums_size}]')" || fail 'cannot prepare expected release assets'

release_metadata_matches() {
  jq -e --argjson id "$resume_release_id" --arg tag "$tag" \
    --arg title "$tag" --arg body "Automated binary release for $tag." \
    '.id == $id and .tag_name == $tag and .name == $title and .body == $body and .draft == true and .prerelease == false' \
    >/dev/null <<<"$1"
}
release_assets_are_expected_subset() {
  jq -e --argjson expected "$expected_assets" \
    '(.assets | type == "array") and ([.assets[].name] | length == (unique | length)) and all(.assets[]; . as $asset | any($expected[]; .name == $asset.name and .digest == $asset.digest and .size == $asset.size))' \
    >/dev/null <<<"$1"
}
release_assets_are_complete() {
  jq -e --argjson expected "$expected_assets" \
    '. as $release | ([.assets[].name] | sort) == ($expected | map(.name) | sort) and all($expected[]; . as $asset | any($release.assets[]; .name == $asset.name and .digest == $asset.digest and .size == $asset.size))' \
    >/dev/null <<<"$1"
}

if [[ -n "$resume_release_id" ]]; then
  [[ "$resume_release_id" =~ ^[0-9]+$ ]] || fail 'resume release ID is malformed'
  release_json="$(gh_api "repos/$repository/releases/$resume_release_id")" || fail 'selected draft release is missing or unreadable'
  release_metadata_matches "$release_json" || fail 'selected draft metadata differs from the verified release source'
  release_assets_are_expected_subset "$release_json" || fail 'selected draft has unexpected or mismatched assets'
  missing_assets="$(jq -r --argjson expected "$expected_assets" '. as $release | $expected[] as $asset | select([$release.assets[].name] | index($asset.name) == null) | $asset.name' <<<"$release_json")" || fail 'cannot inspect selected draft assets'
  while IFS= read -r missing_asset; do
    [[ -n "$missing_asset" ]] || continue
    @@GH_PREFIX@@ release upload "$tag" "$missing_asset" --repo "$repository" || fail "cannot upload missing draft asset: $missing_asset"
  done <<<"$missing_assets"
  release_json="$(gh_api "repos/$repository/releases/$resume_release_id")" || fail 'cannot re-read selected draft'
  release_metadata_matches "$release_json" || fail 'selected draft metadata changed during resume'
  release_assets_are_complete "$release_json" || fail 'resumed draft assets do not match the expected checksums'
  gh_api --method PATCH "repos/$repository/releases/$resume_release_id" -F draft=false >/dev/null || fail 'cannot publish the verified draft'
  published_json="$(gh_api "repos/$repository/releases/$resume_release_id")" || fail 'cannot verify the published release'
  jq -e --arg tag "$tag" '.tag_name == $tag and .name == $tag and .draft == false and .prerelease == false' >/dev/null <<<"$published_json" || fail 'published release metadata failed verification'
  release_assets_are_complete "$published_json" || fail 'published release assets failed verification'
else
  existing_tags="$(gh_api --paginate "repos/$repository/releases?per_page=100" --jq '.[].tag_name')" || fail 'cannot check for a release created after source selection'
  if grep -Fxq -- "$tag" <<<"$existing_tags"; then
    fail 'release appeared after source selection; rerun to reconcile its draft state'
  fi
  @@GH_PREFIX@@ release create "$tag" --repo "$repository" --verify-tag --target "$source_sha" --title "$tag" --notes "Automated binary release for $tag." --latest=false "$linux_name" "$macos_name" SHA256SUMS || fail 'release creation failed; a leftover draft will be reconciled on the next run'
fi"#;

pub(super) fn publish(package: &str, binary: &str, gh_prefix: &str) -> String {
    let template = PUBLISH_TEMPLATE;
    template
        .replace("@@BINARY@@", binary)
        .replace("@@PACKAGE@@", package)
        .replace("@@GH_PREFIX@@", gh_prefix)
}
