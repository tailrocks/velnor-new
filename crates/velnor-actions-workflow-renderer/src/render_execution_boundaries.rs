// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole, WorkflowPolicy};

use crate::{
    RenderError,
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
    steps,
};

#[path = "render_required_verdict_step.rs"]
mod required_verdict;
use required_verdict::required_verdict_step;

type JobMap = BTreeMap<String, Job>;

pub(super) fn validate_preview_acquisition(
    jobs: &JobMap,
    policy: WorkflowPolicy,
    controller: &super::ControllerLocator,
    non_inputs: &velnor_actions_contract::NonInputs,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    let dispatch = matches!(
        controller,
        super::ControllerLocator::ProtectedBaseDispatch { .. }
    );
    let approved = dispatch && policy == WorkflowPolicy::VelnorRepositoryV1;
    if approved {
        reject_paired_native_checks(jobs)?;
    }
    let canonical = approved
        .then(crate::steps::protected_preview_acquire_step)
        .transpose()?;
    let canonical_tip = approved
        .then(crate::state_seal::tip::stale_tip_stops_job_step)
        .transpose()?;
    let canonical_guard = (approved && !non_inputs.is_empty())
        .then(|| crate::non_input_checkout::guard_step(non_inputs))
        .transpose()?;
    for (id, job) in jobs {
        let mut acquired = false;
        for (index, step) in job.steps.iter().enumerate() {
            if approved && let StepKind::Internal { operation, .. } = &step.kind {
                let (operation, _) = steps::split_internal_operation(operation)?;
                if !supported_preview_operation(operation) {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "preview_operation_unsupported:{id}:{operation}"
                    )));
                }
            }
            if matches!(&step.kind, StepKind::Action { uses, .. }
                if uses == "./.github/actions/velnor-preview-setup")
                && step.role != Some(StepRole::AcquireProtectedPreview)
            {
                return Err(RenderError::InvalidWorkflow(format!(
                    "untyped_preview_acquisition:{id}"
                )));
            }
            if step.role == Some(StepRole::AcquireProtectedPreview) {
                if acquired
                    || canonical.as_ref() != Some(step)
                    || job.steps[..index]
                        .iter()
                        .enumerate()
                        .any(|(position, prior)| {
                            !allowed_preview_prelude(
                                prior,
                                position.checked_sub(1).map(|previous| &job.steps[previous]),
                                canonical_tip.as_ref(),
                                canonical_guard.as_ref(),
                                non_inputs,
                                checkout_uses,
                            )
                        })
                {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "unapproved_preview_acquisition:{id}"
                    )));
                }
                acquired = true;
            } else if approved && step.role == Some(StepRole::AcquireVelnor) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "mixed_preview_acquisition:{id}"
                )));
            } else if approved
                && matches!(
                    step.kind,
                    StepKind::Internal { .. }
                        | StepKind::TaskExecution { .. }
                        | StepKind::NativeCheckExecution { .. }
                )
                && !acquired
            {
                return Err(RenderError::InvalidWorkflow(format!(
                    "preview_invocation_before_acquisition:{id}"
                )));
            }
        }
    }
    Ok(())
}

fn reject_paired_native_checks(jobs: &JobMap) -> Result<(), RenderError> {
    let mut owners = BTreeMap::<&str, &str>::new();
    for (job_id, job) in jobs {
        for step in &job.steps {
            if let StepKind::NativeCheckExecution { task_id, .. } = &step.kind
                && owners
                    .insert(task_id, job_id)
                    .is_some_and(|prior| prior != job_id)
            {
                return Err(RenderError::InvalidWorkflow(
                    "preview_native_paired_check_unsupported".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn allowed_preview_prelude(
    prior: &Step,
    previous: Option<&Step>,
    canonical_tip: Option<&Step>,
    canonical_guard: Option<&Step>,
    non_inputs: &velnor_actions_contract::NonInputs,
    checkout_uses: &str,
) -> bool {
    if canonical_tip == Some(prior) {
        return true;
    }
    if canonical_guard == Some(prior)
        && let Some(checkout) = previous
        && velnor_actions_contract::workflow::step_identity::is_configured_checkout(
            checkout,
            checkout_uses,
        )
        && let StepKind::Action { with, .. } = &checkout.kind
        && with.get(crate::non_input_checkout::SPARSE_CHECKOUT_KEY)
            == Some(&non_inputs.sparse_checkout_patterns().join("\n"))
        && with
            .get(crate::non_input_checkout::SPARSE_CONE_MODE_KEY)
            .is_some_and(|value| value == "false")
    {
        return true;
    }
    matches!(
        prior.role,
        Some(
            StepRole::Checkout
                | StepRole::MiseSetup
                | StepRole::DownloadPlan
                | StepRole::AttestationDownload
        )
    )
}

fn supported_preview_operation(operation: &str) -> bool {
    matches!(
        operation,
        "plan-v1"
            | "merge-v1"
            | "fetch-reports-v1"
            | "resolve-qualification-v1"
            | "write-request-v1"
            | "assemble-group-ledgers-v1"
            | "emit-group-ledger-v1"
            | "execute-check-v1"
            | "execute-native-homebrew-check-v1"
            | "resolve-task-execution-v1"
            | "write-task-report-v1"
    )
}

pub(super) fn validate_raw_authority(
    jobs: &JobMap,
    policy: WorkflowPolicy,
    authority: &super::CanonicalAuthoritySteps,
) -> Result<(), RenderError> {
    if !authority.matches_raw_required(jobs) {
        return Err(RenderError::InvalidWorkflow(
            "required_verdict_candidate_execution".to_owned(),
        ));
    }
    if policy == WorkflowPolicy::VelnorRepositoryV1 && !authority.matches_raw_producer_plan(jobs) {
        return Err(RenderError::InvalidWorkflow(
            "trusted_plan_candidate_execution".to_owned(),
        ));
    }
    Ok(())
}

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
        || count(if authority.preview_dispatch_enabled() {
            StepRole::AcquireProtectedPreview
        } else {
            StepRole::AcquireVelnor
        }) != 1
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
    validate_plan_prefix(prefix, phase, authority.preview_dispatch_enabled())
}

fn matches_plan_operation(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Internal { operation, .. } if operation == steps::PLAN_OPERATION)
}

fn validate_plan_prefix(
    prefix: &[Step],
    phase: ValidationPhase,
    preview: bool,
) -> Result<(), RenderError> {
    if !has_plan_bootstrap(prefix, phase, preview) {
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

fn has_plan_bootstrap(prefix: &[Step], phase: ValidationPhase, preview: bool) -> bool {
    let count = |role| prefix.iter().filter(|step| step.role == Some(role)).count();
    let acquire_role = if preview {
        StepRole::AcquireProtectedPreview
    } else {
        StepRole::AcquireVelnor
    };
    let Some(acquire) = prefix
        .iter()
        .position(|step| step.role == Some(acquire_role))
    else {
        return false;
    };
    count(StepRole::Checkout) == 1
        && phase.admits_mise_count(count(StepRole::MiseSetup))
        && count(acquire_role) == 1
        && prefix[..acquire]
            .iter()
            .all(|step| matches!(step.role, Some(StepRole::Checkout | StepRole::MiseSetup)))
}

fn trusted_plan_step(step: &Step, request: &str) -> bool {
    match &step.kind {
        StepKind::Action { uses, with, .. } => match step.role {
            Some(StepRole::Checkout) => uses == steps::CHECKOUT_USES,
            Some(StepRole::AcquireProtectedPreview) => {
                crate::steps::protected_preview_acquire_step()
                    .is_ok_and(|approved| approved == *step)
            }
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
                    | StepRole::StateSelect
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
