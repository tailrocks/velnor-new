use super::*;
use crate::yaml::render_yaml;

fn pins() -> RustToolchainQualificationPins {
    RustToolchainQualificationPins {
        mise_setup: crate::setup::MiseSetup {
            uses: format!("jdx/mise-action@{}", "a".repeat(40)),
            version: velnor_actions_mise::MISE_VERSION.to_owned(),
            sha256: crate::setup::MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        },
        mbx_version: velnor_actions_mise::MR_BOXINGTON_VERSION.to_owned(),
        rust_version: "1.99.0".to_owned(),
        manifest_url: "https://static.rust-lang.org/dist/channel-rust-1.99.0.toml".to_owned(),
        manifest_sha256: MANIFEST_SHA256.to_owned(),
    }
}

#[test]
fn qualification_measures_both_hosted_targets_without_claiming_a_result() {
    let jobs = jobs(&pins()).expect("catalog-aligned official pins render");
    assert_eq!(
        jobs.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        ["rust-toolchain-linux-x64", "rust-toolchain-macos-arm64"]
    );

    for (index, (id, body)) in jobs.iter().enumerate() {
        let rendered = render_yaml(body);
        let (runner, platform, artifact) = if index == 0 {
            ("ubuntu-26.04", "linux_x64", "rust-toolchain-linux-x64")
        } else {
            ("macos-26", "macos_arm64", "rust-toolchain-macos-arm64")
        };
        assert!(rendered.contains(&format!("runs-on: {runner}")), "{id}");
        assert!(
            rendered.contains("if: inputs.mode == 'rust-toolchain'"),
            "{id}"
        );
        assert!(rendered.contains("permissions:\n  contents: read"), "{id}");
        assert!(!rendered.contains("actions: write"), "{id}");
        assert!(rendered.contains("ref: ${{ github.sha }}"), "{id}");
        assert!(rendered.contains("persist-credentials: \"false\""), "{id}");
        assert!(rendered.contains("name: Setup Mise"), "{id}");
        assert!(rendered.contains("version: 2026.10.7"), "{id}");
        assert!(
            rendered.contains(if index == 0 {
                crate::setup::MISE_BINARY_SHA256_LINUX_X64
            } else {
                crate::setup::MISE_BINARY_SHA256_MACOS_ARM64
            }),
            "{id}"
        );
        assert!(
            rendered.contains("name: Install pinned MBX through Mise"),
            "{id}"
        );
        assert!(
            rendered.contains(
                "mise --no-config --no-env --no-hooks install \"mr-boxington@$MBX_VERSION\""
            ),
            "{id}"
        );
        assert!(rendered.contains("mise --no-config --no-env --no-hooks which mbx --tool \"mr-boxington@$MBX_VERSION\""), "{id}");
        assert!(rendered.contains("MBX_VERSION: 1.23.0"), "{id}");
        assert!(
            rendered.contains("--project-root \"$GITHUB_WORKSPACE\""),
            "{id}"
        );
        assert!(
            rendered.contains("--mbx-executable \"$MBX_EXECUTABLE\""),
            "{id}"
        );
        assert!(rendered.contains("--mbx-version \"$MBX_VERSION\""), "{id}");
        assert!(
            rendered.contains("python3 scripts/qualification/qualify_rust_toolchain.py"),
            "{id}"
        );
        assert!(rendered.contains("RUST_VERSION: 1.99.0"), "{id}");
        assert!(
            rendered.contains(&format!("QUALIFICATION_PLATFORM: {platform}")),
            "{id}"
        );
        assert!(rendered.contains(MANIFEST_SHA256), "{id}");
        assert!(
            rendered.contains("install official Rust components"),
            "{id}"
        );
        assert!(
            rendered.contains("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"),
            "{id}"
        );
        assert!(rendered.contains(&format!("name: {artifact}")), "{id}");
        assert!(rendered.contains("if-no-files-found: error"), "{id}");
        assert!(!rendered.contains("secrets."), "{id}");
        assert!(!rendered.contains("publish"), "{id}");
    }
}

#[test]
fn qualification_rejects_unreviewed_version_url_and_digest() {
    let mut candidate = pins();
    candidate.rust_version = "1.100.0".to_owned();
    assert!(jobs(&candidate).is_err());

    let mut candidate = pins();
    candidate.manifest_url = "https://example.invalid/channel.toml".to_owned();
    assert!(jobs(&candidate).is_err());

    let mut candidate = pins();
    candidate.manifest_sha256 = "a".repeat(64);
    assert!(jobs(&candidate).is_err());

    let mut candidate = pins();
    candidate.mbx_version = "1.23.1".to_owned();
    assert!(jobs(&candidate).is_err());
}
