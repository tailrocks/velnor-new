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
    shell_step_to_yaml(step, &[binary], &env, job_env, false, None)
}
