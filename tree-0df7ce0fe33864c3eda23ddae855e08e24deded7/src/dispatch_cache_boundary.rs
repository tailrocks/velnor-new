//! Prevent cache access before the plan has authenticated dispatch context.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

pub(crate) const DISPATCH_DENY: &str = "github.event_name != 'workflow_dispatch'";

/// Suppress every cache path until a validated directive controls its use.
///
/// Raw dispatch metadata can deny cache access, but cannot grant it. The
/// current renderer has no cache producer that admits its selected backend
/// object against a validated directive before import, so every cache path
/// stays disabled on workflow dispatch, including downstream jobs.
pub(crate) fn suppress_unvalidated_cache_access(jobs: &mut BTreeMap<String, Job>) {
    for job in jobs.values_mut() {
        for step in &mut job.steps {
            if is_mise_setup_cache(step) {
                disable_mise_cache(step);
            } else if is_cache_access(step) {
                suppress_dispatch(step);
            }
        }
    }
}

fn is_mise_setup_cache(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, with, .. }
        if uses.starts_with("jdx/mise-action@")
            && with.get("cache").is_some_and(|value| value == "true"))
}

fn disable_mise_cache(step: &mut Step) {
    let StepKind::Action { with, .. } = &mut step.kind else {
        return;
    };
    with.insert(
        "cache".to_owned(),
        "${{ github.event_name != 'workflow_dispatch' && 'true' || 'false' }}".to_owned(),
    );
}

fn is_cache_access(step: &Step) -> bool {
    let StepKind::Action { uses, .. } = &step.kind else {
        return false;
    };
    uses.starts_with("actions/cache@")
        || uses.starts_with("actions/cache/")
        || uses.starts_with("jdx/mr-boxington-action@")
        || uses == crate::tool_seed::TOOL_SEED_USES
}

fn suppress_dispatch(step: &mut Step) {
    if step.name == crate::cache_steps::TOOLS_RESTORE_NAME
        && step.condition.as_deref() == Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
    {
        return;
    }
    if step.condition.as_deref() == Some(DISPATCH_DENY) {
        return;
    }
    let Some(prior) = step.condition.take() else {
        // GitHub applies its implicit success() check when an if expression
        // does not contain a status-check function. Keep the deny predicate
        // short for the common no-condition cache step.
        step.condition = Some(DISPATCH_DENY.to_owned());
        return;
    };
    if prior == "success()" {
        step.condition = Some(DISPATCH_DENY.to_owned());
        return;
    }
    step.condition = Some(format!("({prior}) && {DISPATCH_DENY}"));
}

#[cfg(test)]
#[path = "dispatch_cache_boundary_tests.rs"]
mod tests;
