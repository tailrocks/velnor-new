//! Runner-specific V2 tools-cache steps kept outside shared lane actions.

use velnor_actions_contract::{RunsOn, Step, StepKind, StepRole};

/// Keep helper acquisition and cache-tar setup before the steps shared by jobs.
pub(super) fn peel_cache_helper_prefix(steps: &[Step]) -> Option<(Vec<Step>, &[Step])> {
    let start = usize::from(steps.first().is_some_and(is_preseed_plan_failure_guard));
    let end = start
        + steps[start..]
            .iter()
            .take_while(|step| {
                matches!(
                    step.role,
                    Some(
                        StepRole::AcquireVelnor
                            | StepRole::PreseedDownload
                            | StepRole::PreseedVerifyManifest
                            | StepRole::PreseedStage
                            | StepRole::CacheTarLaunchers
                    )
                )
            })
            .count();
    let prefix = &steps[..end];
    let acquire = prefix
        .iter()
        .filter(|step| step.role == Some(StepRole::AcquireVelnor))
        .count();
    let preseed_roles = prefix
        .iter()
        .filter_map(|step| match step.role {
            Some(
                StepRole::PreseedDownload
                | StepRole::PreseedVerifyManifest
                | StepRole::PreseedStage,
            ) => step.role,
            _ => None,
        })
        .collect::<Vec<_>>();
    let launcher = prefix
        .iter()
        .position(|step| step.role == Some(StepRole::CacheTarLaunchers));
    let preseed_order = [
        StepRole::PreseedDownload,
        StepRole::PreseedVerifyManifest,
        StepRole::PreseedStage,
    ];
    let preseed_valid = if start > 0 {
        preseed_roles == preseed_order
    } else {
        preseed_roles.is_empty() || preseed_roles == preseed_order
    };
    let source_count = acquire + usize::from(!preseed_roles.is_empty());
    if acquire > 1
        || (acquire > 0 && !preseed_roles.is_empty())
        || !preseed_valid
        || launcher.is_some_and(|index| source_count != 1 || index == 0)
        || prefix
            .iter()
            .filter(|step| step.role == Some(StepRole::CacheTarLaunchers))
            .count()
            > 1
    {
        return None;
    }
    Some((prefix.to_vec(), &steps[end..]))
}

fn is_preseed_plan_failure_guard(step: &Step) -> bool {
    step.name == "Require successful pre-seed plan"
        && step.id.is_none()
        && step.role.is_none()
        && step.condition.as_deref() == Some("always() && needs.plan.result != 'success'")
        && matches!(
            &step.kind,
            StepKind::Shell { run, env }
                if run == &[
                    "sh".to_owned(),
                    "-c".to_owned(),
                    crate::toolchain_env::with_credential_unset_script("exit 1"),
                ] && env == &crate::toolchain_env::credential_scrub()
        )
}

/// If the Scale Set lane cannot access the cache, keep the launcher in the hosted job only.
pub(super) fn same_cache_helper_prefix_shape(
    hosted: &[Step],
    local: &[Step],
    hosted_runs_on: &str,
    local_runs_on: &str,
    hosted_has_cache_prelude: bool,
    local_has_cache_prelude: bool,
) -> bool {
    let hosted_guard = hosted
        .first()
        .filter(|step| is_preseed_plan_failure_guard(step));
    let local_guard = local
        .first()
        .filter(|step| is_preseed_plan_failure_guard(step));
    if hosted_guard != local_guard {
        return false;
    }
    let is_helper_source = |step: &&Step| {
        matches!(
            step.role,
            Some(
                StepRole::AcquireVelnor
                    | StepRole::PreseedDownload
                    | StepRole::PreseedVerifyManifest
                    | StepRole::PreseedStage
            )
        )
    };
    let hosted_stages = hosted
        .iter()
        .filter(is_helper_source)
        .cloned()
        .collect::<Vec<_>>();
    let local_stages = local
        .iter()
        .filter(is_helper_source)
        .cloned()
        .collect::<Vec<_>>();
    if hosted_stages != local_stages {
        return false;
    }
    let hosted_launchers = hosted
        .iter()
        .filter(|step| step.role == Some(StepRole::CacheTarLaunchers))
        .collect::<Vec<_>>();
    let local_launchers = local
        .iter()
        .filter(|step| step.role == Some(StepRole::CacheTarLaunchers))
        .collect::<Vec<_>>();
    if hosted_launchers == local_launchers {
        return true;
    }
    hosted_launchers.len() == 1
        && local_launchers.is_empty()
        && !local_has_cache_prelude
        && hosted_has_cache_prelude
        && matches!(RunsOn::parse(hosted_runs_on), Ok(RunsOn::Hosted(_)))
        && matches!(RunsOn::parse(local_runs_on), Ok(RunsOn::ScaleSet(_)))
}

/// Keep tools-cache identity and restore steps outside the steps shared by runner lanes.
pub(super) fn peel_tools_cache_prelude(
    steps: &[Step],
    runs_on: &str,
    needs_plan: bool,
) -> Option<(Vec<Step>, Vec<Step>)> {
    let mut prelude = Vec::new();
    let mut common = Vec::new();
    for step in steps {
        if is_tools_prelude_step(step.role) {
            prelude.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    if !valid_tools_cache_prelude(&prelude, runs_on, needs_plan) {
        return None;
    }
    let setup = steps
        .iter()
        .position(|step| step.role == Some(StepRole::MiseSetup));
    if setup.is_some_and(|setup| {
        steps
            .iter()
            .enumerate()
            .any(|(index, step)| is_tools_prelude_step(step.role) && index > setup)
    }) {
        return None;
    }
    Some((prelude, common))
}

fn is_tools_prelude_step(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(StepRole::ToolsCacheIdentity | StepRole::ToolSeed | StepRole::ToolsCacheRestore)
    )
}

fn valid_tools_cache_prelude(steps: &[Step], runs_on: &str, needs_plan: bool) -> bool {
    match steps {
        [] => true,
        [identity, restore] => {
            valid_identity(identity, runs_on, needs_plan) && valid_restore(restore, needs_plan)
        }
        _ => false,
    }
}

fn valid_identity(step: &Step, runs_on: &str, needs_plan: bool) -> bool {
    step.role == Some(StepRole::ToolsCacheIdentity)
        && step.condition.as_deref()
            == Some(crate::cache_p08::tools_cache_identity_condition(needs_plan))
        && matches!(
            &step.kind,
            StepKind::Action { uses, with, env }
                if crate::cache_p08::validate_runtime_identity_action(
                    step,
                    uses,
                    runs_on,
                    with,
                    env,
                )
                .is_ok()
        )
}

fn valid_restore(step: &Step, needs_plan: bool) -> bool {
    step.role == Some(StepRole::ToolsCacheRestore)
        && step.id == Some(velnor_actions_contract::StepId::ToolsRestore)
        && step.condition.as_deref()
            == Some(crate::cache_p08::tools_cache_restore_condition(needs_plan))
        && matches!(
            &step.kind,
            StepKind::Action { uses, with, env }
                if uses == crate::cache_steps::TOOLS_RESTORE_USES
                    && env.is_empty()
                    && with.len() == 2
                    && with.get("key").is_some_and(|key| is_tools_cache_key(key))
                    && with.get(crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT)
                        == Some(&crate::cache_steps::TOOLS_SEED_ADMITTED_EXPRESSION.to_owned())
        )
}

/// Ensure the lane-specific preludes have the same action and key structure.
pub(super) fn same_tools_cache_prelude_shape(
    hosted: &[Step],
    local: &[Step],
    hosted_runs_on: &str,
    local_runs_on: &str,
    hosted_needs_plan: bool,
    local_needs_plan: bool,
) -> bool {
    if !valid_tools_cache_prelude(hosted, hosted_runs_on, hosted_needs_plan)
        || !valid_tools_cache_prelude(local, local_runs_on, local_needs_plan)
    {
        return false;
    }
    if local.is_empty()
        && !hosted.is_empty()
        && matches!(RunsOn::parse(hosted_runs_on), Ok(RunsOn::Hosted(_)))
        && matches!(RunsOn::parse(local_runs_on), Ok(RunsOn::ScaleSet(_)))
    {
        return true;
    }
    if hosted_needs_plan != local_needs_plan || hosted.len() != local.len() {
        return false;
    }
    hosted
        .iter()
        .zip(local)
        .all(|(hosted, local)| same_tools_step(hosted, local, hosted_runs_on, local_runs_on))
}

fn same_tools_step(hosted: &Step, local: &Step, hosted_runs_on: &str, local_runs_on: &str) -> bool {
    if hosted.id != local.id || hosted.role != local.role || hosted.condition != local.condition {
        return false;
    }
    match (hosted.role, &hosted.kind, &local.kind) {
        (
            Some(StepRole::ToolsCacheIdentity),
            StepKind::Action {
                uses: hosted_uses,
                with: hosted_with,
                env: hosted_env,
            },
            StepKind::Action {
                uses: local_uses,
                with: local_with,
                env: local_env,
            },
        ) => {
            Some(hosted_uses.as_str())
                == crate::cache_p08::runtime_prelude_action_uses(hosted_runs_on)
                && Some(local_uses.as_str())
                    == crate::cache_p08::runtime_prelude_action_uses(local_runs_on)
                && hosted_env == local_env
                && hosted_with.keys().eq(local_with.keys())
        }
        (
            Some(StepRole::ToolsCacheRestore),
            StepKind::Action {
                uses: hosted_uses,
                with: hosted_with,
                env: hosted_env,
            },
            StepKind::Action {
                uses: local_uses,
                with: local_with,
                env: local_env,
            },
        ) => {
            hosted_uses == crate::cache_steps::TOOLS_RESTORE_USES
                && local_uses == crate::cache_steps::TOOLS_RESTORE_USES
                && hosted_env == local_env
                && same_keyed_inputs(hosted_with, local_with, "key")
                && same_seed_admission_input(
                    hosted_with,
                    local_with,
                    crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT,
                )
        }
        _ => false,
    }
}

fn is_tools_cache_key(key: &str) -> bool {
    crate::cache_p08::is_v2_cache_key_expression(key)
}

fn same_keyed_inputs(
    hosted: &std::collections::BTreeMap<String, String>,
    local: &std::collections::BTreeMap<String, String>,
    field: &str,
) -> bool {
    let (Some(hosted_key), Some(local_key)) = (hosted.get(field), local.get(field)) else {
        return false;
    };
    if !is_tools_cache_key(hosted_key) || !is_tools_cache_key(local_key) {
        return false;
    }
    let mut hosted_shape = hosted.clone();
    let mut local_shape = local.clone();
    let normalized = crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION;
    hosted_shape.insert(field.to_owned(), normalized.to_owned());
    local_shape.insert(field.to_owned(), normalized.to_owned());
    hosted_shape == local_shape
}

fn same_seed_admission_input(
    hosted: &std::collections::BTreeMap<String, String>,
    local: &std::collections::BTreeMap<String, String>,
    field: &str,
) -> bool {
    let expected = crate::cache_steps::TOOLS_SEED_ADMITTED_EXPRESSION;
    hosted.get(field).map(String::as_str) == Some(expected)
        && local.get(field).map(String::as_str) == Some(expected)
}

#[cfg(test)]
#[path = "lane_share_runtime_tests.rs"]
mod preseed_guard_tests;
