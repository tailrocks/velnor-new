//! Shared-lane cache outputs and policy derived from the shared key step.

use velnor_actions_contract::{PullRequestCachePolicy, Step, StepKind};

use super::{
    MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_KEY_NAME, MBX_BUNDLE_SAVE_NAME, SHARED_CACHE_KEY, identity,
    pr_cache,
};
use crate::RenderError;

/// Use the common key step's scope to match the lifecycle's effective policy.
pub(crate) fn shared_lane_policy(
    steps: &[Step],
    requested: PullRequestCachePolicy,
) -> Result<PullRequestCachePolicy, RenderError> {
    let mut keys = steps.iter().filter(|step| step.name == MBX_BUNDLE_KEY_NAME);
    let Some(key_step) = keys.next() else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_shared_key_step_missing".to_owned(),
        ));
    };
    if keys.next().is_some() {
        return Err(RenderError::InvalidWorkflow(
            "mbx_shared_key_step_ambiguous".to_owned(),
        ));
    }
    let StepKind::Shell { env, .. } = &key_step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_shared_key_step_not_shell".to_owned(),
        ));
    };
    let Some(scope) = env.get("MBX_CACHE_SCOPE") else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_shared_key_scope_missing".to_owned(),
        ));
    };
    Ok(identity::effective_policy(requested, scope))
}

/// Rebind elected saves to outputs exported by the shared composite call.
pub(crate) fn bind_shared_lane_outputs(
    steps: &mut [Step],
    policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    for step in steps {
        if step.name == MBX_BUNDLE_EXPORT_NAME {
            let condition = step.condition.as_mut().ok_or_else(|| {
                RenderError::InvalidWorkflow("mbx_shared_export_condition_missing".to_owned())
            })?;
            pr_cache::rebind_lane_condition(condition, policy)?;
        } else if step.name == MBX_BUNDLE_SAVE_NAME {
            let condition = step.condition.as_mut().ok_or_else(|| {
                RenderError::InvalidWorkflow("mbx_shared_save_condition_missing".to_owned())
            })?;
            pr_cache::rebind_lane_condition(condition, policy)?;
            let StepKind::Action { with, .. } = &mut step.kind else {
                return Err(RenderError::InvalidWorkflow(
                    "mbx_shared_save_not_action".to_owned(),
                ));
            };
            let Some(key) = with.get_mut("key") else {
                return Err(RenderError::InvalidWorkflow(
                    "mbx_shared_save_key_missing".to_owned(),
                ));
            };
            SHARED_CACHE_KEY.clone_into(key);
        }
    }
    Ok(())
}
