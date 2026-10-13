//! Conditions and steps that remain owned by each named check lane.

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract::workflow::lanes::{
    NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
};
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

pub(super) fn same_or_admitted_condition(hosted: &Job, local: &Job) -> bool {
    if hosted.condition == local.condition {
        return true;
    }
    hosted.condition.is_none()
        && local.condition.as_deref() == Some(EPHEMERAL_CHECK_ADMISSION_CONDITION)
        && matches!(
            (&hosted.check_runner, &local.check_runner),
            (Some(hosted_runner), Some(local_runner))
                if hosted_runner == local_runner
                    && hosted_runner.platform == CheckPlatform::LinuxX64
                    && hosted_runner.executor == CheckExecutor::Hosted
        )
}

pub(super) fn peel_lane_specific(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
    let mut common = Vec::new();
    let mut lane_specific = Vec::new();
    for step in steps {
        if is_lane_specific(step) {
            lane_specific.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    (common, lane_specific)
}

fn is_lane_specific(step: &Step) -> bool {
    if matches!(
        step.role,
        Some(StepRole::ToolsCacheSave | StepRole::TofuProvidersSave)
    ) {
        return true;
    }
    match &step.kind {
        StepKind::Action { uses, .. } if uses == crate::steps::UPLOAD_ARTIFACT_USES => true,
        StepKind::Shell { env, .. } | StepKind::Internal { env, .. } => {
            env.contains_key(NAMED_CHECK_JOB_ID_ENV)
                || env.contains_key(NAMED_CHECK_LANE_VARIANT_ENV)
        }
        StepKind::Action { .. } => false,
    }
}
