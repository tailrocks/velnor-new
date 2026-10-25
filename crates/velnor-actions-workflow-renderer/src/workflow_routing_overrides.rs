use std::collections::BTreeSet;

use crate::{RenderError, RenderedFile};

use super::{WorkflowRoutingPolicy, inventory, native_preview};

/// Producer-supplied routing exceptions for exact generated workflow paths.
///
/// These classifications affect placement inspection only. The renderer still
/// parses every workflow and validates the complete cross-file call inventory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowRoutingOverrides {
    hosted_only_paths: BTreeSet<String>,
}

impl WorkflowRoutingOverrides {
    /// Mark one generated workflow as intentionally hosted-only.
    pub fn mark_hosted_only(&mut self, workflow_path: impl Into<String>) {
        self.hosted_only_paths.insert(workflow_path.into());
    }

    pub(super) fn is_hosted_only(&self, workflow_path: &str) -> bool {
        self.hosted_only_paths.contains(workflow_path)
    }

    /// Iterate explicitly classified paths in sorted order.
    pub fn hosted_only_paths(&self) -> impl Iterator<Item = &str> {
        self.hosted_only_paths.iter().map(String::as_str)
    }
}

/// Examine every job runner and reusable-workflow call in the final workflow files.
///
/// # Errors
///
/// The function gives an error for incorrect workflow data, unsupported Scale Set placement,
/// or a reusable-workflow call whose target is missing, does not accept calls, or creates a
/// call cycle.
pub fn validate_generated_workflow_calls(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
) -> Result<(), RenderError> {
    validate_generated_workflow_calls_with_overrides(
        files,
        policy,
        &WorkflowRoutingOverrides::default(),
    )
}

/// Examine every generated workflow with producer-supplied path classifications.
///
/// # Errors
///
/// Returns an error when a workflow, placement override, or reusable-workflow call fails validation.
pub fn validate_generated_workflow_calls_with_overrides(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
    overrides: &WorkflowRoutingOverrides,
) -> Result<(), RenderError> {
    if !policy.native_hosted_tasks.is_empty() {
        return Err(super::invalid(
            "native_hosted_tasks_require_typed_workflow".to_owned(),
        ));
    }
    inventory::validate_final_workflow_inventory(files, policy, overrides, None, None)
}

/// Examine final files against the typed workflow used to render them.
///
/// Typed native checks and declared macOS task jobs need this workflow intermediate representation
/// to prove their runner contract. The public file-only audit rejects those jobs because it has no
/// typed source proof.
///
/// # Errors
///
/// Returns an error when the workflow intermediate representation or generated workflows fail
/// validation.
pub fn validate_generated_workflow_calls_with_ir(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
) -> Result<(), RenderError> {
    validate_generated_workflow_calls_with_ir_and_overrides(
        files,
        policy,
        workflow,
        &WorkflowRoutingOverrides::default(),
    )
}

/// Examine final files against typed workflow source and producer classifications.
///
/// # Errors
///
/// Returns an error when source binding, routing metadata, or generated workflows fail validation.
pub fn validate_generated_workflow_calls_with_ir_and_overrides(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
    overrides: &WorkflowRoutingOverrides,
) -> Result<(), RenderError> {
    workflow
        .validate()
        .map_err(|error| super::invalid(format!("typed_workflow_invalid:{error}")))?;
    policy.validate_native_task_source(workflow)?;
    inventory::validate_final_workflow_inventory(
        files,
        policy,
        overrides,
        Some(&workflow.jobs),
        None,
    )
}

/// Validate generated workflows when this tree includes the authenticated native preview graph.
///
/// This keeps exact source-derived preview runner expressions behind a typed entry point; the
/// general file and IR validators continue to reject unresolved runners.
///
/// # Errors
///
/// Returns an error if the typed workflow or the generated workflow files fail validation.
pub fn validate_generated_workflow_calls_with_native_preview_ir(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
) -> Result<(), RenderError> {
    validate_generated_workflow_calls_with_native_preview_ir_and_overrides(
        files,
        policy,
        workflow,
        &WorkflowRoutingOverrides::default(),
    )
}

/// Validate a native-preview tree against typed workflow source and classifications.
///
/// # Errors
///
/// Returns an error when source binding, routing metadata, or native-preview files fail validation.
pub fn validate_generated_workflow_calls_with_native_preview_ir_and_overrides(
    files: &[RenderedFile],
    policy: &WorkflowRoutingPolicy,
    workflow: &velnor_actions_contract::WorkflowIr,
    overrides: &WorkflowRoutingOverrides,
) -> Result<(), RenderError> {
    workflow
        .validate()
        .map_err(|error| super::invalid(format!("typed_workflow_invalid:{error}")))?;
    policy.validate_native_task_source(workflow)?;
    inventory::validate_final_workflow_inventory(
        files,
        policy,
        overrides,
        Some(&workflow.jobs),
        Some(native_preview::Context),
    )
}
