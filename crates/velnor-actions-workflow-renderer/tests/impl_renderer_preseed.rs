//! Pre-seed mode: build-once templates plus strict closure gates.
use velnor_actions_contract::{Job, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    PreseedStageSource, RenderError, checkout_step, merge_step, plan_step, preseed_build_step,
    preseed_download_step, preseed_manifest_script, preseed_manifest_step, preseed_stage_step,
    preseed_upload_step, preseed_verify_step, render_workflow_ir_strict,
};

use super::impl_renderer_fixtures::*;

const TARGET: &str = "x86_64-unknown-linux-gnu";
const MARK: &str = "(pre-seed trust-on-review)";

fn build_argv() -> Vec<String> {
    mise_argv(
        "rust@1.98.1",
        "mbx",
        &[
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ],
    )
}

fn preseed_plan() -> Result<(String, Job), RenderError> {
    let build = build_argv();
    Ok(job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            preseed_build_step(build.clone())?,
            preseed_verify_step()?,
            preseed_manifest_step(&build, TARGET)?,
            preseed_upload_step()?,
            preseed_stage_step(PreseedStageSource::LocalBuild, STAGED)?,
            plan_step(),
        ],
    ))
}

fn preseed_final() -> Result<(String, Job), RenderError> {
    let mut final_job = job(
        "velnor-final",
        "Velnor / Required",
        vec!["velnor-plan".to_owned()],
        vec![
            preseed_download_step()?,
            preseed_stage_step(PreseedStageSource::DownloadedArtifact, STAGED)?,
            merge_step(),
        ],
    );
    final_job.1.condition = Some("always()".to_owned());
    Ok(final_job)
}

#[test]
fn preseed_templates_carry_trust_mark_and_exact_artifact() -> Result<(), RenderError> {
    let build = build_argv();
    let manifest = preseed_manifest_step(&build, TARGET)?;
    for step in [
        preseed_build_step(build.clone())?,
        preseed_verify_step()?,
        manifest,
        preseed_upload_step()?,
        preseed_download_step()?,
        preseed_stage_step(PreseedStageSource::LocalBuild, STAGED)?,
    ] {
        assert!(
            step.name.ends_with(MARK),
            "unmarked pre-seed step: {:?}",
            step.name
        );
    }
    let script = preseed_manifest_script(TARGET, "rust@1.98.1");
    for key in [
        "\"schema\":1",
        "\"commit\":",
        "\"target\":",
        "\"toolchain\":",
        "\"sha256\":",
    ] {
        assert!(script.contains(key), "manifest misses {key}: {script}");
    }
    assert!(script.contains("$GITHUB_SHA"), "commit recorded: {script}");
    assert!(
        !script.contains('\''),
        "inner quotes break shellcheck quoting: {script}"
    );
    assert!(
        preseed_manifest_step(&build, "wasm32-unknown-unknown").is_err(),
        "bad target accepted"
    );
    assert!(
        preseed_manifest_step(&["mbx".to_owned()], TARGET).is_err(),
        "toolchain-less build accepted"
    );
    assert!(
        preseed_stage_step(PreseedStageSource::LocalBuild, "target/release/x").is_err(),
        "unstaged path accepted"
    );
    Ok(())
}

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
    let plan = step_names(&text, "velnor-plan");
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
    let final_steps = step_names(&text, "velnor-final");
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

#[test]
fn preseed_requires_velnor_policy() {
    let mut ctx = fixture_ctx();
    ctx.preseed = true;
    let err = strict(&fixture_ir(vec![minimal_plan_job().expect("plan")]), &ctx);
    assert!(
        err.is_err_and(|err| err.to_string().contains("preseed_requires_velnor_policy")),
        "consumer pre-seed accepted"
    );
}
