//! Final fan-in: fetch placement, publish, tolerant downloads, no wildcards.
use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;
use velnor_actions_contract_workflow::{NeedsConclusions, Permissions};
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
fn verdict_download_continues_fetch_retries_and_publish_always_runs() -> Result<(), RenderError> {
    let text = final_text()?;
    // Only the plan download tolerates absence (a failed plan must
    // still reach the merge verdict); the fetch step retries bounded
    // in-helper instead of masking failures (F5).
    assert_eq!(
        text.matches("continue-on-error: true").count(),
        1,
        "only the plan download tolerates absence:\n{text}"
    );
    let plan = step_block(&text, "Download plan");
    assert!(plan.contains("continue-on-error: true"), "{plan}");
    let fetch = step_block(&text, "Download every expected matrix artifact");
    assert!(
        !fetch.contains("continue-on-error"),
        "fetch must not mask failures:\n{fetch}"
    );
    // Fail-closed shape intact: merge runs and gates the verdict, the
    // final report always publishes.
    let merge = step_block(&text, "Merge reports");
    assert!(merge.contains("VELNOR_INTERNAL_OP: merge-v1"), "{merge}");
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
fn rendered_final_publish_carries_retention_days() -> Result<(), RenderError> {
    use velnor_actions_workflow_steps::steps::ARTIFACT_RETENTION_DAYS;
    let text = final_text()?;
    let publish = step_block(&text, "Publish final report");
    assert!(publish.contains("retention-days"), "{publish}");
    assert!(
        publish.contains(&ARTIFACT_RETENTION_DAYS.to_string()),
        "{publish}"
    );
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
    assert!(conclusions.gate_matches(&ir.jobs));
    // The emitted inventory value equals the finalized derivation, not
    // just its presence: parse the merge step scalar back to JSON.
    let (key, value) = conclusions.expected_env();
    assert_eq!(key, "VELNOR_NEEDS_EXPECTED");
    // JSON of the asserted `["plan"]` inventory above.
    assert_eq!(value, "[\"plan\"]");
    let line = merge
        .lines()
        .find(|line| line.contains("VELNOR_NEEDS_EXPECTED:"))
        .unwrap_or_else(|| panic!("merge lacks the expected line:\n{merge}"));
    let scalar = line
        .split_once("VELNOR_NEEDS_EXPECTED:")
        .unwrap_or(("", ""))
        .1
        .trim();
    let unquoted = scalar
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or_default();
    assert_eq!(unquoted.replace("\\\"", "\""), value);
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
fn expected_inventory_excludes_post_gate_jobs() -> Result<(), RenderError> {
    use velnor_actions_contract_workflow::NEEDS_EXPECTED_ENV;
    use velnor_actions_workflow_jobs::context::{
        FINAL_CONDITION, FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID,
    };
    let plan = job(
        PLAN_JOB_ID,
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let (final_id, mut final_job) = job(
        FINAL_JOB_ID,
        "Required",
        vec![PLAN_JOB_ID.to_owned()],
        vec![
            acquire_fixture()?,
            write_request_step("merge-v1")?,
            merge_step(),
        ],
    );
    final_job.condition = Some(FINAL_CONDITION.to_owned());
    // Downstream of the gate by construction: needs the gate, push-gated
    // like the real baseline publisher, so PR runs always skip it.
    let (publish_id, mut publish) = job(
        PUBLISH_JOB_ID,
        "Publish baseline",
        vec![FINAL_JOB_ID.to_owned()],
        vec![checkout_step(&checkout_pin())?],
    );
    publish.condition =
        Some("github.event_name == 'push' && github.ref == 'refs/heads/main'".to_owned());
    let ir = fixture_ir(vec![plan, (final_id, final_job), (publish_id, publish)]);
    // Acyclic by construction: the gate needs upstream only, the
    // downstream job needs the gate.
    assert_eq!(ir.jobs[FINAL_JOB_ID].needs, [PLAN_JOB_ID.to_owned()]);
    assert!(
        !ir.jobs[FINAL_JOB_ID]
            .needs
            .contains(&PUBLISH_JOB_ID.to_owned())
    );
    assert_eq!(ir.jobs[PUBLISH_JOB_ID].needs, [FINAL_JOB_ID.to_owned()]);
    let text = strict(&ir, &fixture_ctx())?;
    assert!(
        text.contains(&format!("  {PUBLISH_JOB_ID}:")),
        "downstream job renders: {text}"
    );
    let conclusions = NeedsConclusions::from_finalized_jobs(FINAL_JOB_ID, &ir.jobs)
        .map_err(RenderError::Contract)?;
    assert_eq!(conclusions.inventory, vec![PLAN_JOB_ID.to_owned()]);
    assert!(conclusions.gate_matches(&ir.jobs));
    let (key, value) = conclusions.expected_env();
    assert_eq!(key, NEEDS_EXPECTED_ENV);
    let merge = step_block(&text, "Merge reports");
    let line = merge
        .lines()
        .find(|line| line.contains(NEEDS_EXPECTED_ENV))
        .unwrap_or_else(|| panic!("merge lacks the expected line:\n{merge}"));
    let scalar = line
        .split_once(NEEDS_EXPECTED_ENV)
        .unwrap_or(("", ""))
        .1
        .trim()
        .trim_start_matches(':')
        .trim()
        .trim_matches('"');
    assert_eq!(scalar.replace("\\\"", "\""), value);
    assert!(
        !merge.contains(PUBLISH_JOB_ID),
        "merge expects upstream only:\n{merge}"
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
