//! Runner-specific V2 tools-cache steps kept outside shared lane actions.

use velnor_actions_contract::{RunsOn, Step, StepKind, StepRole};

/// Peel runtime identity and restore steps before the common lane prefix.
pub(super) fn peel_tools_cache_prelude(
    steps: &[Step],
    runs_on: &str,
) -> Option<(Vec<Step>, Vec<Step>)> {
    let mut prelude = Vec::new();
    let mut common = Vec::new();
    for step in steps {
        if is_tools_cache_prelude_step(step) {
            prelude.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    if !valid_tools_cache_prelude(&prelude, runs_on) {
        return None;
    }
    if let Some(setup) = steps
        .iter()
        .position(|step| step.role == Some(StepRole::MiseSetup))
        && steps
            .iter()
            .enumerate()
            .any(|(index, step)| is_tools_cache_prelude_step(step) && index > setup)
    {
        return None;
    }
    Some((prelude, common))
}

fn is_tools_cache_prelude_step(step: &Step) -> bool {
    matches!(
        step.role,
        Some(StepRole::ToolsCacheIdentity | StepRole::ToolsCacheRestore)
    )
}

fn valid_tools_cache_prelude(steps: &[Step], runs_on: &str) -> bool {
    match steps {
        [] => true,
        [identity, restore] => {
            let identity_ok = identity.role == Some(StepRole::ToolsCacheIdentity)
                && identity.condition.is_none()
                && matches!(
                    &identity.kind,
                    StepKind::Action { uses, with, env }
                        if crate::cache_p08::validate_runtime_identity_action(
                            uses,
                            runs_on,
                            with,
                            env,
                        )
                        .is_ok()
                );
            let expected_paths = crate::cache_steps::TOOLS_CACHE_PATHS.join("\n");
            let restore_ok = matches!(
                &restore.kind,
                StepKind::Action { uses, with, env }
                    if restore.role == Some(StepRole::ToolsCacheRestore)
                        && uses == crate::cache_steps::TOOLS_RESTORE_USES
                        && env.is_empty()
                        && with.get("path").map(String::as_str) == Some(expected_paths.as_str())
                        && with.get("key").is_some_and(|key| tool_cache_key_digest(key).is_some())
            );
            identity_ok
                && restore_ok
                && restore.condition.as_deref()
                    == Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
        }
        _ => false,
    }
}

/// Ensure both runners use the same exact V2 payload shape while retaining lane identity.
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
    hosted.iter().zip(local).all(|(hosted, local)| {
        if hosted.role != local.role || hosted.id != local.id || hosted.condition != local.condition
        {
            return false;
        }
        match (&hosted.kind, &local.kind) {
            (
                StepKind::Shell {
                    run: hosted_run,
                    env: hosted_env,
                },
                StepKind::Shell {
                    run: local_run,
                    env: local_env,
                },
            ) => hosted_run == local_run && hosted_env.keys().eq(local_env.keys()),
            (
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
                if hosted.role == Some(StepRole::ToolsCacheIdentity) {
                    Some(hosted_uses.as_str())
                        == crate::cache_p08::runtime_identity_action_uses(hosted_runs_on)
                        && Some(local_uses.as_str())
                            == crate::cache_p08::runtime_identity_action_uses(local_runs_on)
                        && hosted_env == local_env
                        && hosted_with.keys().eq(local_with.keys())
                } else {
                    hosted_uses == local_uses
                        && hosted_env == local_env
                        && same_cache_inputs(hosted_with, local_with)
                }
            }
            _ => false,
        }
    })
}

fn tool_cache_key_digest(key: &str) -> Option<&str> {
    let suffix = "${{steps.v2.outputs.identity}}";
    let body = key.strip_prefix("mise-tools-v2-")?.strip_suffix(suffix)?;
    let digest = body.strip_suffix('-')?;
    (digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
    .then_some(digest)
}

fn same_cache_inputs(
    hosted: &std::collections::BTreeMap<String, String>,
    local: &std::collections::BTreeMap<String, String>,
) -> bool {
    let (Some(hosted_key), Some(local_key)) = (hosted.get("key"), local.get("key")) else {
        return false;
    };
    if tool_cache_key_digest(hosted_key).is_none() || tool_cache_key_digest(local_key).is_none() {
        return false;
    }
    let mut hosted_shape = hosted.clone();
    let mut local_shape = local.clone();
    let normalized = "mise-tools-v2-{static-identity}-${{steps.v2.outputs.identity}}";
    hosted_shape.insert("key".to_owned(), normalized.to_owned());
    local_shape.insert("key".to_owned(), normalized.to_owned());
    hosted_shape == local_shape
}
