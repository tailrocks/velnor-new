use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole, WorkflowPolicy};

use crate::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
    steps,
};

type JobMap = BTreeMap<String, Job>;

#[derive(Clone, Copy)]
pub(super) enum ValidationPhase {
    BeforeSetup,
    Finalized,
}

impl ValidationPhase {
    fn admits_mise_count(self, count: usize) -> bool {
        match self {
            Self::BeforeSetup => count <= 1,
            Self::Finalized => count == 1,
        }
    }
}

pub(super) fn validate_required_verdict(
    jobs: &JobMap,
    phase: ValidationPhase,
    authority: &super::CanonicalAuthoritySteps,
) -> Result<(), RenderError> {
    let Some(job) = jobs.get(FINAL_JOB_ID) else {
        return Ok(());
    };
    let count = |role| {
        job.steps
            .iter()
            .filter(|step| step.role == Some(role))
            .count()
    };
    if count(StepRole::Checkout) != 1
        || !phase.admits_mise_count(count(StepRole::MiseSetup))
        || count(StepRole::AcquireVelnor) != 1
        || count(StepRole::PreparePinnedTools) != 1
    {
        return Err(RenderError::InvalidWorkflow(
            "required_verdict_candidate_execution".to_owned(),
        ));
    }
    if !authority.matches_required(&job.steps)
        || job.steps.iter().any(|step| !required_verdict_step(step))
    {
        return Err(RenderError::InvalidWorkflow(
            "required_verdict_candidate_execution".to_owned(),
        ));
    }
    Ok(())
}

fn required_verdict_step(step: &Step) -> bool {
    match &step.kind {
        StepKind::Action { uses, with, .. } => match step.role {
            Some(StepRole::Checkout) => uses.starts_with("actions/checkout@"),
            Some(StepRole::MiseSetup) => {
                uses.starts_with("jdx/mise-action@")
                    && ["install", "env", "cache", "cache_save"]
                        .iter()
                        .all(|key| with.get(*key).is_some_and(|value| value == "false"))
            }
            Some(StepRole::DownloadPlan | StepRole::AttestationDownload) => {
                uses == crate::steps::DOWNLOAD_ARTIFACT_USES
            }
            Some(StepRole::PublishFinal) => uses == crate::steps::UPLOAD_ARTIFACT_USES,
            _ => false,
        },
        StepKind::Shell { .. } => matches!(
            step.role,
            Some(StepRole::AcquireVelnor | StepRole::PreparePinnedTools)
        ),
        StepKind::Internal { operation, .. } => {
            operation == steps::MERGE_OPERATION
                || operation == steps::FETCH_OPERATION
                || operation
                    == &format!(
                        "{}:{}",
                        steps::WRITE_REQUEST_OPERATION,
                        steps::MERGE_OPERATION
                    )
        }
        StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. } => false,
    }
}

pub(super) fn validate_trusted_plan(
    jobs: &JobMap,
    policy: WorkflowPolicy,
    phase: ValidationPhase,
    authority: &super::CanonicalAuthoritySteps,
) -> Result<(), RenderError> {
    if policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(());
    }
    let Some(plan) = jobs.get(PLAN_JOB_ID) else {
        return Ok(());
    };
    let plan_indices = plan
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| matches_plan_operation(step).then_some(index))
        .collect::<Vec<_>>();
    if plan_indices.len() != 1 {
        return Err(RenderError::InvalidWorkflow(
            "trusted_plan_operation_count".to_owned(),
        ));
    }
    let prefix = &plan.steps[..=plan_indices[0]];
    if !authority.matches_producer_plan(prefix) {
        return Err(RenderError::InvalidWorkflow(
            "trusted_plan_candidate_execution".to_owned(),
        ));
    }
    validate_plan_prefix(prefix, phase)
}

fn matches_plan_operation(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Internal { operation, .. } if operation == steps::PLAN_OPERATION)
}

fn validate_plan_prefix(prefix: &[Step], phase: ValidationPhase) -> Result<(), RenderError> {
    if !has_plan_bootstrap(prefix, phase) {
        return Err(RenderError::InvalidWorkflow(
            "trusted_plan_bootstrap_missing".to_owned(),
        ));
    }
    let request = format!(
        "{}:{}",
        steps::WRITE_REQUEST_OPERATION,
        steps::PLAN_OPERATION
    );
    if !prefix.iter().any(
        |step| matches!(&step.kind, StepKind::Internal { operation, .. } if operation == &request),
    ) {
        return Err(RenderError::InvalidWorkflow(
            "trusted_plan_request_missing".to_owned(),
        ));
    }
    if prefix.iter().any(|step| !trusted_plan_step(step, &request)) {
        return Err(RenderError::InvalidWorkflow(
            "trusted_plan_candidate_execution".to_owned(),
        ));
    }
    Ok(())
}

fn has_plan_bootstrap(prefix: &[Step], phase: ValidationPhase) -> bool {
    let count = |role| prefix.iter().filter(|step| step.role == Some(role)).count();
    let Some(acquire) = prefix
        .iter()
        .position(|step| step.role == Some(StepRole::AcquireVelnor))
    else {
        return false;
    };
    count(StepRole::Checkout) == 1
        && phase.admits_mise_count(count(StepRole::MiseSetup))
        && count(StepRole::AcquireVelnor) == 1
        && prefix[..acquire]
            .iter()
            .all(|step| matches!(step.role, Some(StepRole::Checkout | StepRole::MiseSetup)))
}

fn trusted_plan_step(step: &Step, request: &str) -> bool {
    match &step.kind {
        StepKind::Action { uses, with, .. } => match step.role {
            Some(StepRole::Checkout) => uses == steps::CHECKOUT_USES,
            Some(StepRole::MiseSetup) => {
                uses == &format!(
                    "jdx/mise-action@{}",
                    velnor_actions_actionlint::actions::MISE_ACTION_SHA
                ) && ["install", "env", "cache", "cache_save"]
                    .iter()
                    .all(|key| with.get(*key).is_some_and(|value| value == "false"))
            }
            Some(StepRole::CargoSourcesRestore) => {
                uses == &format!(
                    "actions/cache/restore@{}",
                    velnor_actions_actionlint::actions::CACHE_ACTION_SHA
                )
            }
            _ => false,
        },
        StepKind::Shell { .. } => matches!(
            step.role,
            Some(
                StepRole::AcquireVelnor
                    | StepRole::PreparePinnedTools
                    | StepRole::PrepareRustComponents
                    | StepRole::VerifyRustManifest
                    | StepRole::VerifyJactionlint
                    | StepRole::CargoSourcesFetch
                    | StepRole::CacheKeys
            )
        ),
        StepKind::Internal { operation, .. } => {
            operation == request
                || operation == steps::PLAN_OPERATION
                || operation == steps::RESOLVE_QUALIFICATION_OPERATION
        }
        StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. } => false,
    }
}
