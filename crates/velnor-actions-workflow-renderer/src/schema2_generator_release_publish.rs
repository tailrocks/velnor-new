//! Asset validation, canonical manifest creation, and release publication.

use super::super::AssetNames;

const WORKSPACE_VERSION: &str = r#"
workspace_version() {
  awk -F '"' '
    /^\[workspace\.package\]$/ { in_workspace = 1; next }
    /^\[/ { in_workspace = 0 }
    in_workspace && $1 == "version = " && $2 ~ /^[0-9]+\.[0-9]+\.[0-9]+$/ {
      value = $2
      count += 1
    }
    END {
      if (count != 1) exit 1
      print value
    }
  ' Cargo.toml
}
RELEASE_VERSION="$(workspace_version)"
test "$RELEASE_VERSION" = "@VERSION@"
"#;

const ASSET_HELPERS: &str = r#"
assert_files() {
  dir="$1"
  asset="$2"
  sidecar="$3"
  test -z "$(find "$dir" -mindepth 1 -maxdepth 1 ! -type f -print -quit)"
  actual="$(find "$dir" -mindepth 1 -maxdepth 1 -type f -printf '%f\n' | LC_ALL=C sort)"
  expected="$(printf '%s\n%s\n' "$asset" "$sidecar" | LC_ALL=C sort)"
  test "$actual" = "$expected"
}
verify_sidecar() {
  asset="$1"
  sidecar="$2"
  name="$(basename "$asset")"
  expected="$(awk -v name="$name" '
    NR != 1 || NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ || $2 != name { exit 1 }
    { print $1 }
    END { if (NR != 1) exit 1 }
  ' "$sidecar")"
  actual="$(sha256sum "$asset" | cut -d ' ' -f1)"
  test "$expected" = "$actual"
  printf '%s\n' "$actual"
}
verify_attestation() {
  gh attestation verify "$1" --repo "$GITHUB_REPOSITORY" --signer-workflow "$GITHUB_REPOSITORY/@WORKFLOW@" --source-digest "$GITHUB_SHA" --source-ref refs/heads/main --deny-self-hosted-runners
}
validate_assets() {
  assert_files @LINUX_DIR@ @LINUX_BIN@ @LINUX_SUM@
  assert_files @MACOS_ARM_DIR@ @MACOS_ARM_BIN@ @MACOS_ARM_SUM@
  assert_files @MACOS_X64_DIR@ @MACOS_X64_BIN@ @MACOS_X64_SUM@
  linux_sha="$(verify_sidecar @LINUX_DIR@/@LINUX_BIN@ @LINUX_DIR@/@LINUX_SUM@)"
  macos_arm_sha="$(verify_sidecar @MACOS_ARM_DIR@/@MACOS_ARM_BIN@ @MACOS_ARM_DIR@/@MACOS_ARM_SUM@)"
  macos_x64_sha="$(verify_sidecar @MACOS_X64_DIR@/@MACOS_X64_BIN@ @MACOS_X64_DIR@/@MACOS_X64_SUM@)"
  for artifact in @LINUX_DIR@/@LINUX_BIN@ @LINUX_DIR@/@LINUX_SUM@ @MACOS_ARM_DIR@/@MACOS_ARM_BIN@ @MACOS_ARM_DIR@/@MACOS_ARM_SUM@ @MACOS_X64_DIR@/@MACOS_X64_BIN@ @MACOS_X64_DIR@/@MACOS_X64_SUM@; do
    verify_attestation "$artifact"
  done
}
verify_canonical_manifest() {
  manifest="$1"
  tag="$2"
  linux_sha="$3"
  macos_arm_sha="$4"
  macos_x64_sha="$5"
  jq -e --arg commit "$GITHUB_SHA" --arg tag "$tag" --arg linux_sha "$linux_sha" --arg macos_arm_sha "$macos_arm_sha" --arg macos_x64_sha "$macos_x64_sha" '. == {schema:1,version:"@VERSION@",repository:"@REPOSITORY@",commit:$commit,targets:[{target:"@LINUX_TARGET@",artifact:("https://github.com/@REPOSITORY@/releases/download/"+$tag+"/@LINUX_BIN@"),sha256:$linux_sha},{target:"@MACOS_ARM_TARGET@",artifact:("https://github.com/@REPOSITORY@/releases/download/"+$tag+"/@MACOS_ARM_BIN@"),sha256:$macos_arm_sha},{target:"@MACOS_X64_TARGET@",artifact:("https://github.com/@REPOSITORY@/releases/download/"+$tag+"/@MACOS_X64_BIN@"),sha256:$macos_x64_sha}]}' "$manifest" >/dev/null
}
"#;

const PREPARE_BODY: &str = r#"
verify_same_sha_ci
verify_release_environment
validate_assets
tag="generator-$GITHUB_SHA"
rm -rf release-manifest
mkdir -p release-manifest
manifest="@MANIFEST_PATH@"
jq -n --arg commit "$GITHUB_SHA" --arg tag "$tag" --arg linux_sha "$linux_sha" --arg macos_arm_sha "$macos_arm_sha" --arg macos_x64_sha "$macos_x64_sha" '{schema:1,version:"@VERSION@",repository:"@REPOSITORY@",commit:$commit,targets:[{target:"@LINUX_TARGET@",artifact:("https://github.com/@REPOSITORY@/releases/download/"+$tag+"/@LINUX_BIN@"),sha256:$linux_sha},{target:"@MACOS_ARM_TARGET@",artifact:("https://github.com/@REPOSITORY@/releases/download/"+$tag+"/@MACOS_ARM_BIN@"),sha256:$macos_arm_sha},{target:"@MACOS_X64_TARGET@",artifact:("https://github.com/@REPOSITORY@/releases/download/"+$tag+"/@MACOS_X64_BIN@"),sha256:$macos_x64_sha}]}' > "$manifest"
verify_canonical_manifest "$manifest" "$tag" "$linux_sha" "$macos_arm_sha" "$macos_x64_sha"
"#;

const PUBLISH_BODY: &str = r#"
verify_same_sha_ci
verify_release_environment
validate_assets
manifest="@MANIFEST_PATH@"
test -z "$(find release-manifest -mindepth 1 -maxdepth 1 ! -type f -print -quit)"
test "$(find release-manifest -mindepth 1 -maxdepth 1 -type f -printf '%f\n')" = "@MANIFEST_NAME@"
tag="generator-$GITHUB_SHA"
verify_attestation "$manifest"
verify_canonical_manifest "$manifest" "$tag" "$linux_sha" "$macos_arm_sha" "$macos_x64_sha"
if git ls-remote --quiet --exit-code --refs "https://github.com/$GITHUB_REPOSITORY.git" "refs/tags/$tag" >/dev/null 2>&1; then
  echo "release tag already exists: $tag" >&2
  exit 1
else
  status="$?"
  test "$status" -eq 2 || exit "$status"
fi
gh release create "$tag" --repo "$GITHUB_REPOSITORY" --target "$GITHUB_SHA" --title "$tag" --latest=false --notes "velnor-actions @VERSION@ built from $GITHUB_SHA." @LINUX_DIR@/@LINUX_BIN@ @LINUX_DIR@/@LINUX_SUM@ @MACOS_ARM_DIR@/@MACOS_ARM_BIN@ @MACOS_ARM_DIR@/@MACOS_ARM_SUM@ @MACOS_X64_DIR@/@MACOS_X64_BIN@ @MACOS_X64_DIR@/@MACOS_X64_SUM@ "$manifest"
"#;

pub(super) fn prepare_manifest(version: &str, assets: &AssetNames) -> String {
    script(version, assets, PREPARE_BODY)
}

pub(super) fn publish(version: &str, assets: &AssetNames) -> String {
    script(version, assets, PUBLISH_BODY)
}

fn script(version: &str, assets: &AssetNames, body: &str) -> String {
    let template = format!(
        "set -eu\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        super::CATALOG_VERSION,
        super::GH_SETUP,
        super::CI_CHECK,
        super::ENVIRONMENT_CHECK,
        super::RELEASE_CONTEXT_CHECK,
        WORKSPACE_VERSION,
        ASSET_HELPERS,
        body
    );
    replacements(template, version, assets)
}

fn replacements(mut script: String, version: &str, assets: &AssetNames) -> String {
    let manifest_name = format!("velnor-actions-release-manifest-{version}.json");
    let manifest_path = format!("release-manifest/{manifest_name}");
    for (key, value) in [
        ("@LINUX_DIR@", "linux-assets"),
        ("@LINUX_BIN@", assets.linux_bin.as_str()),
        ("@LINUX_SUM@", assets.linux_sum.as_str()),
        ("@MACOS_ARM_DIR@", "macos-arm64-assets"),
        ("@MACOS_ARM_BIN@", assets.macos_arm_bin.as_str()),
        ("@MACOS_ARM_SUM@", assets.macos_arm_sum.as_str()),
        ("@MACOS_X64_DIR@", "macos-x64-assets"),
        ("@MACOS_X64_BIN@", assets.macos_x64_bin.as_str()),
        ("@MACOS_X64_SUM@", assets.macos_x64_sum.as_str()),
        ("@LINUX_TARGET@", "x86_64-unknown-linux-gnu"),
        ("@MACOS_ARM_TARGET@", "aarch64-apple-darwin"),
        ("@MACOS_X64_TARGET@", "x86_64-apple-darwin"),
        ("@REPOSITORY@", "tailrocks/velnor-new"),
        ("@VERSION@", version),
        ("@WORKFLOW@", ".github/workflows/generator-release.yml"),
        ("@MANIFEST_NAME@", manifest_name.as_str()),
        ("@MANIFEST_PATH@", manifest_path.as_str()),
    ] {
        script = script.replace(key, value);
    }
    script
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_and_asset_names_follow_generator_version() {
        let version = "0.1.1";
        let generated = script(version, &AssetNames::for_version(version), PREPARE_BODY);

        assert!(generated.contains("velnor-actions-0.1.1-x86_64-unknown-linux-gnu"));
        assert!(generated.contains("velnor-actions-0.1.1-aarch64-apple-darwin"));
        assert!(generated.contains("velnor-actions-0.1.1-x86_64-apple-darwin"));
        assert!(generated.contains("velnor-actions-release-manifest-0.1.1.json"));
        assert!(generated.contains("test \"$RELEASE_VERSION\" = \"0.1.1\""));
        assert!(!generated.contains("velnor-actions-0.1.0-"));
    }
}
