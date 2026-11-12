// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::WorkflowMetadata;
use crate::{OrchestratorError, internal::internal};

pub(super) fn admit_workflow(
    workflow: &WorkflowMetadata,
    path: &str,
    name: &str,
    pin: Option<&crate::package_release_public::HistoricalWorkflowPin>,
) -> Result<(), OrchestratorError> {
    let Some(pin) = pin else {
        return validate_workflow_metadata(workflow, path, name);
    };
    let observed = crate::package_release_public::ObservedWorkflow {
        id: workflow.id,
        path: &workflow.path,
        name: &workflow.name,
        state: &workflow.state,
    };
    if pin.admits(&observed, path, name) {
        Ok(())
    } else {
        Err(internal("package_workflow_identity_mismatch"))
    }
}

pub(super) fn validate_workflow_metadata(
    workflow: &WorkflowMetadata,
    path: &str,
    name: &str,
) -> Result<(), OrchestratorError> {
    if workflow.id == 0
        || workflow.path != path
        || workflow.name != name
        || workflow.state != "active"
    {
        return Err(internal("package_workflow_identity_mismatch"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "package_release_run_identity_tests.rs"]
mod tests;
