//! Draft and publish one verified release through a stable release ID.

use super::assets::{ASSETS, REPOSITORY, VERSION};
use super::{ATTESTATION_DIR, DIR, FILE, ProductReleasePins, publication_verify_script};
use crate::RenderError;

const ACCEPTED_DIRECTORY: &str = "release-accepted";
pub(super) const ACCEPTED_MANIFEST: &str = "release-accepted/release-manifest.json";
pub(super) const ACCEPTANCE_RECEIPT: &str = "release-accepted/release-acceptance.json";

/// Publish the exact qualified inventory, then emit receipt inputs after immutability.
pub(super) fn publish_script(pins: &ProductReleasePins) -> Result<String, RenderError> {
    let paths = release_asset_path_list();
    let upload = paths
        .iter()
        .map(|path| format!("  '{path}'"))
        .collect::<Vec<_>>()
        .join(" \\\n");
    let verify = publication_verify_script(pins);
    let preflight = tag_preflight_script();
    let ci_check = super::super::super::release_eligibility::publisher_script(pins)?;
    let names = asset_names_bash(&paths);
    let draft_check = verify_release_script("true", "draft-release.json", "false");
    let published_check = published_release_verify_script();
    Ok([
        "set -eu".to_owned(),
        format!("tag='v{VERSION}'"),
        "release_tmp=\"$(mktemp -d)\"\ntrap 'rm -rf \"$release_tmp\"' EXIT".to_owned(),
        release_tag_source_script(),
        release_api_helpers(),
        names,
        verify,
        preflight,
        ci_check.clone(),
        create_source_tag_script(),
        format!(
            "gh release create \"$tag\" --repo \"$GITHUB_REPOSITORY\" --verify-tag --target \"$GITHUB_SHA\" --title \"velnor-actions $tag\" --latest=false --draft --notes \"velnor-actions {VERSION} built from $GITHUB_SHA.\""
        ),
        release_id_script(),
        format!("gh release upload \"$tag\" --repo \"$GITHUB_REPOSITORY\" \\\n{upload}"),
        draft_check,
        ci_check,
        "assert_release_tag_source".to_owned(),
        "gh api \"repos/$GITHUB_REPOSITORY/releases/$release_id\" --method PATCH -F draft=false > /dev/null".to_owned(),
        published_check,
        acceptance_receipt_script(),
    ]
    .join("\n"))
}

/// Require confirmed 404 responses for both immutable tag and release lookups.
pub(super) fn tag_preflight_script() -> String {
    format!("bash scripts/generator-release/preflight-release-tag.sh '{VERSION}' '{REPOSITORY}'")
}

/// Resolve a release ID once; all later release reads use that immutable ID.
fn release_id_script() -> String {
    "release_view=\"$(gh release view \"$tag\" --repo \"$GITHUB_REPOSITORY\" --json databaseId,tagName,isDraft)\"\nrelease_id=\"$(jq -er --arg tag \"$tag\" '.databaseId as $id | select(.tagName == $tag and .isDraft == true and ($id | type == \"number\" and floor == . and . > 0)) | .databaseId' <<<\"$release_view\")\"\n[[ \"$release_id\" =~ ^[1-9][0-9]*$ ]]".to_owned()
}

fn create_source_tag_script() -> String {
    "created_ref=\"$(gh api \"repos/$GITHUB_REPOSITORY/git/refs\" --method POST -f \"ref=refs/tags/$tag\" -f \"sha=$GITHUB_SHA\")\"\njq -e --arg tag \"$tag\" --arg sha \"$GITHUB_SHA\" '.ref == (\"refs/tags/\" + $tag) and .object.type == \"commit\" and .object.sha == $sha' <<<\"$created_ref\" > /dev/null\nassert_release_tag_source".to_owned()
}

fn release_tag_source_script() -> String {
    r#"assert_release_tag_source() {
  local reference object kind digest next
  local -a seen=()
  reference="$(gh api "repos/$GITHUB_REPOSITORY/git/ref/tags/$tag")"
  object="$(jq -ce '.object | select(type == "object")' <<<"$reference")"
  for _ in 1 2 3 4 5 6 7 8; do
    kind="$(jq -er '.type' <<<"$object")"
    digest="$(jq -er '.sha | select(type == "string" and test("^[0-9a-f]{40}$"))' <<<"$object")"
    case "$kind" in
      commit)
        [[ "$digest" == "$GITHUB_SHA" ]] || return 1
        return 0
        ;;
      tag)
        for previous in "${seen[@]}"; do [[ "$previous" != "$digest" ]] || return 1; done
        seen+=("$digest")
        next="$(gh api "repos/$GITHUB_REPOSITORY/git/tags/$digest")"
        object="$(jq -ce '.object | select(type == "object")' <<<"$next")"
        ;;
      *) return 1 ;;
    esac
  done
  return 1
}"#
    .to_owned()
}

fn verify_release_script(
    expected_draft: &str,
    response_name: &str,
    require_canonical_asset_urls: &str,
) -> String {
    format!(
        "release_response=\"$release_tmp/{response_name}\"\ngh api \"repos/$GITHUB_REPOSITORY/releases/$release_id\" > \"$release_response\"\nverify_release_metadata \"$release_response\" '{expected_draft}'\nverify_release_assets \"$release_response\" '{require_canonical_asset_urls}'"
    )
}

/// Verify immutable release metadata, source tag, URLs, sizes, and all asset digests.
pub(super) fn published_release_verify_script() -> String {
    [
        "published_release_response=\"$release_tmp/published-release.json\"".to_owned(),
        verify_release_script("false", "published-release.json", "true"),
        "assert_release_tag_source".to_owned(),
    ]
    .join("\n")
}

fn release_api_helpers() -> String {
    r#"verify_release_metadata() {
  local response="$1" expected_draft="$2"
  jq -e --argjson release_id "$release_id" --arg tag "$tag" --arg source "$GITHUB_SHA" --arg api_url "https://api.github.com/repos/$GITHUB_REPOSITORY/releases/$release_id" --arg html_url "https://github.com/$GITHUB_REPOSITORY/releases/tag/$tag" --argjson expected_draft "$expected_draft" '.id == $release_id and .tag_name == $tag and .target_commitish == $source and .url == $api_url and (if $expected_draft then true else .html_url == $html_url end) and .draft == $expected_draft and .prerelease == false and (if $expected_draft then true else .immutable == true end)' "$response" > /dev/null
}

verify_release_assets() {
  local response="$1" require_canonical_url="$2" path name digest size url
  jq -e --argjson expected "$expected_release_asset_names" '(.assets | type) == "array" and all(.assets[]; (.name | type) == "string") and ([.assets[].name] | sort) == ($expected | sort) and ([.assets[].name] | length) == ([.assets[].name] | unique | length)' "$response" > /dev/null
  for path in "${release_asset_paths[@]}"; do
    test -f "$path" && test ! -L "$path" && test -s "$path"
    name="${path##*/}"
    digest="$(sha256sum "$path" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')"
    size="$(wc -c < "$path")"
    size="${size//[[:space:]]/}"
    url="https://github.com/$GITHUB_REPOSITORY/releases/download/$tag/$name"
    if [[ "$require_canonical_url" == true ]]; then
      jq -e --arg name "$name" --arg url "$url" --arg digest "sha256:$digest" --argjson size "$size" '[.assets[] | select(.name == $name)] as $matches | ($matches | length) == 1 and $matches[0].state == "uploaded" and $matches[0].browser_download_url == $url and $matches[0].digest == $digest and $matches[0].size == $size' "$response" > /dev/null
    else
      jq -e --arg name "$name" --arg digest "sha256:$digest" --argjson size "$size" '[.assets[] | select(.name == $name)] as $matches | ($matches | length) == 1 and $matches[0].state == "uploaded" and $matches[0].digest == $digest and $matches[0].size == $size' "$response" > /dev/null
    fi
  done
}"#
        .to_owned()
}

fn acceptance_receipt_script() -> String {
    format!(
        "[[ \"$GITHUB_RUN_ID\" =~ ^[0-9]+$ && \"$GITHUB_RUN_ATTEMPT\" =~ ^[0-9]+$ ]]\nif [[ -e '{ACCEPTED_DIRECTORY}' || -L '{ACCEPTED_DIRECTORY}' ]]; then echo 'release acceptance directory already exists' >&2; exit 1; fi\nmkdir -m 700 '{ACCEPTED_DIRECTORY}'\ncp -- '{DIR}/{FILE}' '{ACCEPTED_MANIFEST}'\ncmp -- '{DIR}/{FILE}' '{ACCEPTED_MANIFEST}'\nmanifest_sha256=\"$(sha256sum '{ACCEPTED_MANIFEST}' | awk 'NR == 1 {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')\"\nmanifest_size=\"$(wc -c < '{ACCEPTED_MANIFEST}')\"\nmanifest_size=\"${{manifest_size//[[:space:]]/}}\"\njq -n --arg repository \"$GITHUB_REPOSITORY\" --arg version '{VERSION}' --arg source_commit \"$GITHUB_SHA\" --arg workflow_authority_sha \"$GITHUB_WORKFLOW_SHA\" --arg run_id \"$GITHUB_RUN_ID\" --arg run_attempt \"$GITHUB_RUN_ATTEMPT\" --arg tag \"$tag\" --argjson release_id \"$release_id\" --arg manifest_name '{FILE}' --arg manifest_sha256 \"$manifest_sha256\" --argjson manifest_size \"$manifest_size\" --slurpfile release \"$published_release_response\" '{{schema:1,repository:$repository,version:$version,source_commit:$source_commit,workflow_authority_sha:$workflow_authority_sha,run_id:$run_id,run_attempt:$run_attempt,tag:$tag,release_id:$release_id,immutable:true,manifest:{{name:$manifest_name,sha256:$manifest_sha256,size:$manifest_size}},assets:($release[0].assets|map({{name:.name,url:.browser_download_url,digest:.digest,size:.size}})|sort_by(.name))}}' > '{ACCEPTANCE_RECEIPT}'\ntest -s '{ACCEPTANCE_RECEIPT}'\njq -e --arg repository \"$GITHUB_REPOSITORY\" --arg source \"$GITHUB_SHA\" --argjson release_id \"$release_id\" --argjson expected \"$expected_release_asset_names\" --arg manifest_sha256 \"$manifest_sha256\" '.schema == 1 and .repository == $repository and .source_commit == $source and .release_id == $release_id and .immutable == true and .manifest.sha256 == $manifest_sha256 and (.assets | length) == ($expected | length)' '{ACCEPTANCE_RECEIPT}' > /dev/null"
    )
}

/// Release assets published in stable target order, followed by the manifest and bundles.
#[cfg(test)]
pub(super) fn release_asset_paths() -> String {
    release_asset_path_list().join(" ")
}

pub(super) fn acceptance_artifact_name() -> String {
    "generator-release-accepted-${{ github.sha }}".to_owned()
}

pub(super) fn acceptance_artifact_paths() -> [&'static str; 2] {
    [ACCEPTED_MANIFEST, ACCEPTANCE_RECEIPT]
}

pub(super) fn release_asset_path_list() -> Vec<String> {
    let mut paths = ASSETS
        .iter()
        .flat_map(|asset| {
            [
                format!("{}/{}", asset.directory, asset.binary),
                format!("{}/{}", asset.directory, asset.sidecar),
                format!("{}/{}", asset.directory, asset.provenance),
            ]
        })
        .collect::<Vec<_>>();
    paths.push(format!("{DIR}/{FILE}"));
    for asset in ASSETS {
        for name in [asset.binary, asset.sidecar, asset.provenance] {
            paths.push(format!("{ATTESTATION_DIR}/{name}.intoto.jsonl"));
        }
    }
    paths.push(format!("{ATTESTATION_DIR}/{FILE}.intoto.jsonl"));
    paths
}

fn asset_names_bash(paths: &[String]) -> String {
    let names = paths
        .iter()
        .map(|path| {
            path.rsplit('/')
                .next()
                .map(|name| format!("'{name}'"))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    let expected = names.join(" ");
    format!(
        "release_asset_paths=(\n{}\n)\nrelease_asset_names=( {expected} )\nexpected_release_asset_names=\"$(printf '%s\\n' \"${{release_asset_names[@]}}\" | jq -R . | jq -s .)\"",
        paths
            .iter()
            .map(|path| format!("  '{path}'"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
