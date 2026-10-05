//! Asset validation, canonical manifest creation, and release publication.

use super::super::AssetNames;
use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;

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
  provenance="$4"
  unexpected="$(find "$dir" -mindepth 1 -maxdepth 1 ! -type f -print -quit)" || return 1
  test -z "$unexpected" || return 1
  actual_files="$(find "$dir" -mindepth 1 -maxdepth 1 -type f -printf '%f\n')" || return 1
  actual="$(LC_ALL=C sort <<EOF
$actual_files
EOF
)" || return 1
  expected="$(LC_ALL=C sort <<EOF
$asset
$sidecar
$provenance
EOF
)" || return 1
  test "$actual" = "$expected" || return 1
}
verify_sidecar() {
  asset="$1"
  sidecar="$2"
  name="$(basename "$asset")"
  expected="$(awk -v name="$name" '
    NR != 1 || NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ || $2 != name { exit 1 }
    { print $1 }
    END { if (NR != 1) exit 1 }
  ' "$sidecar")" || return 1
  actual_line="$(sha256sum -- "$asset")" || return 1
  actual="${actual_line%%  *}"
  actual_name="${actual_line#*  }"
  test "$actual_name" = "$asset" || return 1
  test "${#actual}" -eq 64 || return 1
  case "$actual" in
    ''|*[!0-9a-f]*) return 1 ;;
  esac
  test "$expected" = "$actual" || return 1
  printf '%s\n' "$actual"
}
verify_attestation() {
  gh attestation verify "$1" --repo "$GITHUB_REPOSITORY" --signer-workflow "$GITHUB_REPOSITORY/@WORKFLOW@" --source-digest "$GITHUB_SHA" --source-ref refs/heads/main --deny-self-hosted-runners
}
validate_assets() {
  assert_files @LINUX_DIR@ @LINUX_BIN@ @LINUX_SUM@ @LINUX_PROVENANCE@ || return 1
  assert_files @MACOS_ARM_DIR@ @MACOS_ARM_BIN@ @MACOS_ARM_SUM@ @MACOS_ARM_PROVENANCE@ || return 1
  assert_files @MACOS_X64_DIR@ @MACOS_X64_BIN@ @MACOS_X64_SUM@ @MACOS_X64_PROVENANCE@ || return 1
  verify_sidecar @LINUX_DIR@/@LINUX_BIN@ @LINUX_DIR@/@LINUX_SUM@ >/dev/null || return 1
  verify_sidecar @MACOS_ARM_DIR@/@MACOS_ARM_BIN@ @MACOS_ARM_DIR@/@MACOS_ARM_SUM@ >/dev/null || return 1
  verify_sidecar @MACOS_X64_DIR@/@MACOS_X64_BIN@ @MACOS_X64_DIR@/@MACOS_X64_SUM@ >/dev/null || return 1
  for artifact in @LINUX_DIR@/@LINUX_BIN@ @LINUX_DIR@/@LINUX_SUM@ @LINUX_DIR@/@LINUX_PROVENANCE@ @MACOS_ARM_DIR@/@MACOS_ARM_BIN@ @MACOS_ARM_DIR@/@MACOS_ARM_SUM@ @MACOS_ARM_DIR@/@MACOS_ARM_PROVENANCE@ @MACOS_X64_DIR@/@MACOS_X64_BIN@ @MACOS_X64_DIR@/@MACOS_X64_SUM@ @MACOS_X64_DIR@/@MACOS_X64_PROVENANCE@; do
    verify_attestation "$artifact" || return 1
  done
}
"#;

const PREPARE_BODY: &str = r#"
verify_same_sha_ci || exit 1
verify_release_environment || exit 1
validate_assets || exit 1
rust_version="$(catalog_version RUST_VERSION)"
mr_boxington_version="$(catalog_version MR_BOXINGTON_VERSION)"
bash scripts/generator-release/create-release-manifest.sh "@VERSION@" "@REPOSITORY@" "$rust_version" "$mr_boxington_version"
mkdir -p release-manifest
manifest="@MANIFEST_PATH@"
mv release-manifest.json "$manifest"
test -s "$manifest"
"#;

const PUBLISH_BODY: &str = r#"
verify_same_sha_ci || exit 1
verify_release_environment || exit 1
validate_assets || exit 1
manifest="@MANIFEST_PATH@"
unexpected="$(find release-manifest -mindepth 1 -maxdepth 1 ! -type f -print -quit)" || exit 1
test -z "$unexpected" || exit 1
actual_manifest="$(find release-manifest -mindepth 1 -maxdepth 1 -type f -printf '%f\n')" || exit 1
test "$actual_manifest" = "@MANIFEST_NAME@" || exit 1
verify_attestation "$manifest" || exit 1
rust_version="$(catalog_version RUST_VERSION)"
mr_boxington_version="$(catalog_version MR_BOXINGTON_VERSION)"
bash scripts/generator-release/create-release-manifest.sh "@VERSION@" "@REPOSITORY@" "$rust_version" "$mr_boxington_version"
cmp release-manifest.json "$manifest"
tag="v@VERSION@"
if git ls-remote --quiet --exit-code --refs "https://github.com/$GITHUB_REPOSITORY.git" "refs/tags/$tag" >/dev/null 2>&1; then
  echo "release tag already exists: $tag" >&2
  exit 1
else
  status="$?"
  test "$status" -eq 2 || exit "$status"
fi
gh release create "$tag" --repo "$GITHUB_REPOSITORY" --target "$GITHUB_SHA" --title "velnor-actions $tag" --latest=false --notes "velnor-actions @VERSION@ built from $GITHUB_SHA." @LINUX_DIR@/@LINUX_BIN@ @LINUX_DIR@/@LINUX_SUM@ @LINUX_DIR@/@LINUX_PROVENANCE@ @MACOS_ARM_DIR@/@MACOS_ARM_BIN@ @MACOS_ARM_DIR@/@MACOS_ARM_SUM@ @MACOS_ARM_DIR@/@MACOS_ARM_PROVENANCE@ @MACOS_X64_DIR@/@MACOS_X64_BIN@ @MACOS_X64_DIR@/@MACOS_X64_SUM@ @MACOS_X64_DIR@/@MACOS_X64_PROVENANCE@ "$manifest"
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
    let manifest_name = RELEASE_MANIFEST_FILENAME;
    let manifest_path = format!("release-manifest/{manifest_name}");
    for (key, value) in [
        ("@LINUX_DIR@", "linux-assets"),
        ("@LINUX_BIN@", assets.linux_bin.as_str()),
        ("@LINUX_SUM@", assets.linux_sum.as_str()),
        ("@LINUX_PROVENANCE@", assets.linux_provenance.as_str()),
        ("@MACOS_ARM_DIR@", "macos-assets"),
        ("@MACOS_ARM_BIN@", assets.macos_arm_bin.as_str()),
        ("@MACOS_ARM_SUM@", assets.macos_arm_sum.as_str()),
        (
            "@MACOS_ARM_PROVENANCE@",
            assets.macos_arm_provenance.as_str(),
        ),
        ("@MACOS_X64_DIR@", "macos-intel-assets"),
        ("@MACOS_X64_BIN@", assets.macos_x64_bin.as_str()),
        ("@MACOS_X64_SUM@", assets.macos_x64_sum.as_str()),
        (
            "@MACOS_X64_PROVENANCE@",
            assets.macos_x64_provenance.as_str(),
        ),
        ("@LINUX_TARGET@", "x86_64-unknown-linux-gnu"),
        ("@MACOS_ARM_TARGET@", "aarch64-apple-darwin"),
        ("@MACOS_X64_TARGET@", "x86_64-apple-darwin"),
        ("@REPOSITORY@", "tailrocks/velnor-new"),
        ("@VERSION@", version),
        ("@WORKFLOW@", ".github/workflows/generator-release.yml"),
        ("@MANIFEST_NAME@", manifest_name),
        ("@MANIFEST_PATH@", manifest_path.as_str()),
    ] {
        script = script.replace(key, value);
    }
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    use std::process::Command;
    #[cfg(unix)]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn manifest_and_asset_names_follow_generator_version() {
        let version = "0.1.1";
        let generated = script(version, &AssetNames::for_version(version), PREPARE_BODY);

        assert!(generated.contains("velnor-actions-0.1.1-x86_64-unknown-linux-gnu"));
        assert!(generated.contains("velnor-actions-0.1.1-aarch64-apple-darwin"));
        assert!(generated.contains("velnor-actions-0.1.1-x86_64-apple-darwin"));
        assert!(generated.contains(RELEASE_MANIFEST_FILENAME));
        assert!(!generated.contains("velnor-actions-release-manifest-0.1.1.json"));
        assert!(generated.contains("test \"$RELEASE_VERSION\" = \"0.1.1\""));
        assert!(!generated.contains("velnor-actions-0.1.0-"));
    }

    #[test]
    #[cfg(unix)]
    fn sidecar_failure_inside_command_substitution_never_returns_a_hash()
    -> Result<(), Box<dyn std::error::Error>> {
        let version = "0.1.1";
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root =
            std::env::temp_dir().join(format!("velnor-sidecar-{}-{timestamp}", std::process::id()));
        let bin = root.join("bin");
        fs::create_dir_all(&bin)?;
        let asset = root.join("artifact");
        let sidecar = root.join("artifact.sha256");
        fs::write(&asset, b"artifact bytes")?;

        let fake_sha256sum = bin.join("sha256sum");
        fs::write(
            &fake_sha256sum,
            "#!/bin/sh\ntest \"$1\" = \"--\" || exit 2\ntest \"$TEST_SHA_STATUS\" = 0 || exit \"$TEST_SHA_STATUS\"\nshift\nprintf '%s  %s\\n' \"$TEST_HASH\" \"$1\"\n",
        )?;
        fs::set_permissions(&fake_sha256sum, fs::Permissions::from_mode(0o755))?;

        let helpers = replacements(
            ASSET_HELPERS.to_owned(),
            version,
            &AssetNames::for_version(version),
        );
        let invocation = format!(
            "set -eu\n{helpers}\nhash=\"$(verify_sidecar \"$1\" \"$2\")\" || exit $?\nprintf '%s\\n' \"$hash\"\n"
        );
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let valid_sidecar = format!("{digest}  artifact\n");
        let cases = [
            (valid_sidecar.clone(), "0", true),
            ("malformed\n".to_owned(), "0", false),
            (format!("{}  artifact\n", "f".repeat(64)), "0", false),
            (valid_sidecar, "3", false),
        ];

        for (sidecar_contents, sha_status, expected_success) in cases {
            fs::write(&sidecar, &sidecar_contents)?;
            let output = Command::new("/bin/bash")
                .args(["-c", &invocation, "bash"])
                .arg(&asset)
                .arg(&sidecar)
                .env("PATH", &path)
                .env("TEST_HASH", digest)
                .env("TEST_SHA_STATUS", sha_status)
                .output()?;
            assert_eq!(
                output.status.success(),
                expected_success,
                "sidecar={sidecar_contents:?}, stdout={}, stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if expected_success {
                assert_eq!(output.stdout, format!("{digest}\n").as_bytes());
            } else {
                assert!(output.stdout.is_empty());
            }
        }

        fs::remove_dir_all(root)?;
        Ok(())
    }
}
