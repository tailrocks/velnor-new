use velnor_actions_contract::workflow::lanes::{
    NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV,
};
use velnor_actions_contract::{Step, StepKind, StepRole};

/// Find the point where the lane-specific MBX cache prelude begins.
pub(crate) fn mbx_prelude_index(steps: &[Step]) -> Option<usize> {
    let preflight = steps
        .iter()
        .position(|step| step.role == Some(StepRole::MbxPreflight))?;
    (preflight < steps.len()).then_some(preflight)
}

/// Separate an MBX prelude from the remaining shared lane body.
pub(crate) fn peel_mbx_prelude(steps: &[Step]) -> Option<(Vec<Step>, &[Step])> {
    let mut end = 0;
    for step in steps {
        if is_mbx_prelude_step(step) {
            end += 1;
        } else {
            break;
        }
    }
    let prelude = &steps[..end];
    if prelude.first()?.role != Some(StepRole::MbxPreflight)
        || !prelude.iter().any(crate::cache_steps::is_mbx_action)
    {
        return None;
    }
    Some((prelude.to_vec(), &steps[end..]))
}

/// Keep the provider output owner in the job scope that consumes its outputs.
pub(crate) fn peel_provider_restore_prefix(steps: &[Step]) -> (Vec<Step>, &[Step]) {
    let Some(index) = steps
        .iter()
        .position(|step| step.role == Some(StepRole::TofuProvidersRestore))
    else {
        return (Vec::new(), steps);
    };
    let end = index + 1;
    (steps[..end].to_vec(), &steps[end..])
}

/// Separate writer and MBX export steps from the common lane action.
pub(crate) fn peel_postlude(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
    let mut common = Vec::new();
    let mut postlude = Vec::new();
    for step in steps {
        if is_postlude_step(step) {
            postlude.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    (common, postlude)
}

fn is_mbx_prelude_step(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::MbxPreflight | StepRole::MbxCache | StepRole::MbxVersionCheck)
    )
}

fn is_postlude_step(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::ToolsCacheSave | StepRole::TofuProvidersSave)
    )
}

/// Pull named-check and artifact steps out of the shared composite body.
pub(crate) fn peel_lane_specific(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
    let mut common = Vec::new();
    let mut extra = Vec::new();
    for step in steps {
        if is_lane_specific(step) {
            extra.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    (common, extra)
}

fn is_lane_specific(step: &Step) -> bool {
    if is_elected_save(step) {
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

fn is_elected_save(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::ToolsCacheSave | StepRole::TofuProvidersSave)
    ) || step.name == crate::cache_steps::TOOLS_SAVE_NAME
        || step.name == crate::tofu_cache::TOFU_PROVIDERS_SAVE_NAME
}
