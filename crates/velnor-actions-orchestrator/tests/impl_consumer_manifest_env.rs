//! Consumer runtime-manifest env: Plan + Check carry it, Acquire/Velnor don't.
use std::fs;

use velnor_actions_orchestrator::prepare;

use crate::impl_common::{TestResult, config_with_branch, git, make_repo};

/// Step-env key carrying the release manifest into Check/Plan steps.
const MANIFEST_KEY: &str = "VELNOR_RELEASE_MANIFEST_JSON";

/// Realistic release manifest: GitHub asset URLs at the generator version.
///
/// Version must match the workspace package version or the consumer
/// acquire gate rejects it; URLs mirror the real release-asset shape so
/// the private-subcommand scan and YAML quoting face realistic input.
fn release_manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ]
    .iter()
    .map(|target| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
            "c".repeat(64)
        )
    })
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{targets}]}}"
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

/// Byte offsets of every manifest-env occurrence in the YAML.
fn manifest_offsets(yaml: &str) -> Vec<usize> {
    yaml.match_indices(MANIFEST_KEY).map(|(at, _)| at).collect()
}

#[test]
fn consumer_plan_and_check_carry_manifest_env() -> TestResult {
    let manifest = release_manifest_json();
    assert!(
        !velnor_actions_mise::command::is_reserved_env_key(MANIFEST_KEY),
        "manifest key must pass the reserved-key gate"
    );
    velnor_actions_workflow_renderer::scan_for_private_subcommands(&manifest)
        .map_err(|err| format!("manifest trips subcommand scan: {err}"))?;
    let yaml = render_consumer_yaml(&manifest)?;
    let offsets = manifest_offsets(&yaml);
    assert_eq!(
        offsets.len(),
        2,
        "Plan + Check carry it, nothing else:\n{yaml}"
    );
    let check = yaml
        .find("Check generated files")
        .ok_or("missing freshness step")?;
    let plan = yaml.find("- name: Plan").ok_or("missing plan step")?;
    assert!(
        offsets[0] > check && offsets[0] < plan,
        "first occurrence sits in Check generated files"
    );
    assert!(offsets[1] > plan, "second occurrence sits in Plan");
    let escaped = manifest.replace('\\', "\\\\").replace('"', "\\\"");
    assert!(
        yaml.contains(&format!("{MANIFEST_KEY}: \"{escaped}\"")),
        "Str emitter must double-quote the JSON:\n{yaml}"
    );
    let acquire = yaml
        .find("- name: Acquire Velnor")
        .ok_or("missing acquire step")?;
    let tail = &yaml[acquire..];
    let end = tail["- name: ".len()..]
        .find("- name: ")
        .map_or(tail.len(), |at| at + "- name: ".len());
    assert_eq!(
        tail[..end].trim_end(),
        expected_acquire_block(),
        "Acquire block must stay byte-identical"
    );
    Ok(())
}

/// Golden Acquire step: fixed argv, asset env only, never the manifest.
///
/// Proves the manifest env flows only into Plan + Check and the
/// acquisition path is untouched by the runtime-manifest change.
fn expected_acquire_block() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "- name: Acquire Velnor\n        env:\n          VELNOR_ASSET_SHA256: {}\n          VELNOR_ASSET_URL: https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-x86_64-unknown-linux-gnu\n        run: \"sh -c 'mkdir -p $RUNNER_TEMP/velnor/bin && curl -fsSL \\\"$VELNOR_ASSET_URL\\\" -o $RUNNER_TEMP/velnor/bin/velnor-actions-{version} && echo \\\"$VELNOR_ASSET_SHA256  $RUNNER_TEMP/velnor/bin/velnor-actions-{version}\\\" | sha256sum -c - && chmod +x $RUNNER_TEMP/velnor/bin/velnor-actions-{version}'\"",
        "c".repeat(64)
    )
}

#[test]
fn velnor_yaml_carries_no_manifest_env() -> TestResult {
    let repo = make_repo(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n",
    )?;
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
    let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
        &prep.workflow.ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
    )
    .map_err(|err| format!("render: {err}"))?;
    assert!(
        !yaml.contains(MANIFEST_KEY),
        "velnor policy must not carry the manifest env:\n{yaml}"
    );
    Ok(())
}
