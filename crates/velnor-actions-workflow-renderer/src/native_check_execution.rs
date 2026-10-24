//! Fixed SDK dispatch for closed native Homebrew checks.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, document_steps::shell_step_to_yaml, yaml::Yaml};

/// Render one authenticated staged-SDK invocation with plan-bound identity.
pub(super) fn render(step: &Step, job_env: &BTreeMap<String, String>) -> Result<Yaml, RenderError> {
    let StepKind::NativeCheckExecution {
        check_id,
        operation,
        task_id,
        sdk_version,
    } = &step.kind
    else {
        return Err(RenderError::InvalidWorkflow(
            "native_check_step_kind_mismatch".to_owned(),
        ));
    };
    if sdk_version != env!("CARGO_PKG_VERSION") {
        return Err(RenderError::InvalidWorkflow(
            "native_check_sdk_version_mismatch".to_owned(),
        ));
    }

    if job_env
        .get("GH_TOKEN")
        .is_some_and(|value| !value.is_empty())
    {
        return Err(RenderError::InvalidWorkflow(
            "native_check_job_token_inheritance_rejected".to_owned(),
        ));
    }
    let env = BTreeMap::from([
        (
            crate::steps::INTERNAL_OP_ENV.to_owned(),
            "execute-native-homebrew-check-v1".to_owned(),
        ),
        ("VELNOR_CHECK_ID".to_owned(), check_id.clone()),
        (
            "VELNOR_NATIVE_CHECK_OPERATION".to_owned(),
            operation.as_str().to_owned(),
        ),
        ("VELNOR_TASK_ID".to_owned(), task_id.clone()),
    ]);
    let binary = format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{sdk_version}");
    let mut rendered = shell_step_to_yaml(step, &[binary], &env, job_env, false, None)?;
    insert_scoped_github_token(&mut rendered)?;
    Ok(rendered)
}

fn insert_scoped_github_token(rendered: &mut Yaml) -> Result<(), RenderError> {
    let Yaml::Map(entries) = rendered else {
        return Err(RenderError::InvalidWorkflow(
            "native_check_step_render_shape_invalid".to_owned(),
        ));
    };
    if let Some((_, value)) = entries.iter_mut().find(|(key, _)| key == "env") {
        let Yaml::Map(env) = value else {
            return Err(RenderError::InvalidWorkflow(
                "native_check_step_env_shape_invalid".to_owned(),
            ));
        };
        if env.iter().any(|(key, _)| key == "GH_TOKEN") {
            return Err(RenderError::InvalidWorkflow(
                "native_check_step_token_already_present".to_owned(),
            ));
        }
        env.push((
            "GH_TOKEN".to_owned(),
            Yaml::str("${{ github.token }}".to_owned()),
        ));
        env.sort_by(|left, right| left.0.cmp(&right.0));
    } else {
        entries.push((
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}".to_owned()),
            )]),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_check_execution_tests.rs"]
mod tests;
