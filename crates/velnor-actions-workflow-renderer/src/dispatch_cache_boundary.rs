// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

pub(crate) const DISPATCH_DENY: &str = "github.event_name != 'workflow_dispatch'";

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
    if step.role == Some(velnor_actions_contract::StepRole::ToolsCacheRestore)
        && step.condition.as_deref() == Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
    {
        return;
    }
    if step.condition.as_deref() == Some(DISPATCH_DENY) {
        return;
    }
    let Some(prior) = step.condition.take() else {
        step.condition = Some(DISPATCH_DENY.to_owned());
        return;
    };
    if prior == "success()" {
        step.condition = Some(DISPATCH_DENY.to_owned());
        return;
    }
    step.condition = Some(format!("({prior}) && {DISPATCH_DENY}"));
}

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
        || (step.role == Some(velnor_actions_contract::StepRole::FamilySave)
            && velnor_actions_contract::workflow::cache_family_steps::STACK_FAMILIES
                .into_iter()
                .any(|family| {
                    condition
                        == Some(
                            velnor_actions_contract::workflow::cache_family_steps::family_save_condition(
                                family,
                            )
                            .as_str(),
                        )
                }))
        || (step.role == Some(velnor_actions_contract::StepRole::ToolsCacheSave)
            && (condition == Some(tools_save_readonly.as_str())
                || condition == Some(tools_save_pr_scoped.as_str())));
    condition_is_push_only
        && matches!(&step.kind, StepKind::Action { uses, .. }
            if uses.starts_with("actions/cache/save@"))
}
