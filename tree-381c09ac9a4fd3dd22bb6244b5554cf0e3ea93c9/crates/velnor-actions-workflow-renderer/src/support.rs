//! Policy gating and Velnor support-job construction.
//!
//! Consumer policy rejects support IR; Velnor policy merges typed Alint,
//! policy, and optional candidate jobs. No toolchain-qualification job exists.
//! The always-on `velnor-workflow-lint` job arrives via typed IR and is
//! emitted for both policies; it is never support IR.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, PolicyJob, Step, StepKind, VelnorSupportWorkflow};

use crate::{
    RenderError,
    render::{
        ALINT_JOB_ID, ALINT_USES, CANDIDATE_JOB_ID, CandidateSpec, FINAL_CONDITION,
        FINAL_DISPLAY_NAME, FINAL_JOB_ID, PLAN_JOB_ID, POLICY_JOB_ID, RenderContext, TASK_JOB_ID,
    },
    steps,
};

/// Always-on workflow-lint job ID, emitted for both policies.
pub(crate) const LINT_JOB_ID: &str = "velnor-workflow-lint";

/// Display name of the always-on workflow-lint job.
pub(crate) const LINT_DISPLAY_NAME: &str = "Velnor Workflow Lint";

/// Protected release job ID (Velnor policy, candidate mode only).
pub(crate) const RELEASE_JOB_ID: &str = "velnor-release";

/// Display name of the protected release job.
pub(crate) const RELEASE_DISPLAY_NAME: &str = "Velnor Release";

/// Release ref gate: the job runs only on protected refs (tags/branches).
pub(crate) const RELEASE_REF_CONDITION: &str = "github.ref_protected == true";

/// Consumer policy: reject support IR and Velnor-only job IDs.
///
/// The lint job is a base IR job, not support IR, so it passes through.
pub(crate) fn reject_consumer_support(
    jobs: &BTreeMap<String, Job>,
    support: Option<&VelnorSupportWorkflow>,
) -> Result<(), RenderError> {
    if let Some(workflow) = support
        && (!workflow.policy_jobs.is_empty() || workflow.candidate_validation)
    {
        return Err(RenderError::PolicyRejected {
            policy: "consumer-v1".to_owned(),
            problem: "support_ir_rejected".to_owned(),
        });
    }
    for id in [
        ALINT_JOB_ID,
        POLICY_JOB_ID,
        CANDIDATE_JOB_ID,
        RELEASE_JOB_ID,
    ] {
        if jobs.contains_key(id) {
            return Err(RenderError::PolicyRejected {
                policy: "consumer-v1".to_owned(),
                problem: format!("forbidden_job:{id}"),
            });
        }
    }
    Ok(())
}

/// Velnor policy: merge typed Alint, policy, and optional candidate jobs.
/// The exhaustive `PolicyJob` match is the whole support set: Alint plus
/// policy only, with no toolchain-qualification job.
pub(crate) fn merge_support_jobs(
    jobs: &mut BTreeMap<String, Job>,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let Some(workflow) = support else {
        return Ok(());
    };
    let mut want_alint = false;
    let mut want_policy = false;
    for job in &workflow.policy_jobs {
        match job {
            PolicyJob::Alint if !want_alint => want_alint = true,
            PolicyJob::Policy if !want_policy => want_policy = true,
            _ => {
                return Err(RenderError::PolicyRejected {
                    policy: "velnor-repository-v1".to_owned(),
                    problem: "duplicate_policy_job".to_owned(),
                });
            }
        }
    }
    if want_alint {
        insert_support_job(jobs, ALINT_JOB_ID, alint_job(ctx)?)?;
    }
    if want_policy {
        insert_support_job(jobs, POLICY_JOB_ID, policy_job(ctx)?)?;
    }
    if workflow.candidate_validation {
        let Some(spec) = &ctx.candidate else {
            return Err(RenderError::PolicyRejected {
                policy: "velnor-repository-v1".to_owned(),
                problem: "candidate_without_spec".to_owned(),
            });
        };
        insert_support_job(jobs, CANDIDATE_JOB_ID, candidate_job(ctx, spec)?)?;
        insert_support_job(jobs, RELEASE_JOB_ID, release_job(ctx)?)?;
    }
    extend_final_needs(jobs, want_alint, want_policy, workflow.candidate_validation);
    Ok(())
}

/// Extend the final gate with merged support IDs (workflow-contract §4).
///
/// `Velnor / Required` needs plan + task + lint always, plus alint, policy,
/// and candidate when those policy jobs exist. The release job never gates:
/// it publishes after merge on protected refs only.
fn extend_final_needs(
    jobs: &mut BTreeMap<String, Job>,
    alint: bool,
    policy: bool,
    candidate: bool,
) {
    let mut extra = Vec::new();
    for (want, id) in [
        (alint, ALINT_JOB_ID),
        (policy, POLICY_JOB_ID),
        (candidate, CANDIDATE_JOB_ID),
    ] {
        if want && jobs.contains_key(id) {
            extra.push(id.to_owned());
        }
    }
    let Some(final_job) = jobs.get_mut(FINAL_JOB_ID) else {
        return;
    };
    for id in extra {
        if !final_job.needs.contains(&id) {
            final_job.needs.push(id);
        }
    }
}

/// Insert a support job, failing on ID collision with IR jobs.
pub(crate) fn insert_support_job(
    jobs: &mut BTreeMap<String, Job>,
    id: &str,
    job: Job,
) -> Result<(), RenderError> {
    if jobs.contains_key(id) {
        return Err(RenderError::PolicyRejected {
            policy: "velnor-repository-v1".to_owned(),
            problem: format!("job_collision:{id}"),
        });
    }
    jobs.insert(id.to_owned(), job);
    Ok(())
}

/// Fixed Alint job: checkout plus the pinned-tag Alint action.
pub(crate) fn alint_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    let checkout = steps::checkout_step(&ctx.checkout_uses)?;
    let mut with = BTreeMap::new();
    with.insert("path".to_owned(), ".".to_owned());
    with.insert("config".to_owned(), ".alint.yml".to_owned());
    with.insert("format".to_owned(), "github".to_owned());
    with.insert("fail-on-warning".to_owned(), "true".to_owned());
    steps::scan_for_private_subcommands(ALINT_USES)?;
    Ok(Job {
        display_name: "Velnor Alint".to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: Vec::new(),
        condition: None,
        steps: vec![
            checkout,
            Step {
                name: "Run Alint".to_owned(),
                kind: StepKind::Action {
                    uses: ALINT_USES.to_owned(),
                    with,
                },
            },
        ],
    })
}

/// Fixed policy job: checkout plus caller-supplied validated commands.
pub(crate) fn policy_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    if ctx.policy_commands.is_empty() {
        return Err(RenderError::InvalidWorkflow(
            "policy_without_commands".to_owned(),
        ));
    }
    let mut rendered = Vec::with_capacity(ctx.policy_commands.len() + 1);
    rendered.push(steps::checkout_step(&ctx.checkout_uses)?);
    for command in &ctx.policy_commands {
        rendered.push(steps::shell_step(
            &command.name,
            command.argv.clone(),
            BTreeMap::new(),
        )?);
    }
    Ok(Job {
        display_name: "Velnor Policy".to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: Vec::new(),
        condition: None,
        steps: rendered,
    })
}

/// Candidate job: build once, upload with manifest, download, qualify.
///
/// Qualification runs against the downloaded artifact and never rebuilds
/// it (bootstrap contract §4 steps 4-5). Needs plan; feeds nothing.
pub(crate) fn candidate_job(ctx: &RenderContext, spec: &CandidateSpec) -> Result<Job, RenderError> {
    let target =
        velnor_actions_contract::target_for_runner_label(&ctx.runs_on).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("unsupported_target_for_runner:{}", ctx.runs_on))
        })?;
    let toolchain = toolchain_identity(&spec.build)?;
    let manifest = steps::candidate_manifest_script(target, &toolchain);
    Ok(Job {
        display_name: "Velnor Candidate".to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            steps::shell_step("Build candidate", spec.build.clone(), BTreeMap::new())?,
            steps::shell_step(
                "Write candidate manifest",
                vec!["sh".to_owned(), "-c".to_owned(), manifest],
                BTreeMap::new(),
            )?,
            steps::upload_artifact_step(
                steps::CANDIDATE_ARTIFACT_NAME,
                steps::CANDIDATE_OUTPUT_DIR,
            )?,
            steps::download_artifact_step(
                steps::CANDIDATE_ARTIFACT_NAME,
                steps::CANDIDATE_STAGE_DIR,
            )?,
            steps::shell_step("Qualify candidate", spec.qualify.clone(), BTreeMap::new())?,
        ],
    })
}

/// Protected release job: publish assets plus manifest, verify digests.
///
/// Runs only on protected refs (see [`RELEASE_REF_CONDITION`]); publishes
/// only the qualified candidate. The lock update lands as a separate
/// reviewed change (bootstrap contract §4 step 6), never from this job.
pub(crate) fn release_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    Ok(Job {
        display_name: RELEASE_DISPLAY_NAME.to_owned(),
        runs_on: ctx.runs_on.clone(),
        needs: vec![CANDIDATE_JOB_ID.to_owned()],
        condition: Some(RELEASE_REF_CONDITION.to_owned()),
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            steps::download_artifact_step(
                steps::CANDIDATE_ARTIFACT_NAME,
                steps::CANDIDATE_STAGE_DIR,
            )?,
            steps::shell_step(
                "Publish release assets",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "gh release upload \"$VELNOR_RELEASE_TAG\" \"$VELNOR_STAGE_DIR\"/velnor-actions --clobber".to_owned(),
                ],
                release_env(),
            )?,
            steps::shell_step(
                "Verify release digests",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "gh release download \"$VELNOR_RELEASE_TAG\" --dir \"$RUNNER_TEMP/velnor/verify\" --clobber && sha256sum -c \"$VELNOR_STAGE_DIR\"/candidate-manifest.json.sha256".to_owned(),
                ],
                release_env(),
            )?,
        ],
    })
}

/// Shared release-step env: release tag plus the staged candidate dir.
fn release_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "VELNOR_RELEASE_TAG".to_owned(),
            "${{ github.ref_name }}".to_owned(),
        ),
        (
            "VELNOR_STAGE_DIR".to_owned(),
            steps::CANDIDATE_STAGE_DIR.to_owned(),
        ),
    ])
}

/// Toolchain identity from the fixed build vector (`a@x+b@y`).
///
/// Joins the `tool@exact` specs between `exec` and `--`; fails when the
/// vector has no specs, so the manifest never carries a guessed toolchain.
fn toolchain_identity(build: &[String]) -> Result<String, RenderError> {
    let mut specs = Vec::new();
    let mut in_specs = false;
    for arg in build {
        if arg == "exec" {
            in_specs = true;
        } else if arg == "--" {
            break;
        } else if in_specs {
            specs.push(arg.clone());
        }
    }
    if specs.is_empty() || specs.iter().any(|spec| !spec.contains('@')) {
        return Err(RenderError::BadCommand(
            "candidate_build_without_toolchain".to_owned(),
        ));
    }
    Ok(specs.join("+"))
}

/// Candidate never plans: it needs plan, holds no plan step, feeds no task.
///
/// Qualification downloads the built artifact; a candidate without a
/// download step cannot prove the no-rebuild path and is rejected.
pub(crate) fn check_candidate_invariants(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if let Some(candidate) = jobs.get(CANDIDATE_JOB_ID) {
        if !candidate.needs.contains(&PLAN_JOB_ID.to_owned()) {
            return Err(RenderError::InvalidWorkflow(
                "candidate_must_need_plan".to_owned(),
            ));
        }
        for step in &candidate.steps {
            if let StepKind::Internal { operation } = &step.kind
                && operation == steps::PLAN_OPERATION
            {
                return Err(RenderError::InvalidWorkflow(
                    "candidate_must_not_plan".to_owned(),
                ));
            }
        }
        let downloaded = candidate.steps.iter().any(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. } if uses == steps::DOWNLOAD_ARTIFACT_USES)
        });
        if !downloaded {
            return Err(RenderError::InvalidWorkflow(
                "candidate_must_download_artifact".to_owned(),
            ));
        }
    }
    if let Some(task) = jobs.get(TASK_JOB_ID)
        && task.needs.contains(&CANDIDATE_JOB_ID.to_owned())
    {
        return Err(RenderError::InvalidWorkflow(
            "task_must_not_consume_candidate".to_owned(),
        ));
    }
    Ok(())
}

/// Final gate keeps the exact required-check name and `always()` condition.
///
/// The always-on lint job keeps its exact display name on both policies.
pub(crate) fn check_final_gate(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    if let Some(final_job) = jobs.get(FINAL_JOB_ID) {
        if final_job.display_name != FINAL_DISPLAY_NAME {
            return Err(RenderError::InvalidWorkflow("bad_final_name".to_owned()));
        }
        if final_job.condition.as_deref() != Some(FINAL_CONDITION) {
            return Err(RenderError::InvalidWorkflow(
                "bad_final_condition".to_owned(),
            ));
        }
    }
    if let Some(lint) = jobs.get(LINT_JOB_ID)
        && lint.display_name != LINT_DISPLAY_NAME
    {
        return Err(RenderError::InvalidWorkflow("bad_lint_name".to_owned()));
    }
    Ok(())
}
