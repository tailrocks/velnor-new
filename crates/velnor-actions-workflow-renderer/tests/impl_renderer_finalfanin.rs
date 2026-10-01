//! Final fan-in: fetch placement, publish, tolerant downloads, no wildcards.
use velnor_actions_contract::{GeneratorValidation, NeedsConclusions, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    RenderError, checkout_step, merge_step, plan_step, render_workflow_ir, write_request_step,
};

use super::impl_renderer_fixtures::*;

fn final_job() -> Result<(String, velnor_actions_contract::Job), RenderError> {
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
    Ok((id, job))
}

fn final_text() -> Result<String, RenderError> {
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
    strict(&fixture_ir(vec![plan, final_job()?]), &fixture_ctx())
}

/// Step block between its `- name:` line and the next step or job end.
fn step_block<'a>(text: &'a str, name: &str) -> &'a str {
    let start = text.find(&format!("- name: {name}")).unwrap_or(text.len());
    let rest = &text[start..];
    let end = rest[1..].find("- name:").map_or(rest.len(), |at| at + 1);
    &rest[..end]
}

#[test]
fn final_steps_follow_contract_order() -> Result<(), RenderError> {
    let text = final_text()?;
    let mut at = 0;
    for name in [
        "Download plan",
        "Download every expected matrix artifact",
        "Merge reports",
        "Publish final report",
    ] {
        let next = text[at..]
            .find(&format!("- name: {name}"))
            .unwrap_or_else(|| panic!("missing {name}:\n{text}"));
        at += next + name.len();
    }
    assert!(
        !text.contains("pattern:"),
        "no wildcard artifact matching:\n{text}"
    );
    Ok(())
}

#[test]
fn verdict_downloads_continue_and_publish_always_runs() -> Result<(), RenderError> {
    let text = final_text()?;
    assert_eq!(
        text.matches("continue-on-error: true").count(),
        2,
        "plan plus fetch tolerate absence:\n{text}"
    );
    let publish = step_block(&text, "Publish final report");
    assert!(publish.contains("if: always()"), "{publish}");
    assert!(
        publish.contains("velnor-final-r${{ github.run_id }}-a${{ github.run_attempt }}"),
        "{publish}"
    );
    assert!(publish.contains("final-report.json"), "{publish}");
    Ok(())
}

#[test]
fn merge_steps_carry_needs_channel_matching_final_needs() -> Result<(), RenderError> {
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
    let (final_id, mut final_job) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            acquire_fixture()?,
            write_request_step("merge-v1")?,
            merge_step(),
        ],
    );
    final_job.condition = Some("always()".to_owned());
    let ir = fixture_ir(vec![plan, (final_id, final_job)]);
    let text = strict(&ir, &fixture_ctx())?;
    assert_eq!(
        text.matches("VELNOR_NEEDS_JSON").count(),
        2,
        "write-request plus merge carry the channel:\n{text}"
    );
    let merge = step_block(&text, "Merge reports");
    assert!(
        merge.contains("VELNOR_NEEDS_JSON: ${{ toJSON(needs) }}"),
        "merge lacks the channel:\n{merge}"
    );
    assert!(
        merge.contains("VELNOR_NEEDS_EXPECTED:"),
        "merge lacks the expected inventory:\n{merge}"
    );
    assert_eq!(
        text.matches("VELNOR_NEEDS_EXPECTED").count(),
        2,
        "write-request plus merge carry the expected inventory:\n{text}"
    );
    let plan_region = text.split("  required:").next().unwrap_or_default();
    assert!(
        plan_region.contains("- name: Write request"),
        "plan has its own write-request:\n{text}"
    );
    assert!(
        !plan_region.contains("VELNOR_NEEDS_JSON"),
        "plan write-request must not carry the merge channel:\n{text}"
    );
    // Consumer IR carries no support jobs, so the fixture jobs are the
    // finalized set the renderer derives the channel from.
    let conclusions = NeedsConclusions::from_finalized_jobs("required", &ir.jobs)
        .map_err(RenderError::Contract)?;
    assert_eq!(conclusions.inventory, vec!["plan".to_owned()]);
    assert!(conclusions.gate_matches(&ir));
    Ok(())
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

#[test]
fn fetch_step_carries_auth_without_request_file() -> Result<(), RenderError> {
    let text = final_text()?;
    let fetch = step_block(&text, "Download every expected matrix artifact");
    for want in [
        "fetch-reports-v1",
        "GH_REPO",
        "github.repository",
        "GH_TOKEN",
        "github.token",
    ] {
        assert!(fetch.contains(want), "missing {want}:\n{fetch}");
    }
    assert!(
        !fetch.contains("VELNOR_REQUEST_FILE"),
        "fetch takes no request file:\n{fetch}"
    );
    Ok(())
}
