//! Prevent cache access before the plan has authenticated dispatch context.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

pub(crate) const DISPATCH_DENY: &str = "github.event_name != 'workflow_dispatch'";

/// Suppress every cache path until a validated directive controls its use.
///
/// Raw dispatch metadata cannot grant cache access.
/// Only qualification cache actions with typed owners and complete directive checks can use the cache.
/// Workflow dispatch disables all other cache paths.
pub(crate) fn suppress_unvalidated_cache_access(jobs: &mut BTreeMap<String, Job>) {
    for job in jobs.values_mut() {
        for step in &mut job.steps {
            if is_mise_setup_cache(step) {
                disable_mise_cache(step);
            } else if is_tool_seed(step) {
                suppress_tool_seed(step);
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
    with.insert("cache".to_owned(), "false".to_owned());
    with.insert("cache_save".to_owned(), "false".to_owned());
}

fn is_cache_access(step: &Step) -> bool {
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    uses.starts_with("actions/cache@")
        || uses.starts_with("actions/cache/")
        // The install-only MBX action (`backend: local`) touches no cache.
        || (uses.starts_with("jdx/mr-boxington-action@")
            && with.get("backend").map(String::as_str) != Some("local"))
}

fn is_tool_seed(step: &Step) -> bool {
    step.role == Some(velnor_actions_contract::StepRole::ToolSeed)
        && matches!(&step.kind, StepKind::Action { uses, .. } if uses == crate::tool_seed::TOOL_SEED_USES)
}

fn suppress_tool_seed(step: &mut Step) {
    step.condition = None;
    let StepKind::Action { with, .. } = &mut step.kind else {
        return;
    };
    let Some(key) = with.get("cache_key").cloned() else {
        return;
    };
    if crate::tool_seed::is_guarded_seed_key(&key) {
        return;
    }
    let guarded = if crate::cache_p08::is_v2_cache_key_expression(&key) {
        crate::tool_seed::guarded_seed_key(&key)
    } else {
        String::new()
    };
    with.insert("cache_key".to_owned(), guarded);
}

fn suppress_dispatch(step: &mut Step) {
    if is_push_only_cache_save(step) {
        return;
    }
    if is_bound_qualification_cache_action(step) {
        return;
    }
    if step.role == Some(velnor_actions_contract::StepRole::ToolsCacheRestore)
        && matches!(
            step.condition.as_deref(),
            Some(
                crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION
                    | crate::cache_p08::TOOLS_CACHE_PLAN_RESTORE_CONDITION
            )
        )
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

/// Return true only for cache actions whose typed owner and directive checks match the expected values.
fn is_bound_qualification_cache_action(step: &Step) -> bool {
    use velnor_actions_contract::workflow::StepId;
    use velnor_actions_contract::{StepRole, workflow::QualificationCacheStepStage};

    let Some(StepId::QualificationCache(identity)) = step.id else {
        return false;
    };
    let restore = match (step.role, identity.stage()) {
        (Some(StepRole::QualificationCacheRestore), QualificationCacheStepStage::Restore) => true,
        (Some(StepRole::QualificationCacheSave), QualificationCacheStepStage::Save) => false,
        _ => return false,
    };
    let stage_prefix = if restore {
        "qualification-cache-restore-"
    } else {
        "qualification-cache-save-"
    };
    let Some(owner_tag) = identity
        .as_string()
        .strip_prefix(stage_prefix)
        .map(str::to_owned)
    else {
        return false;
    };
    let bind_id = format!("qualification-cache-bind-{owner_tag}");
    let uses = format!(
        "{}@{}",
        if restore {
            "actions/cache/restore"
        } else {
            "actions/cache/save"
        },
        velnor_actions_actionlint::actions::CACHE_ACTION_SHA
    );
    let mut inputs = BTreeMap::from([
        (
            "key".to_owned(),
            format!(
                "${{{{ steps.{bind_id}.outputs.{} }}}}",
                if restore { "restore_key" } else { "save_key" }
            ),
        ),
        (
            "path".to_owned(),
            format!(
                "${{{{ steps.{bind_id}.outputs.{} }}}}",
                if restore {
                    "restore_stage_path"
                } else {
                    "save_stage_path"
                }
            ),
        ),
    ]);
    if restore {
        inputs.insert("restore-keys".to_owned(), String::new());
    }
    let expected_condition = if restore {
        format!(
            "needs.plan.outputs.qualification_campaign != '' && steps.{bind_id}.outputs.active == 'true'"
        )
    } else {
        let export_id = format!("qualification-cache-export-{owner_tag}");
        format!(
            "needs.plan.outputs.qualification_campaign != '' && steps.{bind_id}.outputs.active == 'true' && steps.{export_id}.outputs.save_requested == 'true' && steps.{bind_id}.outputs.save_key != ''"
        )
    };
    matches!(
        &step.kind,
        StepKind::Action { uses: actual_uses, with, env }
            if actual_uses == &uses
                && with == &inputs
                && env.is_empty()
                && !step.parallel
                && step.condition.as_deref() == Some(expected_condition.as_str())
    )
}

/// Preserve the contract's canonical trust-gated writer gates.
///
/// These exact conditions already exclude workflow dispatch (the runtime
/// key step grants writes only to protected pushes and same-repository
/// pull requests). Conjoining the
/// general dispatch deny would change the typed `ToFu` save protocol while
/// adding no further protection.
fn is_push_only_cache_save(step: &Step) -> bool {
    let condition = step.condition.as_deref();
    let tools_save_readonly = crate::cache_p08::tools_cache_save_condition(
        velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    );
    let tools_save_pr_scoped = crate::cache_p08::tools_cache_save_condition(
        velnor_actions_contract::PullRequestCachePolicy::SameRepositoryScoped,
    );
    let condition_is_push_only = condition
        == Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION)
        || condition
            == Some(velnor_actions_contract::workflow::cache_family::SOURCES_SAVE_CONDITION)
        || condition
            == Some(velnor_actions_contract::workflow::cache_family::TOFU_PROVIDERS_SAVE_CONDITION)
        || condition
            == Some(velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_SAVE_CONDITION)
        || (step.role == Some(velnor_actions_contract::StepRole::ToolsCacheSave)
            && (condition == Some(tools_save_readonly.as_str())
                || condition == Some(tools_save_pr_scoped.as_str())));
    condition_is_push_only
        && matches!(&step.kind, StepKind::Action { uses, .. }
            if uses.starts_with("actions/cache/save@"))
}

#[cfg(test)]
#[path = "dispatch_cache_boundary_condition_tests.rs"]
mod condition_tests;
#[cfg(test)]
#[path = "dispatch_cache_boundary_tests.rs"]
mod tests;
