//! Final fan-in: fetch placement, publish, tolerant downloads, no wildcards.
use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_contract_workflow::Permissions;
use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::{
    RenderError, checkout_step, merge_step, plan_step, write_request_step,
};

use super::impl_renderer_fixtures::*;

fn final_job() -> Result<(String, velnor_actions_contract_workflow::Job), RenderError> {
    let (id, mut job) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            acquire_fixture()?,
            write_request_step("merge-v1")?,
            merge_step(),
        ],
    );
    job.condition = Some("always()".to_owned());
    job.permissions = Some(Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::Read,
    });
    Ok((id, job))
}

/// Step block between its `- name:` line and the next step or job end.
fn step_block<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text.find(&format!("- name: {name}")).unwrap_or(text.len());
    let rest = &text[start..];
    let end = rest[1..].find("- name:").map_or(rest.len(), |at| at + 1);
    &rest[..end]
}

#[test]
fn candidate_mode_downloads_attestation_before_merge() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir(vec![plan, final_job()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &candidate_ctx(),
    )?;
    let download = step_block(&text, "Download candidate attestation");
    assert!(
        download.contains("velnor-candidate-r${{ github.run_id }}-a${{ github.run_attempt }}"),
        "candidate artifact name:\n{download}"
    );
    assert!(
        download.contains("/candidate"),
        "evidence subdir path:\n{download}"
    );
    let final_at = text.find("required:").expect("final job");
    let mut at = final_at;
    for name in [
        "Download plan",
        "Download candidate attestation",
        "Merge reports",
    ] {
        let next = text[at..]
            .find(&format!("- name: {name}"))
            .unwrap_or_else(|| panic!("missing {name}:\n{text}"));
        at += next + name.len();
    }
    let baseline_support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let mut baseline_ctx = fixture_ctx();
    baseline_ctx.validator_commands = validator_commands();
    let baseline = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, final_job()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&baseline_support),
        &baseline_ctx,
    )?;
    assert!(
        !baseline.contains("Download candidate attestation"),
        "non-candidate mode fetches no attestation:\n{baseline}"
    );
    Ok(())
}
