//! Conditions and steps that remain owned by each named check lane.

use velnor_actions_contract::Job;
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};

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
