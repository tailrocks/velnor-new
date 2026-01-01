//! G4: every rendered job carries its per-kind `timeout-minutes`.
use std::collections::BTreeMap;
use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_contract_workflow::JobTimeout;
use velnor_actions_workflow_jobs::msrv::msrv_job;
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::{
    RenderError, checkout_step, merge_step, plan_step, shell_step, write_request_step,
};

use super::impl_renderer_fixtures::*;

/// `timeout-minutes` values per job section in render order.
fn job_timeouts(text: &str) -> BTreeMap<String, String> {
    let mut timeouts = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
            current = Some(line.trim().trim_end_matches(':').to_owned());
        } else if let (Some(id), Some(value)) =
            (current.as_ref(), line.strip_prefix("    timeout-minutes: "))
        {
            timeouts.insert(id.clone(), value.to_owned());
        }
    }
    timeouts
}

fn consumer_ir() -> Result<velnor_actions_contract_workflow::WorkflowIr, RenderError> {
    let (plan_id, mut plan) = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    plan.timeout_minutes = JobTimeout::PLAN;
    let (lint_id, mut lint) = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![shell_step("Run", vec!["true".to_owned()], BTreeMap::new())?],
    );
    lint.timeout_minutes = JobTimeout::VALIDATOR;
    let (gate_id, mut gate) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![checkout_step(&checkout_pin())?, merge_step()],
    );
    gate.timeout_minutes = JobTimeout::REQUIRED;
    gate.condition = Some("always()".to_owned());
    Ok(fixture_ir(vec![
        (plan_id, plan),
        (lint_id, lint),
        (gate_id, gate),
    ]))
}

#[test]
fn every_job_emits_its_per_kind_timeout() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &consumer_ir()?,
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let timeouts = job_timeouts(&text);
    assert_eq!(
        timeouts,
        BTreeMap::from([
            ("plan".to_owned(), "10".to_owned()),
            ("actionlint".to_owned(), "10".to_owned()),
            ("required".to_owned(), "10".to_owned()),
        ]),
        "{text}"
    );
    assert!(
        text.contains("runs-on: ubuntu-26.04\n    timeout-minutes:"),
        "timeout follows runs-on: {text}"
    );
    Ok(())
}

#[test]
fn support_candidate_release_and_msrv_jobs_carry_per_kind_timeouts() -> Result<(), RenderError> {
    let plan = minimal_plan_job()?;
    let (gate_id, mut gate) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            acquire_fixture()?,
            write_request_step("merge-v1")?,
            merge_step(),
        ],
    );
    gate.condition = Some("always()".to_owned());
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir(vec![plan, (gate_id, gate)]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &candidate_ctx(),
    )?;
    let timeouts = job_timeouts(&text);
    for (id, want) in [
        ("alint", "10"),
        ("cargo-deny", "10"),
        ("cargo-machete", "10"),
        ("zizmor", "10"),
        ("candidate", "30"),
        ("release", "10"),
    ] {
        assert_eq!(
            timeouts.get(id).map(String::as_str),
            Some(want),
            "{id}: {text}"
        );
    }
    let msrv = msrv_job(
        LABEL,
        &checkout_pin(),
        &velnor_actions_workflow_jobs::msrv::MsrvSpec {
            package: "demo".to_owned(),
            rust_version: "1.98".to_owned(),
        },
        vec![
            "mise".to_owned(),
            "exec".to_owned(),
            "rust@1.98".to_owned(),
            "--".to_owned(),
            "cargo".to_owned(),
            "check".to_owned(),
            "--locked".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    assert_eq!(msrv.timeout_minutes, JobTimeout::MSRV);
    Ok(())
}
