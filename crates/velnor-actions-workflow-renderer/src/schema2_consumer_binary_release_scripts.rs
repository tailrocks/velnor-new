//! Exact-source identity and untrusted package-output handoff scripts.

/// Check one workspace package and binary against locked Cargo metadata.
pub(super) fn identity(metadata_command: &str) -> String {
    IDENTITY.replace("@METADATA_COMMAND@", metadata_command)
}

/// Build one raw binary and emit the closed package-output handoff.
pub(super) fn build(metadata_script: &str, target_add: &str, build: &str) -> String {
    BUILD
        .replace("@IDENTITY@", metadata_script)
        .replace("@TARGET_ADD@", target_add)
        .replace("@BUILD@", build)
}

const IDENTITY: &str = r#"set -euo pipefail
manifest_path="$GITHUB_WORKSPACE/$MANIFEST_PATH"
test -f "$manifest_path" && test ! -L "$manifest_path" || { echo 'manifest missing or symlinked' >&2; exit 1; }
workspace_root="$(cd "$(dirname "$manifest_path")" && pwd -P)"
expected_manifest="$workspace_root/$(basename "$manifest_path")"
test "$expected_manifest" = "$workspace_root/Cargo.toml" || { echo 'manifest must name workspace Cargo.toml' >&2; exit 1; }
manifest_path="$expected_manifest"
test -f "$workspace_root/Cargo.lock" && test ! -L "$workspace_root/Cargo.lock" || { echo 'Cargo.lock missing or symlinked' >&2; exit 1; }
git -C "$workspace_root" ls-files --error-unmatch Cargo.lock >/dev/null || { echo 'Cargo.lock is not tracked' >&2; exit 1; }
metadata="$( @METADATA_COMMAND@ )"
record="$(jq -cer --arg workspace "$workspace_root" --arg package "$PACKAGE_NAME" --arg bin "$BINARY_NAME" \
  '. as $doc | [.packages[] | select(.name == $package)] as $matches
   | if ($matches|length) != 1 then error("package must resolve once") else $matches[0] end
   | . as $package_record
   | if ($package_record.manifest_path|startswith($workspace+"/"))|not then error("package outside workspace") else . end
   | if $doc.workspace_root != $workspace or ($doc.workspace_members|index($package_record.id)) == null then error("not a workspace member") else . end
   | [.targets[] | select(.name == $bin and (.kind|index("bin")) != null)] as $bins
   | if ($bins|length) != 1 then error("binary target must resolve once") else $bins[0] end
   | if ((.["required-features"] // [])|length) != 0 then error("required-features unsupported") else . end
   | {package_version:$package_record.version,bin:.name}' <<<"$metadata")" || { echo 'Cargo metadata identity check failed' >&2; exit 1; }
version="$(jq -er '.package_version' <<<"$record")"
if [[ -n "${EXPECTED_VERSION-}" ]]; then
  test "$version" = "$EXPECTED_VERSION" || { echo 'Cargo version changed after eligibility' >&2; exit 1; }
fi
tag="$PACKAGE_NAME-v$version"
git check-ref-format "refs/tags/$tag" || { echo 'Cargo version cannot form a release tag' >&2; exit 1; }
printf 'package_version=%s\ntag=%s\n' "$version" "$tag" >> "$GITHUB_OUTPUT"
"#;

const BUILD: &str = r#"set -euo pipefail
umask 077
GITHUB_OUTPUT="$RUNNER_TEMP/binary-identity"
export GITHUB_OUTPUT
@IDENTITY@
version="$(sed -n 's/^package_version=//p' "$GITHUB_OUTPUT")"
tag="$(sed -n 's/^tag=//p' "$GITHUB_OUTPUT")"
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
output="$RUNNER_TEMP/velnor/preview-output"
test ! -e "$output" && test ! -L "$output" || { echo 'producer directory already exists' >&2; exit 1; }
mkdir -p "$output"
payload="$BINARY_NAME-$TARGET_TRIPLE"
cp "$binary" "$output/$payload"
digest="$(shasum -a 256 "$output/$payload" | awk '{print $1}')"
sbom="$payload.sbom.json"
jq -cnS --arg name "$payload" --arg digest "$digest" \
  '{bomFormat:"CycloneDX",specVersion:"1.6",version:1,metadata:{component:{type:"file",name:$name,hashes:[{alg:"SHA-256",content:$digest}]}}}' > "$output/$sbom"
sbom_digest="$(shasum -a 256 "$output/$sbom" | awk '{print $1}')"
jq -cnS --arg source_commit "$GITHUB_SHA" --arg source_ref "$GITHUB_REF" \
  --arg version "$version" --arg product "$PACKAGE_NAME" --arg target "$TARGET_TRIPLE" \
  --arg payload "$payload" --arg digest "$digest" --arg sbom "$sbom" --arg sbom_digest "$sbom_digest" \
  '{schema:"velnor.package-output.v1",source_commit:$source_commit,source_ref:$source_ref,version:$version,
    payloads:[{product_id:$product,target:$target,format:"raw-binary",name:$payload,sha256:$digest}],
    companions:[],sboms:[{payload_name:$payload,name:$sbom,sha256:$sbom_digest}]}' > "$output/package-output.json"
test -s "$output/$payload" && test -s "$output/$sbom" && test -s "$output/package-output.json"
"#;
