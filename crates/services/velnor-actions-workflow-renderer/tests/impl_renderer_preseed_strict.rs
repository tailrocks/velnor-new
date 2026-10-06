//! Pre-seed strict closure gates: staged acceptance, gap rejection.

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_workflow_renderer::{
    PRESEED_STAGE_NAME, PRESEED_VERIFY_MANIFEST_NAME, RenderError, render_workflow_ir_strict,
};

use super::impl_renderer_fixtures::*;
use super::impl_renderer_preseed::{preseed_final, preseed_plan};

#[test]
fn strict_preseed_accepts_staged_internal_steps() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.preseed = true;
    let text = render_workflow_ir_strict(
        &fixture_ir(vec![preseed_plan()?, preseed_final()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    )?;
    let plan = step_names(&text, "plan");
    for name in [
        "Build helper",
        "Stage helper",
        "Check generated files",
        "Plan",
        "Publish plan",
    ] {
        assert!(
            plan.iter().any(|step| step.contains(name)),
            "plan misses {name}: {plan:?}"
        );
    }
    let build_at = plan.iter().position(|s| s.contains("Build helper"));
    let stage_at = plan.iter().position(|s| s.contains("Stage helper"));
    let check_at = plan.iter().position(|s| s.contains("Check generated"));
    assert!(build_at < stage_at && stage_at < check_at, "{plan:?}");
    let final_steps = step_names(&text, "required");
    let download_at = final_steps
        .iter()
        .position(|s| s.contains("Download helper"));
    let merge_at = final_steps.iter().position(|s| s.contains("Merge reports"));
    assert!(
        download_at.is_some() && download_at < merge_at,
        "final downloads before merge: {final_steps:?}"
    );
    Ok(())
}

#[test]
fn strict_preseed_closure_rejects_gaps() {
    let mut ctx = fixture_ctx();
    ctx.preseed = true;
    let (id, mut plan) = preseed_plan().expect("plan");
    plan.steps
        .retain(|step| step.name != "Upload helper (pre-seed trust-on-review)");
    let err = render_workflow_ir_strict(
        &fixture_ir(vec![(id, plan), preseed_final().expect("final")]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    );
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_incomplete")),
        "plan without upload accepted"
    );
    let (id, mut final_job) = preseed_final().expect("final");
    final_job
        .steps
        .retain(|step| step.name != "Download helper (pre-seed trust-on-review)");
    let err = render_workflow_ir_strict(
        &fixture_ir(vec![preseed_plan().expect("plan"), (id, final_job)]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    );
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_incomplete")),
        "final without download accepted"
    );
    let (id, mut unstaged) = preseed_final().expect("final");
    unstaged
        .steps
        .retain(|step| step.name != PRESEED_STAGE_NAME);
    let err = render_workflow_ir_strict(
        &fixture_ir(vec![preseed_plan().expect("plan"), (id, unstaged)]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    );
    assert!(
        err.is_err_and(|err| err.to_string().contains("internal_without_acquire")),
        "final without stage accepted"
    );
    let (id, mut misordered) = preseed_final().expect("final");
    let stage_at = misordered
        .steps
        .iter()
        .position(|step| step.name == PRESEED_STAGE_NAME)
        .expect("stage step");
    let stage = misordered.steps.remove(stage_at);
    misordered.steps.insert(0, stage);
    let err = render_workflow_ir_strict(
        &fixture_ir(vec![preseed_plan().expect("plan"), (id, misordered)]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    );
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_misordered")),
        "stage-before-download accepted"
    );
}

#[test]
fn strict_preseed_closure_rejects_plan_manifest_gap() {
    let mut ctx = fixture_ctx();
    ctx.preseed = true;
    let (id, mut plan) = preseed_plan().expect("plan");
    plan.steps
        .retain(|step| step.name != "Write helper manifest (pre-seed trust-on-review)");
    let err = render_workflow_ir_strict(
        &fixture_ir(vec![(id, plan), preseed_final().expect("final")]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    );
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_incomplete:plan:manifest")),
        "plan without manifest accepted"
    );
}

#[test]
fn strict_preseed_closure_rejects_verify_gaps() {
    let mut ctx = fixture_ctx();
    ctx.preseed = true;
    let (id, mut unverified) = preseed_final().expect("final");
    unverified
        .steps
        .retain(|step| step.name != PRESEED_VERIFY_MANIFEST_NAME);
    let err = render_workflow_ir_strict(
        &fixture_ir(vec![preseed_plan().expect("plan"), (id, unverified)]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    );
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_incomplete")),
        "final without manifest verify accepted"
    );
    let (id, mut verify_late) = preseed_final().expect("final");
    let verify_at = verify_late
        .steps
        .iter()
        .position(|step| step.name == PRESEED_VERIFY_MANIFEST_NAME)
        .expect("verify step");
    let verify = verify_late.steps.remove(verify_at);
    verify_late.steps.push(verify);
    let err = render_workflow_ir_strict(
        &fixture_ir(vec![preseed_plan().expect("plan"), (id, verify_late)]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    );
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_misordered")),
        "verify-after-stage accepted"
    );
}

#[test]
fn strict_rejects_stage_without_preseed_mode() {
    let ctx = fixture_ctx();
    let err = render_workflow_ir_strict(
        &fixture_ir(vec![
            preseed_plan().expect("plan"),
            preseed_final().expect("final"),
        ]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &ctx,
        &mise(),
    );
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_stage_without_mode")),
        "smuggled stage accepted"
    );
}
