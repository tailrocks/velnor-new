//! Native preview generation keeps stable authorization independent.
use super::{ENABLED, binary_repo};
use crate::impl_common::{TestResult, git};
use velnor_actions_orchestrator::{export_homebrew_source_policy, prepare, render_staged_tree};

fn preview_config(stable: bool) -> String {
    let stable_config = if stable {
        ENABLED.to_owned()
    } else {
        ENABLED
            .replace(
                "[stacks.rust.binary_release.stable]\nenabled = true",
                "[stacks.rust.binary_release.stable]\nenabled = false",
            )
            .replace(
                "[stacks.rust.release]\nenabled = true",
                "[stacks.rust.release]\nenabled = false",
            )
    }
    .replace("default_branch = \"testmain\"", "default_branch = \"main\"");
    format!("{stable_config}\n[stacks.rust.binary_release.preview]\nenabled = true\n")
}

fn tap_enabled_config(stable: bool) -> String {
    format!(
        "{}\n{}",
        preview_config(stable),
        r#"
[distribution.homebrew_tap]
enabled = true
repository = "example/homebrew-tap"
environment = "tap-update"
app_id_variable = "TAP_APP_ID"
app_private_key_secret = "TAP_APP_PRIVATE_KEY"
protection_observer = { app_id_variable = "TAP_PROTECTION_APP_ID", app_private_key_secret = "TAP_PROTECTION_APP_PRIVATE_KEY" }
merge = "squash-after-gates"
[distribution.homebrew_tap.destinations.preview]
formula = "Formula/demo-preview.rb"
[distribution.homebrew_tap.review_completion]
check_name = "Review completion"
app_id_variable = "REVIEW_COMPLETION_APP_ID"
expected_reviewers = [{ app_id_variable = "SECURITY_REVIEW_APP_ID" }]
[distribution.homebrew_formula]
schema = "velnor.homebrew-formula-spec.v1"
template = "velnor-homebrew-standalone-v1"
description = "Demo command line tool"
homepage = "https://example.com/demo"
license = "Apache-2.0"
channels = [{ channel = "preview", formula = "demo-preview", formula_class = "DemoPreview", destination = "Formula/demo-preview.rb", conflicts = [] }]
host = { product = "demo", targets = ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"], bins = ["demo"] }
auxiliaries = []
stable_sentinel = { date = "2026-10-24", reason = "Stable release is not active" }
"#
    )
}

fn tap_enabled_repo(config: &str) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = binary_repo(config)?;
    let source_policy = repo.path().join(".velnor/homebrew-release-policy.json");
    export_homebrew_source_policy(repo.path(), &source_policy)?;
    git(
        &["add", ".velnor/homebrew-release-policy.json"],
        repo.path(),
    )?;
    Ok(repo)
}

#[test]
fn preview_generates_independently_with_sorted_static_inventory() -> TestResult {
    for stable in [false, true] {
        let repo = binary_repo(&preview_config(stable))?;
        std::fs::write(
            repo.path().join("mise.toml"),
            format!(
                "[tools]\nrust = '{}'\n[tasks.release-preview]\nrun = 'true'\n",
                velnor_actions_mise::RUST_VERSION
            ),
        )?;
        git(&["add", "mise.toml"], repo.path())?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let preview = tree
            .get(".github/workflows/binary-preview.yml")
            .ok_or("preview workflow missing")?;
        assert!(preview.contains("execute-release-producer-v1"));
        assert!(preview.contains("sign-package-release-v1"));
        assert!(preview.contains("publish-package-release-v1"));
        assert!(preview.contains("preview-publish"));
        assert!(preview.contains("ref: ${{ steps.source-select.outputs.source_sha }}"));
        assert!(preview.contains("finalize-preview-source-v1"));
        assert!(!preview.contains("Fetch Cargo sources"));
        assert!(!preview.contains("metadata --locked --offline"));
        assert!(!preview.contains("cargo fetch --locked"));
        assert!(!preview.contains("CARGO_HOME: ${{ runner.temp }}/velnor/cargo"));
        assert!(!preview.contains("RUSTUP_HOME: ${{ runner.temp }}/velnor/rustup"));
        assert!(preview.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
        assert!(preview.contains("CARGO_REGISTRY_TOKEN"));
        let executor = preview
            .find("VELNOR_INTERNAL_OP: execute-release-producer-v1")
            .ok_or("preview executor missing")?;
        let step_start = preview[..executor]
            .rfind("      - name: ")
            .ok_or("preview executor step missing")?;
        let step_end = preview[executor..]
            .find("\n      - name: ")
            .map_or(preview.len(), |offset| executor + offset);
        let executor_step = &preview[step_start..step_end];
        assert!(!executor_step.contains("NODE_AUTH_TOKEN:"));
        assert!(!executor_step.contains("NPM_TOKEN:"));
        assert!(!executor_step.contains("${{ github.token }}"));
        assert!(!executor_step.contains("${{ secrets."));
        assert_eq!(
            tree.get(".github/workflows/binary-release.yml").is_some(),
            stable
        );
    }
    Ok(())
}

#[test]
fn version_only_change_preserves_both_generated_channel_policies() -> TestResult {
    let repo = binary_repo(&preview_config(true))?;
    let before = render_staged_tree(&prepare(repo.path())?)?;
    for file in ["Cargo.toml", "Cargo.lock"] {
        let path = repo.path().join(file);
        let bytes = std::fs::read_to_string(&path)?.replace("0.1.0", "0.2.0");
        std::fs::write(path, bytes)?;
    }
    let after = render_staged_tree(&prepare(repo.path())?)?;
    for path in [
        ".github/workflows/binary-preview.yml",
        ".github/workflows/binary-release.yml",
    ] {
        let original = before
            .get(path)
            .ok_or("original channel workflow missing")?;
        let updated = after.get(path).ok_or("updated channel workflow missing")?;
        assert!(original.contains("VELNOR_RELEASE_POLICY:"));
        assert!(!original.contains("base_version"));
        assert_eq!(
            original, updated,
            "version data must resolve at runtime: {path}"
        );
    }
    Ok(())
}

#[test]
fn xcode_preview_and_macos26_named_check_pass_real_staged_validators() -> TestResult {
    let config = preview_config(false)
        .replace(
            "[stacks.rust.binary_release.producer]",
            "[stacks.rust.binary_release.producer]\nrunner = \"xcode-27-arm64\"",
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
        format!(
            "[tools]\nrust = '{}'\n[tasks.release-preview]\nrun = 'true'\n[tasks.verify-native]\nrun = 'true'\n",
            velnor_actions_mise::RUST_VERSION
        ),
    )?;
    git(&["add", "mise.toml"], repo.path())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let preview = tree
        .get(".github/workflows/binary-preview.yml")
        .ok_or("preview missing")?;
    assert!(preview.contains("runs-on: ${{ matrix.runner }}"));
    assert!(preview.contains("payload_matrix"));
    assert!(!preview.contains("Resolve project tool versions"));
    assert!(!preview.contains("RUSTUP_TOOLCHAIN:"));
    let ci = tree.get(".github/workflows/ci.yml").ok_or("CI missing")?;
    assert!(ci.contains("check-native:"));
    assert!(ci.contains("runs-on: macos-26"));
    let lint_config = tree
        .get(".github/jactionlint.yaml")
        .ok_or("jactionlint config missing")?;
    // jactionlint knows both hosted labels natively, so neither is declared
    // self-hosted (that would make it treat GitHub's runners as self-hosted).
    assert!(!lint_config.contains("    - xcode-27\n"), "{lint_config}");
    assert!(!lint_config.contains("    - macos-26\n"), "{lint_config}");
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
    assert!(
        report
            .validated_by
            .iter()
            .any(|tool| tool.starts_with("jactionlint@"))
    );
    assert!(ci.contains("Resolve project tool versions"));
    assert!(ci.contains("zizmor: ignore[github-env]"));
    assert!(!preview.contains("zizmor: ignore[github-env]"));
    Ok(())
}

#[test]
fn preview_refuses_unknown_binary_before_emitting_publication_workflow() -> TestResult {
    let config = preview_config(false).replace("bins = [\"demo\"]", "bins = [\"missing\"]");
    let repo = binary_repo(&config)?;
    let error = render_staged_tree(&prepare(repo.path())?)
        .err()
        .ok_or("missing binary accepted")?;
    assert!(error.to_string().contains("missing_binary_target"));
    Ok(())
}

#[test]
fn preview_rejects_a_branch_that_cannot_pass_its_fixed_environment_policy() -> TestResult {
    let config =
        preview_config(false).replace("default_branch = \"main\"", "default_branch = \"develop\"");
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
    let repo = tap_enabled_repo(&tap_enabled_config(false))?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = tree
        .get(".github/workflows/binary-preview.yml")
        .ok_or("preview workflow missing")?;
    let lint_config = tree
        .get(".github/jactionlint.yaml")
        .ok_or("jactionlint config missing")?;
    assert!(
        lint_config.contains(
            "\nconfig-variables:\n  - REVIEW_COMPLETION_APP_ID\n  - SECURITY_REVIEW_APP_ID\n  - TAP_APP_ID\n  - TAP_PROTECTION_APP_ID\n"
        ),
        "{lint_config}"
    );
    let prepare = job(workflow, "preview-tap-prepare", Some("preview-tap-writer"))?;
    let writer = job(workflow, "preview-tap-writer", Some("preview-tap-merge"))?;
    let merge = job(workflow, "preview-tap-merge", None)?;

    assert!(prepare.contains("prepare-homebrew-tap-v1"));
    assert!(prepare.contains("VELNOR_HOMEBREW_FORMULA_SPEC"));
    assert!(!workflow.contains("package-update"));
    assert!(!workflow.contains("python"));
    assert!(!prepare.contains("environment: tap-update"));
    assert!(!prepare.contains("${{ secrets.TAP_APP_PRIVATE_KEY }}"));
    assert!(!prepare.contains("contents: write"));
    assert!(writer.contains("write-homebrew-tap-pr-v1"));
    assert!(writer.contains("environment: tap-update"));
    assert!(writer.contains("permission-contents: write"));
    assert!(!writer.contains("permission-administration"));
    assert!(merge.contains("merge-homebrew-tap-pr-v1"));
    assert!(merge.contains("environment: tap-update"));
    let (_, writer_mint) = merge
        .split_once("id: tap-token\n")
        .ok_or("writer mint missing")?;
    let (writer_mint, observer_mint) = writer_mint
        .split_once("id: tap-protection-token\n")
        .ok_or("observer mint missing")?;
    assert!(writer_mint.contains("permission-contents: write"));
    assert!(writer_mint.contains("permission-pull-requests: write"));
    assert!(!writer_mint.contains("permission-administration"));
    assert!(!writer_mint.contains("permission-checks"));
    assert!(observer_mint.contains("permission-administration: read"));
    assert!(!observer_mint.contains("permission-contents"));
    assert!(!observer_mint.contains("permission-pull-requests"));
    assert!(!observer_mint.contains("permission-checks"));
    assert!(!writer.contains("permission-checks"));
    assert!(!prepare.contains("VELNOR_TAP_PROTECTION_TOKEN"));
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

    let repo = binary_repo(&preview_config(false))?;
    let mut prep = prepare(repo.path())?;
    let pinned = ToolCatalog::pinned();
    prep.catalog = ToolCatalog::new(
        pinned.version(PinnedTool::Rust),
        pinned.version(PinnedTool::MrBoxington),
        "2.0.0",
        pinned.version(PinnedTool::Jactionlint),
        pinned.version(PinnedTool::Shellcheck),
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

#[test]
fn native_binary_generation_rejects_noncanonical_ci_names_before_discovery() -> TestResult {
    for config in [
        ENABLED.to_owned(),
        preview_config(false),
        preview_config(true),
    ] {
        let custom = config.replace("[workflow]", "[workflow]\nname = \"Custom CI\"");
        let repo = binary_repo(&custom)?;
        let error = prepare(repo.path()).expect_err("native custom CI name must fail validation");
        assert!(
            matches!(error, velnor_actions_orchestrator::OrchestratorError::Config {
            ref key_path, ref problem, ..
        } if key_path == "workflow.name" && problem == "native_binary_release_requires_canonical_ci_name:CI"),
            "{error}"
        );
        assert!(
            !repo
                .path()
                .join(".github/workflows/binary-preview.yml")
                .exists()
        );
        assert!(
            !repo
                .path()
                .join(".github/workflows/binary-release.yml")
                .exists()
        );
    }
    Ok(())
}

#[path = "impl_schema2_binary_lfs.rs"]
mod lfs;
#[path = "impl_schema2_binary_preview_lint.rs"]
mod lint;

#[path = "impl_schema2_binary_preview_stable_tap_reconcile.rs"]
mod stable_tap_reconcile;
