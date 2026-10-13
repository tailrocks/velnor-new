//! Pre-seed mode: build-once templates plus strict closure gates.
use velnor_actions_contract::{Job, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    INTERNAL_OP_ENV, PRESEED_BUILD_OUTPUT, PRESEED_MANIFEST_BINARY_ENV, PRESEED_MANIFEST_OUT_ENV,
    PRESEED_MANIFEST_TARGET_ENV, PRESEED_MANIFEST_TOOLCHAIN_ENV, PRESEED_STAGE_NAME,
    PRESEED_VERIFY_MANIFEST_NAME, PreseedStageSource, RenderError,
    WRITE_PRESEED_MANIFEST_OPERATION, checkout_step, merge_step, plan_step, preseed_build_step,
    preseed_download_step, preseed_manifest_step, preseed_manifest_verify_step, preseed_stage_step,
    preseed_upload_step, preseed_verify_step, render_workflow_ir_strict,
};

use super::impl_renderer_fixtures::*;

const TARGET: &str = "x86_64-unknown-linux-gnu";
const MARK: &str = "(pre-seed trust-on-review)";
const MBX_VERSION: &str = "1.19.0";

fn mbx_probe() -> Vec<String> {
    mise_argv("mr-boxington@1.19.0", "mbx", &["--version"])
}

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
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            preseed_build_step(build.clone())?,
            preseed_verify_step(&mbx_probe(), MBX_VERSION)?,
            preseed_manifest_step(&build, TARGET)?,
            preseed_upload_step()?,
            preseed_stage_step(PreseedStageSource::LocalBuild, STAGED)?,
            plan_step(),
        ],
    ))
}

fn preseed_final() -> Result<(String, Job), RenderError> {
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            preseed_download_step()?,
            preseed_manifest_verify_step(TARGET)?,
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
        preseed_verify_step(&mbx_probe(), MBX_VERSION)?,
        manifest,
        preseed_upload_step()?,
        preseed_download_step()?,
        preseed_manifest_verify_step(TARGET)?,
        preseed_stage_step(PreseedStageSource::LocalBuild, STAGED)?,
    ] {
        assert!(
            step.name.ends_with(MARK),
            "unmarked pre-seed step: {:?}",
            step.name
        );
    }
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
fn preseed_manifest_step_runs_fresh_binary_with_op() -> Result<(), RenderError> {
    let step = preseed_manifest_step(&build_argv(), TARGET)?;
    let velnor_actions_contract::StepKind::Shell { run, env } = &step.kind else {
        panic!("manifest must be a shell step");
    };
    assert_eq!(
        run,
        &vec![PRESEED_BUILD_OUTPUT.to_owned()],
        "fresh binary, no sh -c wrapper: {run:?}"
    );
    assert_eq!(
        env.get(INTERNAL_OP_ENV).map(String::as_str),
        Some(WRITE_PRESEED_MANIFEST_OPERATION),
        "manifest op selected: {env:?}"
    );
    assert_eq!(
        env.get(PRESEED_MANIFEST_BINARY_ENV).map(String::as_str),
        Some(PRESEED_BUILD_OUTPUT),
        "binary path: {env:?}"
    );
    assert_eq!(
        env.get(PRESEED_MANIFEST_OUT_ENV).map(String::as_str),
        Some("${{ runner.temp }}/velnor/preseed-output"),
        "expression-form out dir, no shell expansion: {env:?}"
    );
    assert_eq!(
        env.get(PRESEED_MANIFEST_TARGET_ENV).map(String::as_str),
        Some(TARGET),
        "literal target: {env:?}"
    );
    assert_eq!(
        env.get(PRESEED_MANIFEST_TOOLCHAIN_ENV).map(String::as_str),
        Some("rust@1.98.1"),
        "toolchain derived from the build vector: {env:?}"
    );
    for value in env.values() {
        assert!(
            !value.contains("printf") && !value.contains("sha256sum"),
            "no shell-composed JSON or digest: {value}"
        );
    }
    Ok(())
}

#[test]
fn preseed_verify_pins_binary_and_mbx_route() -> Result<(), RenderError> {
    let step = preseed_verify_step(&mbx_probe(), MBX_VERSION)?;
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        panic!("verify must be a shell step");
    };
    assert_eq!((run[0].as_str(), run[1].as_str()), ("sh", "-c"));
    for need in [
        "test -x target/release/velnor-actions",
        "mise --no-config --no-env --no-hooks exec mr-boxington@1.19.0 -- mbx --version",
        "grep -qxF \"mbx 1.19.0\"",
    ] {
        assert!(run[2].contains(need), "verify misses {need}: {}", run[2]);
    }
    assert!(
        !run[2].contains('\'') && !run[2].contains("$(") && !run[2].contains('`'),
        "verify keeps shellcheck-safe quoting: {}",
        run[2]
    );
    for bad_version in ["", "latest", "1.19", "v1.19.0", "1.19.0 "] {
        assert!(
            preseed_verify_step(&mbx_probe(), bad_version).is_err(),
            "version accepted: {bad_version}"
        );
    }
    for bad_probe in [
        Vec::new(),
        vec!["mbx".to_owned(), "--version".to_owned()],
        mise_argv("mr-boxington@1.19.0", "mbx", &["--help"]),
        mise_argv("mr-boxington@latest", "mbx", &["--version"]),
    ] {
        assert!(
            preseed_verify_step(&bad_probe, MBX_VERSION).is_err(),
            "probe accepted: {bad_probe:?}"
        );
    }
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
