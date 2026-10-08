use velnor_actions_contract::RustBinaryReleaseConfig;

use crate::setup::{MISE_BINARY_SHA256_LINUX_X64, MISE_BINARY_SHA256_MACOS_ARM64, MiseSetup};

use super::{
    RustBinaryReleaseCommands, RustBinaryReleaseRequest, render_rust_binary_release_workflow,
};

fn exec(tool: &str, program: &str, args: &[&str]) -> Vec<String> {
    let mut result = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        tool.to_owned(),
        "--".to_owned(),
        program.to_owned(),
    ];
    result.extend(args.iter().map(ToString::to_string));
    result
}

fn request() -> RustBinaryReleaseRequest {
    let config = RustBinaryReleaseConfig {
        enabled: true,
        manifest_path: "Cargo.toml".to_owned(),
        package: "demo-package".to_owned(),
        binary: Some("demo-binary".to_owned()),
        source_commit_env: Some("REPO_SCAN_SOURCE_COMMIT".to_owned()),
    };
    let rust = "rust@1.98.1";
    let gh = "gh@2.102.0";
    RustBinaryReleaseRequest {
        config,
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        checkout_uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
        download_artifact_uses: crate::steps::DOWNLOAD_ARTIFACT_USES.to_owned(),
        upload_artifact_uses: crate::steps::UPLOAD_ARTIFACT_USES.to_owned(),
        linux_setup: MiseSetup {
            uses: "jdx/mise-action@c2a87611a18de5b3828c5652fe268e992400cb5c".to_owned(),
            version: "2026.10.4".to_owned(),
            sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        },
        macos_setup: MiseSetup {
            uses: "jdx/mise-action@c2a87611a18de5b3828c5652fe268e992400cb5c".to_owned(),
            version: "2026.10.4".to_owned(),
            sha256: MISE_BINARY_SHA256_MACOS_ARM64.to_owned(),
        },
        commands: commands(rust, gh),
    }
}

fn commands(rust: &str, gh: &str) -> RustBinaryReleaseCommands {
    let build = |target: &str| {
        exec(
            rust,
            "cargo",
            &[
                "build",
                "--locked",
                "--release",
                "--message-format=json",
                "--manifest-path",
                "Cargo.toml",
                "--package",
                "demo-package",
                "--bin",
                "demo-binary",
                "--target",
                target,
            ],
        )
    };
    RustBinaryReleaseCommands {
        install_rust: vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            rust.to_owned(),
        ],
        metadata: exec(
            rust,
            "cargo",
            &[
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--locked",
                "--manifest-path",
                "Cargo.toml",
            ],
        ),
        rustc_version: exec(rust, "rustc", &["-vV"]),
        build_linux_x86_64: build("x86_64-unknown-linux-gnu"),
        build_macos_arm64: build("aarch64-apple-darwin"),
        install_gh: vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "--no-env".to_owned(),
            "--no-hooks".to_owned(),
            "install".to_owned(),
            gh.to_owned(),
        ],
        gh_prefix: exec(gh, "gh", &[]),
        gh_version: "2.102.0".to_owned(),
        rust_version: "1.98.1".to_owned(),
    }
}

#[test]
fn renderer_emits_trusted_scheduled_release_and_repo_scan_build_env() {
    let yaml = render_rust_binary_release_workflow(&request()).expect("binary workflow");
    for expected in [
        "cron: 17 * * * *",
        "github.event_name == 'schedule'",
        "needs.verify-source.outputs.should_release == 'true'",
        "refs/tags/$tag",
        "--locked",
        "--release",
        "git merge-base --is-ancestor",
        "semver.fullmatch(version)",
        "functools.cmp_to_key(compare_tags)",
        "cannot order release tags by SemVer precedence",
        "env -u GH_TOKEN",
        "should_release=true",
        "source_sha=%s",
        "demo-package-v$version",
        "REPO_SCAN_SOURCE_COMMIT: ${{ env.SOURCE_SHA }}",
        "RUSTUP_TOOLCHAIN: 1.98.1",
        "needs.build-linux.outputs.artifact_id",
        "needs.build-macos.outputs.artifact_id",
        "demo-binary-${{ needs.verify-source.outputs.version }}-x86_64-unknown-linux-gnu.tar.gz",
        "demo-binary-${{ needs.verify-source.outputs.version }}-aarch64-apple-darwin.tar.gz",
        "demo-binary-${RELEASE_VERSION}-x86_64-unknown-linux-gnu.tar.gz",
        "demo-binary-${RELEASE_VERSION}-aarch64-apple-darwin.tar.gz",
        "Validate archives and prepare checksums",
        "Verify checksums, recheck tag, and publish",
        "release version is not valid SemVer",
        "release_args+=(--prerelease)",
        "tar -tzf \"$linux_source\"",
        "tar -tzf \"$macos_source\"",
        "rwxr-xr-x",
        "--repo \"$GITHUB_REPOSITORY\"",
        "compare/$source_sha...$default_sha",
        "contents: write",
        "SHA256SUMS",
        "remote tag moved after verification",
    ] {
        // Run scripts are emitted as YAML double-quoted scalars, so shell
        // quotes and backslashes are escaped in the serialized document.
        let serialized = expected.replace('\\', "\\\\").replace('"', "\\\"");
        assert!(yaml.contains(&serialized), "missing `{expected}`:\n{yaml}");
    }
    let prep = yaml
        .find("Validate archives and prepare checksums")
        .expect("archive validation step");
    let publish = yaml
        .find("Verify checksums, recheck tag, and publish")
        .expect("token-bearing publish step");
    let token = yaml.rfind("GH_TOKEN:").expect("publish token");
    assert!(prep < publish && publish < token);
    assert!(yaml[prep..publish].contains("tar -tzf"));
    let token_script = &yaml[publish..token];
    assert!(token_script.contains("prepared checksums do not match"));
    assert!(token_script.contains("remote tag moved after verification"));
    assert!(token_script.contains("source is no longer reachable"));
    assert!(token_script.contains("release create"));
    assert!(!token_script.contains("tar -t"));
    assert!(!token_script.contains("assets/incoming-"));
    assert!(!yaml.contains("push:"), "workflow has no push trigger");
    assert!(!yaml.contains("sort -Vr"), "tags are not naturally sorted");
    assert!(!yaml.contains("pull_request"), "workflow has no PR trigger");
    let publisher = &yaml[yaml.find("publish-release:").expect("publisher job")..];
    assert!(!publisher.contains("Check out exact source"));
    assert_eq!(yaml.matches("contents: write").count(), 1);
    assert_eq!(yaml.matches("GH_TOKEN:").count(), 2);
    assert!(!yaml.contains("actions: write"));
    assert!(
        !yaml.contains("@@"),
        "all trusted template fields substituted"
    );
}

#[test]
fn renderer_rejects_existing_releases_in_the_publisher() {
    let yaml = render_rust_binary_release_workflow(&request()).expect("binary workflow");
    for expected in [
        "cannot inventory existing releases with publisher token",
        "multiple releases use the selected tag",
        "release inventory shape is invalid",
        "selected release tag already has a draft release",
        "selected release tag already has a published release",
    ] {
        let serialized = expected.replace('\\', "\\\\").replace('"', "\\\"");
        assert!(yaml.contains(&serialized), "missing `{expected}`:\n{yaml}");
    }
    assert!(!yaml.contains("release upload"));
    assert!(!yaml.contains("draft=false"));
    assert!(!yaml.contains("resume_release_id"));
    assert!(
        !yaml.contains("outputs.resume_release_id"),
        "draft IDs are not read by the verifier"
    );
    let verify_start = yaml.find("verify-source:").expect("source verifier job");
    let verify_end = yaml.find("build-linux:").expect("Linux build job");
    let verifier = &yaml[verify_start..verify_end];
    assert!(verifier.contains("contents: read"));
    assert!(
        !verifier.contains("draft"),
        "read-only job does not inspect drafts"
    );
    let publish = yaml
        .find("Verify checksums, recheck tag, and publish")
        .expect("publisher step");
    let token = yaml.rfind("GH_TOKEN:").expect("publish token");
    let token_script = &yaml[publish..token];
    assert!(token_script.contains("--slurp"));
    assert!(yaml.contains("contents: write"));
    assert!(!token_script.contains("release upload"));
    assert!(!token_script.contains("draft=false"));
}

#[test]
fn renderer_uses_the_requested_checkout_and_artifact_action_refs() {
    let mut input = request();
    input.checkout_uses = "actions/checkout@1111111111111111111111111111111111111111".to_owned();
    input.download_artifact_uses =
        "actions/download-artifact@2222222222222222222222222222222222222222".to_owned();
    input.upload_artifact_uses =
        "actions/upload-artifact@3333333333333333333333333333333333333333".to_owned();

    let yaml = render_rust_binary_release_workflow(&input).expect("binary workflow");
    for (expected, count) in [
        (
            "uses: actions/checkout@1111111111111111111111111111111111111111",
            3,
        ),
        (
            "uses: actions/download-artifact@2222222222222222222222222222222222222222",
            2,
        ),
        (
            "uses: actions/upload-artifact@3333333333333333333333333333333333333333",
            2,
        ),
    ] {
        assert_eq!(yaml.matches(expected).count(), count, "{expected}");
    }
}

#[test]
fn renderer_rejects_commands_outside_the_pinned_cargo_shape() {
    let mut input = request();
    input.commands.build_linux_x86_64 = vec!["echo".to_owned(), "not cargo".to_owned()];
    let error = render_rust_binary_release_workflow(&input).expect_err("arbitrary command refused");
    assert!(
        error
            .to_string()
            .contains("binary_release_command_mismatch")
    );
}

#[test]
fn renderer_rejects_disabled_configuration() {
    let mut input = request();
    input.config.enabled = false;
    assert!(render_rust_binary_release_workflow(&input).is_err());
}
