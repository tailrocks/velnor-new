//! Internal-operation step templates (split from `steps`).
//!
//! Operation travels via env, never argv; the staged binary dispatches
//! on it at event time.

use velnor_actions_contract_workflow::{Step, StepId, StepKind, StepRole};

use crate::{
    RenderError,
    steps::{
        ARTIFACT_EXPORT_OPERATION, FETCH_OPERATION, MERGE_OPERATION, PLAN_OPERATION,
        PUBLISH_OPERATION, VERIFICATION_ARTIFACT_EXPORT_OPERATION, WRITE_REQUEST_OPERATION,
        scan_for_private_subcommands,
    },
};

/// Split an internal operation into env op plus request-file target op.
///
/// `plan-v1`/`merge-v1`/`fetch-reports-v1`/`publish-baseline-v1` target
/// themselves (fetch takes no request file; its input root is the
/// runner-temp velnor directory); `write-request-v1:<target>` gates on
/// `write-request-v1` while materializing the target's request file.
/// # Errors
pub fn split_internal_operation(operation: &str) -> Result<(&str, &str), RenderError> {
    if operation == PLAN_OPERATION
        || operation == MERGE_OPERATION
        || operation == FETCH_OPERATION
        || operation == PUBLISH_OPERATION
        || operation == ARTIFACT_EXPORT_OPERATION
        || operation == VERIFICATION_ARTIFACT_EXPORT_OPERATION
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

/// Fixed baseline-publish step (`publish-baseline-v1`).
#[must_use]
pub fn publish_step() -> Step {
    Step {
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

/// Fixed typed artifact-export step. All values come from the plan-produced
/// matrix; the operation validates them against the downloaded plan before
/// it reads or stages any declared output.
#[must_use]
pub fn artifact_export_step() -> Step {
    use std::collections::BTreeMap;

    use velnor_actions_contract_workflow::{
        ARTIFACT_MISE_TASK_ENV, ARTIFACT_NAME_ENV, ARTIFACT_PLAN_DIGEST_ENV, ARTIFACT_PROVIDER_ENV,
        ARTIFACT_SOURCE_SHA_ENV, ARTIFACT_TASK_ID_ENV, StepKind,
    };

    Step {
        name: "Capture declared artifact outputs".to_owned(),
        id: None,
        role: Some(StepRole::ArtifactBuildExport),
        condition: None,
        kind: StepKind::Internal {
            operation: ARTIFACT_EXPORT_OPERATION.to_owned(),
            env: BTreeMap::from([
                (
                    ARTIFACT_TASK_ID_ENV.to_owned(),
                    "${{ matrix.task_id }}".to_owned(),
                ),
                (
                    ARTIFACT_MISE_TASK_ENV.to_owned(),
                    "${{ matrix.mise_task }}".to_owned(),
                ),
                (
                    ARTIFACT_PROVIDER_ENV.to_owned(),
                    "${{ matrix.provider }}".to_owned(),
                ),
                (
                    ARTIFACT_SOURCE_SHA_ENV.to_owned(),
                    "${{ matrix.source_sha }}".to_owned(),
                ),
                (
                    ARTIFACT_PLAN_DIGEST_ENV.to_owned(),
                    "${{ matrix.plan_digest }}".to_owned(),
                ),
                (
                    ARTIFACT_NAME_ENV.to_owned(),
                    "${{ matrix.artifact_name }}".to_owned(),
                ),
            ]),
        },
    }
}

/// Fixed exporter for outputs from an existing verification job.
#[must_use]
pub fn verification_artifact_export_step(task_id: &str) -> Step {
    use std::collections::BTreeMap;

    use velnor_actions_contract_workflow::ARTIFACT_TASK_ID_ENV;
    use velnor_actions_contract_workflow::{StepId, StepKind, StepRole};

    Step {
        name: "Capture declared verification outputs".to_owned(),
        id: Some(StepId::VerificationArtifactExport),
        role: Some(StepRole::VerificationArtifactExport),
        condition: None,
        kind: StepKind::Internal {
            operation: VERIFICATION_ARTIFACT_EXPORT_OPERATION.to_owned(),
            env: BTreeMap::from([(ARTIFACT_TASK_ID_ENV.to_owned(), task_id.to_owned())]),
        },
    }
}
