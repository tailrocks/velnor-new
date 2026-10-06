//! Consumer manifest file: Acquire provenance comes from the committed file.
//!
//! Generating and checking binaries read the same committed
//! `.velnor/release-manifest.json`, so identical output follows by
//! construction. Absent, invalid, or version-mismatched files fail
//! closed in every build mode; no bake or environment carries provenance.
use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use crate::impl_common::{
    TestResult, config_with_branch, git, make_repo, without_ambient_identity,
};

/// Realistic release manifest: GitHub asset URLs at the generator version.
///
/// The version must match the workspace package version or the consumer
/// acquire gate rejects it; URLs mirror the real release-asset shape so
/// rendering faces realistic input.
fn release_manifest_json() -> String {
    manifest_with_version(env!("CARGO_PKG_VERSION"))
}

/// Release manifest at an explicit version (mismatch fixtures).
fn manifest_with_version(version: &str) -> String {
    // Distinctive digest proving the Acquire step copies the committed file.
    let sha = "c".repeat(64);
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
    .iter()
    .map(|target| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{sha}\"}}"
        )
    })
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "d".repeat(40)
    )
}

/// Render the consumer workflow YAML for a repo carrying `manifest`.
fn render_consumer_yaml(manifest: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    fs::write(repo.path().join(".velnor/release-manifest.json"), manifest)?;
    let prep = prepare(repo.path())?;
    velnor_actions_workflow_renderer::render_workflow_ir(
        &prep.workflow.ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
    )
    .map_err(|err| format!("render: {err}").into())
}

/// Golden Acquire step: URL plus digest copied from the committed file.
///
/// The scrub overlay renders its nine empty keys ahead of the asset
/// env (sorted map order); the download itself stays unchanged.
fn expected_acquire_block() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "- name: Acquire Velnor\n        run: \"sh -c 'unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR; d=\\\"$RUNNER_TEMP/velnor/bin/velnor-actions-{version}\\\"&&mkdir -p \\\"${{d%/*}}\\\"&&s=\\\"/opt/velnor/seed/generator/${{d##*/}}\\\"&&p=\\\"$VELNOR_ASSET_SHA256  \\\"&&if [ -f \\\"$s\\\" ]&&echo \\\"$p$s\\\"|sha256sum -c -;then cp \\\"$s\\\" \\\"$d\\\";else curl -fsSL --retry 5 --retry-all-errors --proto '\\\\''=https'\\\\'' --tlsv1.2 \\\"$VELNOR_ASSET_URL\\\" -o \\\"$d\\\"&&echo \\\"$p$d\\\"|sha256sum -c -;fi&&chmod +x \\\"$d\\\"'\"",
    )
}

fn acquire_block(yaml: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let acquire = yaml
        .find("- name: Acquire Velnor")
        .ok_or("missing acquire step")?;
    let tail = &yaml[acquire..];
    let end = tail["- name: ".len()..]
        .find("- name: ")
        .map_or(tail.len(), |at| at + "- name: ".len());
    Ok(tail[..end].trim_end())
}

#[test]
fn acquire_url_and_sha_come_from_committed_file() -> TestResult {
    let yaml = render_consumer_yaml(&release_manifest_json())?;
    assert!(yaml.contains("ACTIONS_ID_TOKEN_REQUEST_TOKEN: \"\""));
    assert_eq!(acquire_block(&yaml)?, expected_acquire_block());
    Ok(())
}

#[test]
fn manifest_version_mismatch_fails_prepare() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(
        repo.path().join(".velnor/release-manifest.json"),
        manifest_with_version("9.9.9"),
    )?;
    let err = prepare(repo.path()).expect_err("version mismatch fails");
    assert!(
        err.to_string()
            .contains("release_manifest_version_mismatch"),
        "{err}"
    );
    Ok(())
}

#[test]
fn invalid_manifest_fails_prepare() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::write(
        repo.path().join(".velnor/release-manifest.json"),
        "not json",
    )?;
    let err = prepare(repo.path()).expect_err("invalid manifest fails");
    assert!(err.to_string().contains("malformed_json"), "{err}");
    Ok(())
}

#[test]
fn duplicate_manifest_keys_fail_prepare() {
    let duplicate =
        release_manifest_json().replacen("\"schema\":1,", "\"schema\":1,\"schema\":1,", 1);
    assert!(
        render_consumer_yaml(&duplicate).is_err(),
        "duplicate manifest keys must be rejected"
    );
}

#[test]
fn absent_manifest_fails_closed_without_provenance() {
    use velnor_actions_orchestrator::consumer_acquire_step_with_manifest;
    // The release twin returns `None` for an absent file; the pure gate
    // must fail closed with the contract error (no URL, no digest).
    let err = consumer_acquire_step_with_manifest("ubuntu-26.04", env!("CARGO_PKG_VERSION"), None)
        .expect_err("absent manifest fails");
    let text = err.to_string();
    assert!(text.contains("consumer_requires_release_install"), "{text}");
    assert!(text.contains("official"), "{text}");
}

#[test]
#[cfg(debug_assertions)]
fn debug_absent_file_keeps_standin() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    fs::remove_file(repo.path().join(".velnor/release-manifest.json"))?;
    let prep = prepare(repo.path())?;
    let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
        &prep.workflow.ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
    )
    .map_err(|err| format!("render: {err}"))?;
    let version = env!("CARGO_PKG_VERSION");
    let expect = format!(
        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-x86_64-unknown-linux-gnu"
    );
    assert!(yaml.contains(&expect), "debug stand-in preserved:\n{yaml}");
    Ok(())
}

#[test]
fn golden_manifest_fixture_uses_the_canonical_release_schema() -> TestResult {
    let manifest = velnor_actions_contract::ReleaseManifest::parse_json(
        include_str!("../../../fixtures/consumer-release-manifest.json"),
        "fixtures/consumer-release-manifest.json",
    )?;
    manifest.validate("fixtures/consumer-release-manifest.json")?;
    assert_eq!(manifest.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        manifest.targets.len(),
        velnor_actions_contract::ReleaseTarget::ALL.len()
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn symlink_manifest_fails_closed() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let manifest = repo.path().join(".velnor/release-manifest.json");
    let target = repo.path().join(".velnor/real-manifest.json");
    fs::rename(&manifest, &target)?;
    std::os::unix::fs::symlink(&target, &manifest)?;
    let err = prepare(repo.path()).expect_err("symlink manifest fails");
    assert!(
        err.to_string().contains("symlink_refused"),
        "unexpected: {err}"
    );
    Ok(())
}

/// The producer policy does not consume or synthesize a consumer manifest.
#[test]
fn velnor_repository_generation_needs_no_consumer_manifest() -> TestResult {
    without_ambient_identity(
        "velnor_repository_generation_needs_no_consumer_manifest",
        || {
            let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\npolicy = \"velnor-repository-v1\"\n";
            let repo = make_repo(config)?;
            fs::remove_file(repo.path().join(".velnor/release-manifest.json"))?;
            git(
                &[
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/tailrocks/velnor-new.git",
                ],
                repo.path(),
            )?;

            let prep = prepare(repo.path())?;
            assert_eq!(prep.discovery.consumer_manifest_json, None);
            let tree = render_staged_tree(&prep)?;
            let yaml = tree
                .get(WORKFLOW_PATH)
                .ok_or_else(|| std::io::Error::other("missing generated CI workflow"))?;
            assert!(!yaml.contains("Acquire Velnor"), "{yaml}");
            assert!(!yaml.contains(&"a".repeat(64)), "{yaml}");
            assert!(!yaml.contains(&"b".repeat(40)), "{yaml}");
            Ok(())
        },
    )
}
