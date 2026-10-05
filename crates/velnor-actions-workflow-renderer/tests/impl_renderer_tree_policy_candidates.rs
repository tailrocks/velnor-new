use super::{argv_of, fixture_ctx, fixture_ir, simple_job, validator_commands};
use velnor_actions_contract::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    CANDIDATE_JOB_ID, CandidateSpec, RenderError, checkout_step, plan_step, render_workflow_ir,
};

#[test]
fn candidate_never_plans_and_lock_matches_catalog_per_target() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let policy = WorkflowPolicy::VelnorRepositoryV1;
    let mut lonely = fixture_ir()?;
    lonely.jobs.insert(
        CANDIDATE_JOB_ID.to_owned(),
        simple_job(
            "Lonely",
            Vec::new(),
            vec![checkout_step(&super::checkout_pin())?],
        ),
    );
    assert!(render_workflow_ir(&lonely, policy, None, &ctx).is_err());
    let mut planner = fixture_ir()?;
    planner.jobs.insert(
        CANDIDATE_JOB_ID.to_owned(),
        simple_job(
            "Planning candidate",
            vec!["plan".to_owned()],
            vec![checkout_step(&super::checkout_pin())?, plan_step()],
        ),
    );
    assert!(render_workflow_ir(&planner, policy, None, &ctx).is_err());
    let mut thief = fixture_ir()?;
    thief.jobs.insert(
        "velnor-task".to_owned(),
        simple_job(
            "Task",
            vec![CANDIDATE_JOB_ID.to_owned()],
            vec![checkout_step(&super::checkout_pin())?],
        ),
    );
    thief.jobs.insert(
        CANDIDATE_JOB_ID.to_owned(),
        simple_job(
            "Candidate",
            vec!["plan".to_owned()],
            vec![checkout_step(&super::checkout_pin())?],
        ),
    );
    assert!(render_workflow_ir(&thief, policy, None, &ctx).is_err());
    Ok(())
}

#[test]
fn candidate_job_renders_with_plan_dependency() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(CandidateSpec {
        build: argv_of(&[
            "mise",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.19.0",
            "--",
            "build",
        ]),
        qualify: argv_of(&["sh", "-c", "plan-and-check"]),
    });
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir()?,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    assert!(text.contains("candidate:"));
    assert!(text.contains("plan"));
    assert!(text.contains("release:"));
    assert!(text.contains("ref_protected"));
    assert!(text.contains("actions/download-artifact@"));
    assert!(
        text.contains("env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "build must unset:\n{text}"
    );
    assert!(
        text.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
        "qualify must unset:\n{text}"
    );
    assert!(
        text.contains("GITHUB_TOKEN: \"\""),
        "candidate steps must scrub:\n{text}"
    );
    Ok(())
}

#[test]
fn candidate_qualify_rejects_non_shell_vectors() {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(CandidateSpec {
        build: argv_of(&[
            "mise",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.19.0",
            "--",
            "build",
        ]),
        qualify: argv_of(&["mise", "run", "qualify"]),
    });
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let ir = fixture_ir().expect("ir");
    let err = render_workflow_ir(
        &ir,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )
    .expect_err("non-shell qualify must fail");
    assert!(
        format!("{err:?}").contains("qualify_without_unset"),
        "wrong rejection: {err:?}"
    );
}
