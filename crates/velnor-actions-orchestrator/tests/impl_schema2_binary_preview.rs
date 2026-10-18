//! Native preview generation keeps stable authorization independent.
use super::{ENABLED, binary_repo};
use crate::impl_common::{TestResult, git};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

fn preview_config(stable: bool) -> Result<String, Box<dyn std::error::Error>> {
    let tool = serde_json::json!({"qualified_tools":[{
        "id":"rust","version":"1.98.1","backend":{"kind":"core","tool":"rust"},
        "options":{"kind":"rust","components":[],"targets":[]},"depends_on":[],
        "platforms":[{"platform":"linux_x64","artifacts":[{"url":"https://static.rust-lang.org/dist/rust-1.98.1-x86_64-unknown-linux-gnu.tar.xz","sha256":"a".repeat(64)}],"dependency_artifacts":[],"install_tree_sha256":"b".repeat(64),
            "executables":[{"name":"rustc","path":"bin/rustc","sha256":"c".repeat(64),"probe":{"kind":"version","expected":"rustc 1.98.1"}}]}]
    }]});
    let stable_config = if stable {
        ENABLED.to_owned()
    } else {
        ENABLED.replacen("enabled = true", "enabled = false", 1)
    }
    .replace("default_branch = \"testmain\"", "default_branch = \"main\"");
    Ok(format!(
        "{stable_config}\n{}\n{}",
        r#"
[stacks.rust.binary_release.preview]
enabled = true
version_env = "DEMO_VERSION_OVERRIDE"
[stacks.rust.binary_release.preview.evidence]
profile = "signed-sbom-v1"
[stacks.rust.binary_release.preview.producer]
task = "release-preview"
tools = ["rust"]
inputs = ["mise.toml", "Cargo.toml", "Cargo.lock", "src/main.rs"]
[[stacks.rust.binary_release.preview.products]]
id = "demo"
package = "demo"
bins = ["demo"]
targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]
archive = "tar.gz"
"#,
        toml::to_string(&tool)?
    ))
}

fn tap_enabled_config(stable: bool) -> Result<String, Box<dyn std::error::Error>> {
    Ok(format!(
        "{}\n{}",
        preview_config(stable)?,
        r#"
[distribution.homebrew_tap]
enabled = true
source = "rust.binary_release.preview"
repository = "example/homebrew-tap"
formula = "Formula/demo-preview.rb"
environment = "tap-update"
app_id_variable = "TAP_APP_ID"
app_private_key_secret = "TAP_APP_PRIVATE_KEY"
merge = "squash-after-gates"
[distribution.homebrew_tap.updater]
task = "package:update-preview"
source = { mise_config = "mise.toml", working_directory = "." }
runner = { label = "ubuntu-26.04", platform = "linux_x64", executor = "hosted" }
inputs = ["mise.toml", "Formula/demo-preview.rb", "scripts/package-update.sh"]
tools = ["jq", "python"]
timeout_minutes = 5
[distribution.homebrew_tap.review_completion]
check_name = "Review completion"
app_id_variable = "REVIEW_COMPLETION_APP_ID"
expected_reviewers = [{ app_id_variable = "SECURITY_REVIEW_APP_ID" }]
"#
    ))
}

#[test]
fn preview_generates_independently_with_sorted_static_inventory() -> TestResult {
    for stable in [false, true] {
        let repo = binary_repo(&preview_config(stable)?)?;
        std::fs::write(
            repo.path().join("mise.toml"),
            "[tools]\nrust = '1.98.1'\n[tasks.release-preview]\nrun = 'true'\n",
        )?;
        git(&["add", "mise.toml"], repo.path())?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let preview = tree
            .get(".github/workflows/binary-preview.yml")
            .ok_or("preview workflow missing")?;
        assert!(preview.contains("execute-preview-producer-v1"));
        assert!(preview.contains("sign-package-release-v1"));
        assert!(preview.contains("publish-package-release-v1"));
        assert!(preview.contains("preview-publish"));
        let fetch = preview
            .find("Fetch Cargo sources")
            .ok_or("P08 source preparation missing")?;
        let dispatch = preview
            .find("execute-preview-producer-v1")
            .ok_or("producer missing")?;
        assert!(fetch < dispatch);
        assert!(preview.contains("metadata --locked --offline"));
        assert!(preview.contains("cargo fetch --locked"));
        assert!(preview.contains("CARGO_HOME: ${{ runner.temp }}/velnor/cargo"));
        assert!(preview.contains("RUSTUP_HOME: ${{ runner.temp }}/velnor/rustup"));
        assert!(preview.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
        assert!(preview.contains("CARGO_REGISTRY_TOKEN"));
        assert!(preview.contains("NODE_AUTH_TOKEN NPM_TOKEN;"));
        assert_eq!(
            tree.get(".github/workflows/binary-release.yml").is_some(),
            stable
        );
    }
    Ok(())
}

#[test]
fn xcode_preview_and_macos26_named_check_pass_real_staged_validators() -> TestResult {
    let config = preview_config(false)?
        .replace(
            "[stacks.rust.binary_release.preview.producer]",
            "[stacks.rust.binary_release.preview.producer]\nrunner = \"xcode-27-arm64\"",
        )
        .replace("platform = \"linux_x64\"", "platform = \"macos_arm64\"")
        .replace(
            "rust-1.98.1-x86_64-unknown-linux-gnu",
            "rust-1.98.1-aarch64-apple-darwin",
        );
    let config = format!(
        "{config}\n{}",
        r#"[[checks]]
id = "native"
task = "verify-native"
inputs = ["mise.toml"]
tools = []
[checks.runner]
label = "macos-26"
platform = "macos_arm64"
executor = "hosted"
"#
    );
    let repo = binary_repo(&config)?;
    std::fs::write(
        repo.path().join("mise.toml"),
        "[tasks.release-preview]\nrun = 'true'\n[tasks.verify-native]\nrun = 'true'\n",
    )?;
    git(&["add", "mise.toml"], repo.path())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let preview = tree
        .get(".github/workflows/binary-preview.yml")
        .ok_or("preview missing")?;
    assert!(preview.contains("runs-on: xcode-27"));
    let ci = tree.get(".github/workflows/ci.yml").ok_or("CI missing")?;
    assert!(ci.contains("check-native:"));
    assert!(ci.contains("runs-on: macos-26"));
    let actionlint = tree
        .get(".github/actionlint.yaml")
        .ok_or("actionlint config missing")?;
    assert!(actionlint.contains("    - xcode-27\n"));
    assert!(actionlint.contains("    - macos-26\n"));
    let output = tempfile::tempdir()?;
    let report = velnor_actions_orchestrator::generate(
        &prep,
        &velnor_actions_orchestrator::GenerateOptions {
            output_dir: Some(output.path().join("preview")),
        },
    )?;
    assert!(
        report
            .files_written
            .iter()
            .any(|path| path == ".github/workflows/binary-preview.yml")
    );
    Ok(())
}

#[test]
fn preview_refuses_unknown_binary_before_emitting_publication_workflow() -> TestResult {
    let config = preview_config(false)?.replace("bins = [\"demo\"]", "bins = [\"missing\"]");
    let repo = binary_repo(&config)?;
    let error = render_staged_tree(&prepare(repo.path())?)
        .err()
        .ok_or("missing binary accepted")?;
    assert!(error.to_string().contains("preview"));
    Ok(())
}

#[test]
fn preview_rejects_a_branch_that_cannot_pass_its_fixed_environment_policy() -> TestResult {
    let config =
        preview_config(false)?.replace("default_branch = \"main\"", "default_branch = \"develop\"");
    let repo = binary_repo(&config)?;
    let error = render_staged_tree(&prepare(repo.path())?)
        .err()
        .ok_or("unsupported preview branch accepted")?;
    assert!(
        error
            .to_string()
            .contains("preview_default_branch_must_be_main")
    );
    Ok(())
}

#[test]
fn generator_emits_tap_jobs_from_native_distribution_config() -> TestResult {
    let repo = binary_repo(&tap_enabled_config(false)?)?;
    std::fs::write(
        repo.path().join("mise.toml"),
        "[tools]\nrust = '1.98.1'\n[tasks.release-preview]\nrun = 'true'\n",
    )?;
    git(&["add", "mise.toml"], repo.path())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = tree
        .get(".github/workflows/binary-preview.yml")
        .ok_or("preview workflow missing")?;
    let prepare = job(workflow, "preview-tap-prepare", Some("preview-tap-writer"))?;
    let writer = job(workflow, "preview-tap-writer", Some("preview-tap-merge"))?;
    let merge = job(workflow, "preview-tap-merge", None)?;

    assert!(prepare.contains("prepare-homebrew-tap-v1"));
    assert!(!prepare.contains("environment: tap-update"));
    assert!(!prepare.contains("${{ secrets.TAP_APP_PRIVATE_KEY }}"));
    assert!(!prepare.contains("contents: write"));
    assert!(writer.contains("write-homebrew-tap-pr-v1"));
    assert!(writer.contains("environment: tap-update"));
    assert!(writer.contains("permission-contents: write"));
    assert!(!writer.contains("permission-administration"));
    assert!(merge.contains("merge-homebrew-tap-pr-v1"));
    assert!(merge.contains("environment: tap-update"));
    assert!(merge.contains("permission-checks: read"));
    assert!(merge.contains("permission-administration: read"));
    assert!(merge.contains("VELNOR_TAP_COMPLETION_APP_ID"));
    Ok(())
}

fn job<'a>(
    yaml: &'a str,
    name: &str,
    next: Option<&str>,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    let start = yaml
        .find(&format!("  {name}:\n"))
        .ok_or("generated job missing")?;
    let rest = &yaml[start..];
    if let Some(next) = next {
        let end = rest
            .find(&format!("  {next}:\n"))
            .ok_or("next generated job missing")?;
        Ok(&rest[..end])
    } else {
        Ok(rest)
    }
}

#[test]
fn privileged_preview_gh_pin_cannot_come_from_consumer_catalog() -> TestResult {
    use velnor_actions_mise::{PinnedTool, ToolCatalog};

    let repo = binary_repo(&preview_config(false)?)?;
    let mut prep = prepare(repo.path())?;
    let pinned = ToolCatalog::pinned();
    prep.catalog = ToolCatalog::new(
        pinned.version(PinnedTool::Rust),
        pinned.version(PinnedTool::MrBoxington),
        "2.0.0",
        pinned.version(PinnedTool::Actionlint),
        pinned.version(PinnedTool::Shellcheck),
        pinned.version(PinnedTool::Zizmor),
        pinned.version(PinnedTool::Nextest),
        pinned.version(PinnedTool::Opentofu),
    )?;
    let tree = render_staged_tree(&prep)?;
    let workflow = tree
        .get(".github/workflows/binary-preview.yml")
        .ok_or("preview workflow missing")?;
    assert!(workflow.contains(&format!("gh@{}", pinned.version(PinnedTool::Gh))));
    assert!(!workflow.contains("gh@2.0.0"));
    Ok(())
}
