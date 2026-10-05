//! Internal-operation step templates (split from `steps`).
//!
//! Operation travels via env, never argv; the staged binary dispatches
//! on it at event time.

use velnor_actions_contract::{Step, StepKind};

use crate::{
    RenderError,
    steps::{
        FETCH_OPERATION, MERGE_OPERATION, PLAN_OPERATION, PUBLISH_OPERATION,
        WRITE_REQUEST_OPERATION, scan_for_private_subcommands,
    },
};

/// Split an internal operation into env op plus request-file target op.
///
/// `plan-v1`/`merge-v1`/`fetch-reports-v1`/`publish-baseline-v1` target
/// themselves (fetch takes no request file; its input root is the
/// runner-temp velnor directory); `write-request-v1:<target>` gates on
/// `write-request-v1` while materializing the target's request file.
/// # Errors
pub(crate) fn split_internal_operation(operation: &str) -> Result<(&str, &str), RenderError> {
    if operation == PLAN_OPERATION
        || operation == MERGE_OPERATION
        || operation == FETCH_OPERATION
        || operation == PUBLISH_OPERATION
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
        name: name.to_owned(),
        condition: None,
        kind: StepKind::Internal {
            operation: operation.to_owned(),
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
        name: "Write request".to_owned(),
        condition: None,
        kind: StepKind::Internal {
            operation: format!("{WRITE_REQUEST_OPERATION}:{target}"),
        },
    })
}

/// Fixed planner step (`plan-v1`).
#[must_use]
pub fn plan_step() -> Step {
    Step {
        name: "Plan".to_owned(),
        condition: None,
        kind: StepKind::Internal {
            operation: PLAN_OPERATION.to_owned(),
        },
    }
}

/// Fixed report-merge step (`merge-v1`).
#[must_use]
pub fn merge_step() -> Step {
    Step {
        name: "Merge reports".to_owned(),
        condition: None,
        kind: StepKind::Internal {
            operation: MERGE_OPERATION.to_owned(),
        },
    }
}

/// Fixed baseline-publish step (`publish-baseline-v1`).
#[must_use]
pub fn publish_step() -> Step {
    Step {
        name: "Publish baseline".to_owned(),
        condition: None,
        kind: StepKind::Internal {
            operation: PUBLISH_OPERATION.to_owned(),
        },
    }
}
