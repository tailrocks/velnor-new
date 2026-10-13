use velnor_actions_contract::{Step, StepRole};

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
