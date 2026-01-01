//! Runtime identity and immutable publication scripts for consumer binaries.

/// Check one workspace package and binary against locked Cargo metadata.
pub(super) fn identity(metadata_command: &str) -> String {
    IDENTITY.replace("@METADATA_COMMAND@", metadata_command)
}

/// Build, verify, checksum, and record one exact-source binary.
pub(super) fn build(metadata_script: &str, target_add: &str, build: &str) -> String {
    BUILD
        .replace("@IDENTITY@", metadata_script)
        .replace("@TARGET_ADD@", target_add)
        .replace("@BUILD@", build)
}

/// Verify current eligibility and artifact provenance, then create a version tag once.
pub(super) fn publish(
    eligibility: &str,
    metadata_script: &str,
    target: &str,
    asset: &str,
) -> String {
    PUBLISH
        .replace("@ELIGIBILITY@", eligibility)
        .replace("@IDENTITY@", metadata_script)
        .replace("@TARGET@", target)
        .replace("@ASSET@", asset)
}

const IDENTITY: &str = r#"set -euo pipefail
manifest_path="$GITHUB_WORKSPACE/$MANIFEST_PATH"
test -f "$manifest_path" || { echo 'manifest missing' >&2; exit 1; }
test ! -L "$manifest_path" || { echo 'manifest symlink refused' >&2; exit 1; }
workspace_root="$(cd "$(dirname "$manifest_path")" && pwd -P)"
expected_manifest="$workspace_root/Cargo.toml"
test "$manifest_path" = "$expected_manifest" || { echo 'manifest must name workspace Cargo.toml' >&2; exit 1; }
test -f "$workspace_root/Cargo.lock" && test ! -L "$workspace_root/Cargo.lock" || { echo 'Cargo.lock missing or symlinked' >&2; exit 1; }
git -C "$workspace_root" ls-files --error-unmatch Cargo.lock >/dev/null || { echo 'Cargo.lock is not tracked' >&2; exit 1; }
metadata="$(@METADATA_COMMAND@)"
record="$(jq -cer \
  --arg workspace "$workspace_root" \
  --arg manifest "$expected_manifest" \
  --arg package "$PACKAGE_NAME" \
  --arg bin "$BINARY_NAME" \
  '. as $doc
   | [.packages[] | select(.name == $package)] as $matches
   | if ($matches | length) != 1 then error("package must resolve exactly once") else $matches[0] end
   | . as $package_record
   | if ($package_record.manifest_path | startswith($workspace + "/") | not) then error("package manifest escapes workspace") else . end
   | if $doc.workspace_root != $workspace then error("workspace root mismatch") else . end
   | if ($doc.workspace_members | index($package_record.id)) == null then error("package is not a workspace member") else . end
   | [.targets[] | select(.name == $bin and (.kind | index("bin")) != null)] as $bins
   | if ($bins | length) != 1 then error("binary target must resolve exactly once") else $bins[0] end
   | if ((.["required-features"] // []) | length) != 0 then error("binary required-features are unsupported") else . end
   | { package_version: $package_record.version, bin: .name }' <<<"$metadata")" || { echo 'Cargo metadata identity check failed' >&2; exit 1; }
version="$(jq -er '.package_version' <<<"$record")"
actual_bin="$(jq -er '.bin' <<<"$record")"
tag="$PACKAGE_NAME-v$version"
test "$actual_bin" = "$BINARY_NAME" || { echo 'binary target identity mismatch' >&2; exit 1; }
git check-ref-format "refs/tags/$tag" || { echo 'Cargo version cannot form a release tag' >&2; exit 1; }
printf 'package_version=%s\ntag=%s\n' "$version" "$tag" >> "$GITHUB_OUTPUT"
"#;

const BUILD: &str = r#"set -euo pipefail
GITHUB_OUTPUT="$RUNNER_TEMP/binary-identity"
export GITHUB_OUTPUT
@IDENTITY@
version="$(sed -n 's/^package_version=//p' "$GITHUB_OUTPUT")"
tag="$(sed -n 's/^tag=//p' "$GITHUB_OUTPUT")"
test "$version" = "$EXPECTED_VERSION" || { echo 'Cargo version changed after eligibility' >&2; exit 1; }
test "$tag" = "$EXPECTED_TAG" || { echo 'release tag changed after eligibility' >&2; exit 1; }
target_dir="$RUNNER_TEMP/velnor-binary-target-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT"
test ! -e "$target_dir" && test ! -L "$target_dir" || { echo 'target directory already exists' >&2; exit 1; }
mkdir -m 700 "$target_dir"
export CARGO_TARGET_DIR="$target_dir"
@TARGET_ADD@
@BUILD@
binary="$CARGO_TARGET_DIR/$TARGET_TRIPLE/release/$BINARY_NAME"
test -s "$binary" || { echo 'built binary missing' >&2; exit 1; }
file -b "$binary" | grep -E 'Mach-O.*arm64' >/dev/null || { echo 'binary is not Apple Silicon Mach-O' >&2; exit 1; }
asset_dir="$RUNNER_TEMP/consumer-binary-assets"
test ! -e "$asset_dir" && test ! -L "$asset_dir" || { echo 'asset directory already exists' >&2; exit 1; }
mkdir -m 700 "$asset_dir"
cp "$binary" "$asset_dir/$BINARY_NAME-$TARGET_TRIPLE"
cd "$asset_dir"
shasum -a 256 "$BINARY_NAME-$TARGET_TRIPLE" > SHA256SUMS
jq -cn \
  --arg repository "$GITHUB_REPOSITORY" \
  --arg source_sha "$GITHUB_SHA" \
  --arg package "$PACKAGE_NAME" \
  --arg version "$version" \
  --arg bin "$BINARY_NAME" \
  --arg target "$TARGET_TRIPLE" \
  --arg tag "$tag" \
  '{ repository: $repository, source_sha: $source_sha, package: $package, version: $version, bin: $bin, target: $target, tag: $tag }' > release.json
test -s SHA256SUMS && test -s release.json
"#;

const PUBLISH: &str = r#"set -euo pipefail
GITHUB_OUTPUT="$RUNNER_TEMP/consumer-release-eligibility-initial"
export GITHUB_OUTPUT
@ELIGIBILITY@
initial_eligibility_output="$GITHUB_OUTPUT"
actual_output() { sed -n "s/^$1=//p" "$initial_eligibility_output"; }
test "$(actual_output source_sha)" = "$EXPECTED_SOURCE_SHA" || { echo 'source SHA changed' >&2; exit 1; }
test "$(actual_output workflow_authority_sha)" = "$EXPECTED_AUTHORITY_SHA" || { echo 'workflow authority changed' >&2; exit 1; }
test "$(actual_output ci_run_id)" = "$EXPECTED_CI_RUN_ID" || { echo 'latest Required CI run changed' >&2; exit 1; }
test "$(actual_output ci_attempt)" = "$EXPECTED_CI_ATTEMPT" || { echo 'latest Required CI attempt changed' >&2; exit 1; }
recheck_eligibility() {
  local output="$RUNNER_TEMP/consumer-release-eligibility-recheck"
  : > "$output"
  GITHUB_OUTPUT="$output"
  export GITHUB_OUTPUT
  VELNOR_RELEASE_CI_POLL_LIMIT=1 VELNOR_RELEASE_CI_POLL_SECONDS=0 release_eligibility
  test "$(sed -n 's/^source_sha=//p' "$output")" = "$EXPECTED_SOURCE_SHA" || { echo 'source changed before publication' >&2; exit 1; }
  test "$(sed -n 's/^workflow_authority_sha=//p' "$output")" = "$EXPECTED_AUTHORITY_SHA" || { echo 'workflow authority changed before publication' >&2; exit 1; }
  test "$(sed -n 's/^ci_run_id=//p' "$output")" = "$EXPECTED_CI_RUN_ID" || { echo 'latest Required CI run changed before publication' >&2; exit 1; }
  test "$(sed -n 's/^ci_attempt=//p' "$output")" = "$EXPECTED_CI_ATTEMPT" || { echo 'latest Required CI attempt changed before publication' >&2; exit 1; }
}
GITHUB_OUTPUT="$RUNNER_TEMP/consumer-release-identity"
export GITHUB_OUTPUT
@IDENTITY@
version="$(sed -n 's/^package_version=//p' "$GITHUB_OUTPUT")"
tag="$(sed -n 's/^tag=//p' "$GITHUB_OUTPUT")"
test "$version" = "$EXPECTED_VERSION" || { echo 'Cargo version changed before publish' >&2; exit 1; }
test "$tag" = "$EXPECTED_TAG" || { echo 'release tag changed before publish' >&2; exit 1; }
cd assets
test "$(find . -mindepth 1 -maxdepth 1 -type f | wc -l | tr -d ' ')" = 3 || { echo 'unexpected artifact file set' >&2; exit 1; }
test -z "$(find . -mindepth 1 -type l -print -quit)" || { echo 'artifact symlink refused' >&2; exit 1; }
shasum -a 256 -c SHA256SUMS
jq -e \
  --arg repository "$GITHUB_REPOSITORY" \
  --arg source_sha "$EXPECTED_SOURCE_SHA" \
  --arg package "$PACKAGE_NAME" \
  --arg version "$version" \
  --arg bin "$BINARY_NAME" \
  --arg target "@TARGET@" \
  --arg tag "$tag" \
  '. == { repository: $repository, source_sha: $source_sha, package: $package, version: $version, bin: $bin, target: $target, tag: $tag }' release.json >/dev/null
digest="$(shasum -a 256 "@ASSET@" | awk '{print $1}')"
test "${#digest}" -eq 64
case "$digest" in *[!0123456789abcdef]*|'') exit 1 ;; esac
bundle="sha256:${digest}.jsonl"
test ! -e "$bundle"
gh attestation download "@ASSET@" --repo "$GITHUB_REPOSITORY" --predicate-type https://slsa.dev/provenance/v1 --limit 10
test -s "$bundle"
gh attestation verify "@ASSET@" --repo "$GITHUB_REPOSITORY" --bundle "$bundle" \
  --source-digest "$EXPECTED_SOURCE_SHA" --source-ref "refs/heads/$DEFAULT_BRANCH" \
  --signer-workflow "$GITHUB_REPOSITORY/.github/workflows/binary-release.yml" \
  --signer-digest "$EXPECTED_AUTHORITY_SHA" >/dev/null
assert_immutable_releases_enabled() {
  test -n "${IMMUTABILITY_READ_TOKEN-}" || { echo 'immutable releases admin-read token is missing' >&2; exit 1; }
  local settings
  settings="$(GH_TOKEN="$IMMUTABILITY_READ_TOKEN" gh api \
    --method GET \
    -H 'Accept: application/vnd.github+json' \
    -H 'X-GitHub-Api-Version: 2026-03-10' \
    "repos/$GITHUB_REPOSITORY/immutable-releases")" \
    || { echo 'could not prove immutable releases are enabled' >&2; exit 1; }
  jq -e '.enabled == true' <<<"$settings" >/dev/null \
    || { echo 'immutable releases are disabled or response is invalid' >&2; exit 1; }
}
assert_protected_environment() {
  local environment
  environment="$(gh api \
    --method GET \
    -H 'Accept: application/vnd.github+json' \
    -H 'X-GitHub-Api-Version: 2026-03-10' \
    "repos/$GITHUB_REPOSITORY/environments/consumer-binary-release")" \
    || { echo 'could not verify the protected consumer-binary-release environment' >&2; exit 1; }
  jq -e '
    .name == "consumer-binary-release"
    and ([.protection_rules[]? | select(.type == "required_reviewers")] | length > 0)
    and ([.protection_rules[]? | select(.type == "required_reviewers")
          | ((.reviewers | length) > 0 and .prevent_self_review == true)] | all)
    and .deployment_branch_policy.protected_branches == true
    and .deployment_branch_policy.custom_branch_policies == false
  ' <<<"$environment" >/dev/null \
    || { echo 'consumer-binary-release environment lacks required reviewer protection' >&2; exit 1; }
}
require_absent() {
  local endpoint="$1" response
  if response="$(gh api "$endpoint" 2>&1)"; then
    echo "immutable release identity already exists: $endpoint" >&2
    exit 1
  fi
  case "$response" in *'(HTTP 404)'*) ;; *) echo "could not prove identity absent: $endpoint" >&2; exit 1 ;; esac
}
tag_endpoint="repos/$GITHUB_REPOSITORY/git/ref/tags/$tag"
release_endpoint="repos/$GITHUB_REPOSITORY/releases/tags/$tag"
require_absent "$tag_endpoint"
require_absent "$release_endpoint"
recheck_eligibility
assert_immutable_releases_enabled
assert_protected_environment
ref="$(gh api --method POST "repos/$GITHUB_REPOSITORY/git/refs" -f "ref=refs/tags/$tag" -f "sha=$EXPECTED_SOURCE_SHA")"
jq -e --arg ref "refs/tags/$tag" --arg sha "$EXPECTED_SOURCE_SHA" '.ref == $ref and .object.sha == $sha' <<<"$ref" >/dev/null
gh release create "$tag" "@ASSET@" SHA256SUMS release.json --repo "$GITHUB_REPOSITORY" \
  --verify-tag --draft --latest=false --title "$tag" \
  --notes "Rust package $PACKAGE_NAME $version built from $EXPECTED_SOURCE_SHA."
recheck_eligibility
assert_immutable_releases_enabled
assert_protected_environment
created_ref="$(gh api "$tag_endpoint")"
jq -e --arg ref "refs/tags/$tag" --arg sha "$EXPECTED_SOURCE_SHA" \
  '.ref == $ref and .object.sha == $sha' <<<"$created_ref" >/dev/null \
  || { echo 'release tag no longer points at the eligible source' >&2; exit 1; }
gh release edit "$tag" --repo "$GITHUB_REPOSITORY" --draft=false
release="$(gh release view "$tag" --repo "$GITHUB_REPOSITORY" --json tagName,isDraft,isImmutable,assets)"
jq -e \
  --arg tag "$tag" \
  --arg binary "@ASSET@" \
  '(.tagName == $tag) and (.isDraft == false) and (.isImmutable == true) and ([.assets[].name] | sort == ([$binary, "SHA256SUMS", "release.json"] | sort))' <<<"$release" >/dev/null || { echo 'published release is not exact and immutable' >&2; exit 1; }
download_dir="$RUNNER_TEMP/consumer-binary-release-download"
test ! -e "$download_dir" && test ! -L "$download_dir" || { echo 'download directory already exists' >&2; exit 1; }
mkdir -m 700 "$download_dir"
gh release download "$tag" --repo "$GITHUB_REPOSITORY" --dir "$download_dir"
test "$(find "$download_dir" -mindepth 1 -maxdepth 1 -type f | wc -l | tr -d ' ')" = 3 || { echo 'published asset set is unexpected' >&2; exit 1; }
test -z "$(find "$download_dir" -mindepth 1 -type l -print -quit)" || { echo 'published asset symlink refused' >&2; exit 1; }
(cd "$download_dir" && shasum -a 256 -c SHA256SUMS)
cmp SHA256SUMS "$download_dir/SHA256SUMS"
cmp release.json "$download_dir/release.json"
cmp "@ASSET@" "$download_dir/@ASSET@"
gh attestation verify "$download_dir/@ASSET@" --repo "$GITHUB_REPOSITORY" \
  --source-digest "$EXPECTED_SOURCE_SHA" --source-ref "refs/heads/$DEFAULT_BRANCH" \
  --signer-workflow "$GITHUB_REPOSITORY/.github/workflows/binary-release.yml" \
  --signer-digest "$EXPECTED_AUTHORITY_SHA" >/dev/null
"#;
