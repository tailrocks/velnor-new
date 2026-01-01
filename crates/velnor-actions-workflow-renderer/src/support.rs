//! Policy gating and Velnor support-job construction.
//!
//! Consumer policy rejects support IR; Velnor policy merges one typed job
//! per repository validator plus optional candidate jobs (P05-6: no Policy
//! umbrella). No toolchain-qualification job exists. The always-on
//! `actionlint` job arrives via typed IR and is emitted for both policies;
//! it is never support IR.

// Token-hygiene gate lives beside the policy gates (`#[path]`, no `lib.rs` edit).
#[path = "support_tokens.rs"]
mod tokens;

pub(crate) use tokens::check_token_hygiene;

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    Job, JobTimeout, Step, StepKind, ValidatorKind, VelnorSupportWorkflow,
};

use crate::{
    RenderError,
    candidate::{candidate_job, release_job},
    render::{
        ALINT_BINARY_VERSION, ALINT_USES, CANDIDATE_JOB_ID, FINAL_CONDITION, FINAL_DISPLAY_NAME,
        FINAL_JOB_ID, PLAN_JOB_ID, RenderContext, ValidatorCommand,
    },
    steps,
};

/// Always-on workflow-lint job ID, emitted for both policies.
pub(crate) const LINT_JOB_ID: &str = "actionlint";

/// Display name of the always-on workflow-lint job.
pub(crate) const LINT_DISPLAY_NAME: &str = "Actionlint";

/// Protected release job ID (Velnor policy, candidate mode only).
pub(crate) const RELEASE_JOB_ID: &str = "release";

/// Display name of the protected release job.
pub(crate) const RELEASE_DISPLAY_NAME: &str = "Release";

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
        && (!workflow.validators.is_empty() || workflow.candidate_validation)
    {
        return Err(RenderError::PolicyRejected {
            policy: "consumer-v1".to_owned(),
            problem: "support_ir_rejected".to_owned(),
        });
    }
    for validator in ValidatorKind::repository_validators() {
        let id = validator.job_id();
        if jobs.contains_key(id) {
            return Err(RenderError::PolicyRejected {
                policy: "consumer-v1".to_owned(),
                problem: format!("forbidden_job:{id}"),
            });
        }
    }
    for id in [CANDIDATE_JOB_ID, RELEASE_JOB_ID] {
        if jobs.contains_key(id) {
            return Err(RenderError::PolicyRejected {
                policy: "consumer-v1".to_owned(),
                problem: format!("forbidden_job:{id}"),
            });
        }
    }
    Ok(())
}

/// Velnor policy: merge one typed job per validator plus candidates.
/// The exhaustive `ValidatorKind` match is the whole support set, with no
/// toolchain-qualification job and no umbrella grouping.
pub(crate) fn merge_support_jobs(
    jobs: &mut BTreeMap<String, Job>,
    support: Option<&VelnorSupportWorkflow>,
    ctx: &RenderContext,
) -> Result<(), RenderError> {
    let Some(workflow) = support else {
        return Ok(());
    };
    let mut seen = BTreeSet::new();
    for validator in &workflow.validators {
        if *validator == ValidatorKind::Actionlint {
            return Err(RenderError::PolicyRejected {
                policy: "velnor-repository-v1".to_owned(),
                problem: "actionlint_not_support".to_owned(),
            });
        }
        if !seen.insert(*validator) {
            return Err(RenderError::PolicyRejected {
                policy: "velnor-repository-v1".to_owned(),
                problem: "duplicate_validator".to_owned(),
            });
        }
    }
    for validator in &workflow.validators {
        let job = match validator {
            ValidatorKind::Alint => alint_job(ctx)?,
            ValidatorKind::Actionlint => {
                return Err(RenderError::PolicyRejected {
                    policy: "velnor-repository-v1".to_owned(),
                    problem: "actionlint_not_support".to_owned(),
                });
            }
            ValidatorKind::CargoDeny | ValidatorKind::CargoMachete | ValidatorKind::Zizmor => {
                validator_job(ctx, *validator)?
            }
        };
        insert_support_job(jobs, validator.job_id(), job)?;
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
    extend_final_needs(jobs, &workflow.validators, workflow.candidate_validation);
    Ok(())
}

/// Extend the final gate with merged support IDs (workflow-contract §4).
///
/// `Required` needs plan + every crate job + lint always, plus each merged
/// validator and candidate when those policy jobs exist. The release job
/// never gates: it publishes after merge on protected refs only.
fn extend_final_needs(
    jobs: &mut BTreeMap<String, Job>,
    validators: &[ValidatorKind],
    candidate: bool,
) {
    let mut extra = Vec::new();
    for validator in validators {
        let id = validator.job_id();
        if jobs.contains_key(id) {
            extra.push(id.to_owned());
        }
    }
    if candidate && jobs.contains_key(CANDIDATE_JOB_ID) {
        extra.push(CANDIDATE_JOB_ID.to_owned());
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

/// Fixed Alint job: checkout plus the full-SHA Alint action.
pub(crate) fn alint_job(ctx: &RenderContext) -> Result<Job, RenderError> {
    let checkout = steps::checkout_step(&ctx.checkout_uses)?;
    let mut with = BTreeMap::new();
    with.insert("path".to_owned(), ".".to_owned());
    with.insert("config".to_owned(), ".alint.yml".to_owned());
    with.insert("format".to_owned(), "github".to_owned());
    with.insert("fail-on-warning".to_owned(), "true".to_owned());
    with.insert("version".to_owned(), ALINT_BINARY_VERSION.to_owned());
    steps::scan_for_private_subcommands(ALINT_USES)?;
    Ok(Job {
        display_name: ValidatorKind::Alint.display_name().to_owned(),
        runs_on: ctx.runs_on.clone(),
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            checkout,
            Step {
                name: "Run Alint".to_owned(),
                condition: None,
                kind: StepKind::Action {
                    uses: ALINT_USES.to_owned(),
                    with,
                    env: BTreeMap::new(),
                },
            },
        ],
    })
}

/// Fixed validator job: checkout plus its caller-supplied command.
///
/// The validator runs its pinned analyzer with ambient auth: the cold
/// tool bootstrap needs authenticated quota (the scrub overlay broke
/// `ubi:` installs with API 401s and zizmor with empty-token aborts,
/// CI run 36815180228). Static analyzers execute no repository code;
/// cargo-backed validators (deny) run from the cargo isolation dir
/// with an absolute manifest path, so repo `.cargo/config.toml`
/// providers never execute while ambient auth is in scope. The step
/// carries no scrub keys at all.
pub(crate) fn validator_job(
    ctx: &RenderContext,
    validator: ValidatorKind,
) -> Result<Job, RenderError> {
    let command = find_validator_command(&ctx.validator_commands, validator)?;
    Ok(Job {
        display_name: validator.display_name().to_owned(),
        runs_on: ctx.runs_on.clone(),
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            steps::checkout_step(&ctx.checkout_uses)?,
            steps::ambient_shell_step(&command.name, command.argv.clone(), BTreeMap::new())?,
        ],
    })
}

/// Exactly one command per validator; missing or duplicated fails closed.
fn find_validator_command(
    commands: &[ValidatorCommand],
    validator: ValidatorKind,
) -> Result<&ValidatorCommand, RenderError> {
    let mut found = None;
    for command in commands {
        if command.validator == validator {
            if found.is_some() {
                return Err(RenderError::InvalidWorkflow(format!(
                    "duplicate_validator_command:{}",
                    validator.job_id()
                )));
            }
            found = Some(command);
        }
    }
    found.ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("validator_without_command:{}", validator.job_id()))
    })
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
    for (id, job) in jobs {
        if id.as_str() != FINAL_JOB_ID
            && id.as_str() != RELEASE_JOB_ID
            && job.needs.contains(&CANDIDATE_JOB_ID.to_owned())
        {
            return Err(RenderError::InvalidWorkflow(
                "task_must_not_consume_candidate".to_owned(),
            ));
        }
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
