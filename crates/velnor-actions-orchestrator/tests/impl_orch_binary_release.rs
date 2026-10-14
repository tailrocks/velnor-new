//! Generic Cargo binary-release workflow emission and ownership.

use std::fs;

use velnor_actions_orchestrator::{plan_text_checked, prepare, render_staged_tree};

use crate::impl_common::{TestResult, anchor_repo, config_with_branch, git, make_repo, plan_for};

fn track_lock(root: &std::path::Path) -> TestResult {
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    git(&["add", "Cargo.lock"], root)?;
    anchor_repo(root)?;
    Ok(())
}

const CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.binary_release]\nenabled = true\nmanifest_path = \"Cargo.toml\"\npackage = \"demo\"\nbin = \"demo\"\nsource_commit_env = \"REPO_SCAN_SOURCE_COMMIT\"\n";
const CONFIG_WITH_ACTION_OVERRIDES: &str = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
[stacks.rust.binary_release]
enabled = true
manifest_path = "Cargo.toml"
package = "demo"
bin = "demo"
source_commit_env = "REPO_SCAN_SOURCE_COMMIT"
[actions.overrides]
"actions/checkout" = { version = "v7.0.1", sha = "3d3c42e5aac5ba805825da76410c181273ba90b1" }
"actions/download-artifact" = { version = "v8.0.1", sha = "3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c" }
"actions/upload-artifact" = { version = "v7.0.1", sha = "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a" }
"#;

#[test]
fn binary_release_is_generated_and_planned_only_when_enabled() -> TestResult {
    let enabled = make_repo(CONFIG)?;
    fs::write(enabled.path().join("src/main.rs"), "fn main() {}\n")?;
    track_lock(enabled.path())?;
    let prep = prepare(enabled.path())?;
    let tree = render_staged_tree(&prep)?;
    let workflow = tree
        .files
        .iter()
        .find(|file| file.path == ".github/workflows/binary-release.yml")
        .expect("enabled binary release output");
    let yaml = &workflow.bytes;
    for expected in [
        "name: Rust binary release",
        "workflow_dispatch: {}",
        "consumer-binary-release-${{ github.repository }}",
        "release-eligibility:",
        "build-binary:",
        "attest-binary:",
        "publish-binary:",
        "needs.release-eligibility.outputs.source_sha",
        "cargo metadata --format-version 1 --locked --no-deps",
        "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8",
        "contents: write",
        "attestations: write",
        "name: consumer-binary-release",
        "timeout-minutes: 75",
        "refs/tags/$tag",
        "aarch64-apple-darwin",
        "gh release create",
        "--verify-tag --draft --latest=false",
        "require_absent",
        "assert_protected_environment",
        "IMMUTABILITY_READ_TOKEN",
        "isImmutable == true",
        "2026.10.9",
        "rust@1.99.0",
    ] {
        assert!(yaml.contains(expected), "missing `{expected}`");
    }
    assert!(!yaml.contains("inputs:"), "dispatch has no inputs");
    assert!(!yaml.contains("pull_request"), "release has no PR trigger");
    assert!(!yaml.contains("push:"), "release has no push trigger");
    assert!(!yaml.contains("cron:"), "release is not scheduled");
    assert!(
        !yaml.contains("x86_64-unknown-linux-gnu"),
        "consumer release builds one Apple Silicon binary"
    );
    let plan = plan_for(&prep)?;
    assert!(plan.contains(".github/workflows/binary-release.yml"));

    Ok(())
}

#[test]
fn disabled_binary_release_is_not_generated_or_planned() -> TestResult {
    let disabled = make_repo(config_with_branch())?;
    let prep = prepare(disabled.path())?;
    let tree = render_staged_tree(&prep)?;
    assert!(
        tree.files
            .iter()
            .all(|file| file.path != ".github/workflows/binary-release.yml")
    );
    assert!(!plan_text_checked(&prep)?.contains(".github/workflows/binary-release.yml"));
    Ok(())
}

#[test]
fn binary_release_keeps_compiled_artifact_action_pins() -> TestResult {
    let repo = make_repo(CONFIG_WITH_ACTION_OVERRIDES)?;
    fs::write(repo.path().join("src/main.rs"), "fn main() {}\n")?;
    track_lock(repo.path())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let workflow = tree
        .files
        .iter()
        .find(|file| file.path == ".github/workflows/binary-release.yml")
        .expect("enabled binary release output");
    for (expected, count) in [
        (
            "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
            3,
        ),
        (
            "uses: actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333",
            2,
        ),
        (
            "uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9",
            1,
        ),
        (
            "uses: actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8",
            1,
        ),
    ] {
        assert_eq!(
            workflow.bytes.matches(expected).count(),
            count,
            "{expected}"
        );
    }
    assert!(
        !workflow
            .bytes
            .contains("3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c"),
        "download-artifact override must not retarget the consumer release"
    );
    assert!(
        !workflow
            .bytes
            .contains("043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"),
        "upload-artifact override must not retarget the consumer release"
    );
    Ok(())
}

#[test]
fn unapproved_binary_release_action_pin_override_fails_generation() -> TestResult {
    let config = r#"
schema = 1
[workflow]
name = "CI"
default_branch = "testmain"
[stacks.rust.binary_release]
enabled = true
manifest_path = "Cargo.toml"
package = "demo"
bin = "demo"
source_commit_env = "REPO_SCAN_SOURCE_COMMIT"
[actions.overrides]
"jdx/mise-action" = { version = "v5.1.2", sha = "1111111111111111111111111111111111111111" }
"#;
    let repo = make_repo(config)?;
    fs::write(repo.path().join("src/main.rs"), "fn main() {}\n")?;
    track_lock(repo.path())?;
    let prep = prepare(repo.path())?;
    let error = render_staged_tree(&prep).expect_err("unapproved mise action ref must fail closed");
    assert!(error.to_string().contains("unapproved_pair"), "{error}");
    Ok(())
}
