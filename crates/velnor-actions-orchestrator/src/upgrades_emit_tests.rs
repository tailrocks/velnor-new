use velnor_actions_contract::config::UpgradesConfig;
use velnor_actions_workflow_renderer::WorkflowRoutingPolicy;
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::upgrades::{UpgradesSpec, render_upgrades_workflow};
use velnor_actions_workflow_renderer::yaml::{Yaml, render_yaml};

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

fn spec(verify_rust_script: String) -> UpgradesSpec {
    UpgradesSpec {
        config: UpgradesConfig {
            enabled: true,
            ..UpgradesConfig::default()
        },
        runs_on: "ubuntu-26.04".to_owned(),
        checkout_uses: CHECKOUT.to_owned(),
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@8418fedb1110381a0d3ba51a7a415d7c1b884550".to_owned(),
            version: "2026.10.18".to_owned(),
            sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        },
        gh_version: "2.103.0".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generate_tools: vec![
            "jactionlint@2.3.0".to_owned(),
            "shellcheck@0.11.0".to_owned(),
        ],
        verify_rust_script,
        jactionlint_verify: velnor_actions_workflow_renderer::jactionlint_verifier_script(
            velnor_actions_mise::JACTIONLINT_VERSION,
        ),
    }
}

fn job_text<'a>(
    workflow: &'a str,
    id: &str,
    next_id: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    let marker = format!("\n  {id}:\n");
    let start = workflow
        .find(&marker)
        .ok_or_else(|| std::io::Error::other(format!("missing {id} job")))?;
    let next_marker = format!("\n  {next_id}:\n");
    let end = workflow[start + 1..]
        .find(&next_marker)
        .map_or(workflow.len(), |offset| start + 1 + offset);
    Ok(&workflow[start..end])
}

#[test]
fn real_rust_verifier_is_emitted_complete_in_prepare_and_verify()
-> Result<(), Box<dyn std::error::Error>> {
    let verify_rust_script = crate::workflow::toolchain_script(
        "${VELNOR_TOOL_RUST}",
        crate::workflow::ToolchainPhase::Installed,
        velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS,
    );
    let file = render_upgrades_workflow(
        &spec(verify_rust_script.clone()),
        &WorkflowRoutingPolicy::hosted_only(),
    )?;
    let expected_script =
        velnor_actions_contract::config::project_tool_refs::project_tool_refs_to_expressions(
            &verify_rust_script,
        );
    let expected = render_yaml(&Yaml::Map(vec![(
        "VELNOR_VERIFY_RUST_SCRIPT".to_owned(),
        Yaml::str(expected_script),
    )]));
    let expected_binding = format!("          {}", expected.trim_end());

    let prepare = job_text(&file.bytes, "prepare", "verify")?;
    let verify = job_text(&file.bytes, "verify", "publish")?;
    for (id, job) in [("prepare", prepare), ("verify", verify)] {
        assert_eq!(
            job.matches(&expected_binding).count(),
            1,
            "{id} must bind the complete production Rust verifier script",
        );
        assert!(
            job.contains("sh -c \\\"$VELNOR_VERIFY_RUST_SCRIPT\\\""),
            "{id} must execute the complete bound script",
        );
    }
    Ok(())
}
