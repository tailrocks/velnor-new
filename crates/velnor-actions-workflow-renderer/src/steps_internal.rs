//! Internal-operation step templates (split from `steps`).
//!
//! Operation travels via env, never argv; the staged binary dispatches
//! on it at event time.

use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::{
    RenderError,
    steps::{
        FETCH_OPERATION, MERGE_OPERATION, PLAN_OPERATION, PUBLISH_OPERATION,
        RESOLVE_QUALIFICATION_OPERATION, WRITE_REQUEST_OPERATION, scan_for_private_subcommands,
    },
};

const QUALIFICATION_CACHE_OPERATIONS: &[&str] = &[
    "observe-qualification-cache-runtime-v1",
    "observe-qualification-cache-roots-v1",
    "bind-qualification-cache-v1",
    "observe-qualification-cache-backend-v1",
    "import-qualification-cache-payload-v1",
    "export-qualification-cache-payload-v1",
    "write-qualification-cache-receipt-v1",
];

pub(crate) fn is_qualification_cache_operation(operation: &str) -> bool {
    QUALIFICATION_CACHE_OPERATIONS.contains(&operation)
}

pub(crate) fn validate_qualification_cache_env(
    operation: &str,
    step_env: &std::collections::BTreeMap<String, String>,
) -> Result<(), RenderError> {
    for (key, value) in step_env {
        if key == "GH_TOKEN" {
            if operation != "observe-qualification-cache-backend-v1"
                || value != "${{ github.token }}"
            {
                return Err(RenderError::InvalidWorkflow(
                    "qualification_cache_token_scope_invalid".to_owned(),
                ));
            }
        } else if key != "VELNOR_QUALIFICATION_RUNTIME_IDENTITY_FILE"
            && !key.starts_with("VELNOR_QUALIFICATION_CACHE_")
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "qualification_cache_env_not_supported:{key}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "document_steps_qualification_cache_tests.rs"]
mod qualification_cache_tests;

/// Split an internal operation into env op plus request-file target op.
///
/// `plan-v1`/`merge-v1`/`fetch-reports-v1`/`publish-baseline-v1` target
/// themselves (fetch takes no request file; its input root is the
/// runner-temp velnor directory); `write-request-v1:<target>` gates on
/// `write-request-v1` while materializing the target's request file.
/// # Errors
pub(crate) fn split_internal_operation(operation: &str) -> Result<(&str, &str), RenderError> {
    if is_qualification_cache_operation(operation) {
        return Ok((operation, operation));
    }
    if operation == RESOLVE_QUALIFICATION_OPERATION {
        return Ok((operation, PLAN_OPERATION));
    }
    if operation == PLAN_OPERATION
        || operation == MERGE_OPERATION
        || operation == FETCH_OPERATION
        || operation == PUBLISH_OPERATION
        || operation == velnor_actions_contract::PRIVATE_FETCH_OPERATION
        || operation == velnor_actions_contract::PRIVATE_REVOKE_OPERATION
    {
        return Ok((operation, operation));
    }
    let rest = operation
        .strip_prefix(WRITE_REQUEST_OPERATION)
        .and_then(|rest| rest.strip_prefix(':'));
    if let Some(target) = rest
        && (target == PLAN_OPERATION || target == MERGE_OPERATION || target == PUBLISH_OPERATION)
    {
        return Ok((WRITE_REQUEST_OPERATION, target));
    }
    Err(RenderError::BadCommand(format!(
        "unknown_internal_op:{operation}"
    )))
}

/// Internal plan/merge/write-request step; operation travels via env, never argv.
/// # Errors
pub fn internal_step(name: &str, operation: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadCommand("empty_name".to_owned()));
    }
    crate::expressions::check_name_content(name)?;
    split_internal_operation(operation)?;
    scan_for_private_subcommands(name)?;
    Ok(Step {
        parallel: false,
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Internal {
            operation: operation.to_owned(),
            env: std::collections::BTreeMap::new(),
        },
    })
}

/// Fixed write-request step materializing `<target>-request.json` at event time.
/// # Errors
pub fn write_request_step(target: &str) -> Result<Step, RenderError> {
    if target != PLAN_OPERATION && target != MERGE_OPERATION && target != PUBLISH_OPERATION {
        return Err(RenderError::BadCommand(format!(
            "unknown_internal_op:{target}"
        )));
    }
    Ok(Step {
        parallel: false,
        name: "Write request".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Internal {
            operation: format!("{WRITE_REQUEST_OPERATION}:{target}"),
            env: std::collections::BTreeMap::new(),
        },
    })
}

/// Fixed planner step (`plan-v1`).
#[must_use]
pub fn plan_step() -> Step {
    Step {
        parallel: false,
        name: "Plan".to_owned(),
        id: Some(StepId::Plan),
        role: Some(StepRole::PlanProducer),
        condition: None,
        kind: StepKind::Internal {
            operation: PLAN_OPERATION.to_owned(),
            env: std::collections::BTreeMap::new(),
        },
    }
}

/// Fixed report-merge step (`merge-v1`).
#[must_use]
pub fn merge_step() -> Step {
    Step {
        parallel: false,
        name: "Merge reports".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Internal {
            operation: MERGE_OPERATION.to_owned(),
            env: std::collections::BTreeMap::new(),
        },
    }
}

/// Create the fixed baseline-publisher step.
#[must_use]
pub fn publish_step() -> Step {
    Step {
        parallel: false,
        name: "Publish baseline".to_owned(),
        id: Some(StepId::PublishBaseline),
        role: Some(StepRole::BaselinePublisher),
        condition: None,
        kind: StepKind::Internal {
            operation: PUBLISH_OPERATION.to_owned(),
            env: std::collections::BTreeMap::new(),
        },
    }
}

#[cfg(test)]
mod qualification_cache_operation_tests {
    use super::split_internal_operation;

    #[test]
    fn qualification_cache_operations_do_not_take_request_files() {
        for operation in [
            "observe-qualification-cache-runtime-v1",
            "observe-qualification-cache-roots-v1",
            "bind-qualification-cache-v1",
            "observe-qualification-cache-backend-v1",
            "import-qualification-cache-payload-v1",
            "export-qualification-cache-payload-v1",
            "write-qualification-cache-receipt-v1",
        ] {
            assert_eq!(
                split_internal_operation(operation).expect("fixed operation"),
                (operation, operation)
            );
        }
        assert!(split_internal_operation("write-qualification-cache-receipt-v2").is_err());
    }
}
