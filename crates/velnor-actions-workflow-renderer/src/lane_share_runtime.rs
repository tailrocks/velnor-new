//! Runner-specific V2 tools-cache steps kept outside shared lane actions.

use velnor_actions_contract::{RunsOn, Step, StepKind, StepRole};

/// Peel typed runtime identity, seed, and restore steps before lane sharing.
pub(super) fn peel_tools_cache_prelude(
    steps: &[Step],
    runs_on: &str,
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
    if !valid_tools_cache_prelude(&prelude, runs_on) {
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

fn valid_tools_cache_prelude(steps: &[Step], runs_on: &str) -> bool {
    match steps {
        [] => true,
        [identity, restore] => valid_identity(identity, runs_on) && valid_restore(restore),
        _ => false,
    }
}

fn valid_identity(step: &Step, runs_on: &str) -> bool {
    step.role == Some(StepRole::ToolsCacheIdentity)
        && step.condition.is_none()
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

fn valid_restore(step: &Step) -> bool {
    step.role == Some(StepRole::ToolsCacheRestore)
        && step.condition.as_deref() == Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
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
) -> bool {
    if !valid_tools_cache_prelude(hosted, hosted_runs_on)
        || !valid_tools_cache_prelude(local, local_runs_on)
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
    if hosted.len() != local.len() {
        return false;
    }
    hosted
        .iter()
        .zip(local)
        .all(|(hosted, local)| same_tools_step(hosted, local, hosted_runs_on, local_runs_on))
}

fn same_tools_step(hosted: &Step, local: &Step, hosted_runs_on: &str, local_runs_on: &str) -> bool {
    if hosted.role != local.role || hosted.condition != local.condition {
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
