//! Marked neutral workflow documents without CI graph mutation.
use crate::{RenderError, pages_approval::NativePagesApproval, render::RenderContext};
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, StepKind, WorkflowIr};

/// Generation-only bindings supplied by compiled native workflow owners.
#[derive(Debug, Clone)]
pub struct WorkflowDocumentContext {
    /// Generator version for the managed file marker.
    pub generator_version: String,
    /// Exact emitted source, invocation and environment records.
    pub source_helpers: Vec<CompiledSourceHelper>,
    /// Exact native Pages authority from the approved factory.
    pub native_pages_approvals: Vec<NativePagesApproval>,
    /// Complete approved public attestation graph from its compiled owner.
    pub native_publish_approvals: Vec<crate::native_publish_approval::NativePublishApproval>,
    /// Exact qualified `DockerHub` named-secret login authority.
    pub action_credential_approvals: Vec<crate::action_credentials::ActionCredentialApproval>,
}

/// Render an already complete neutral graph without inserting CI operations.
/// # Errors
/// Rejects invalid IR, unknown helper authority, unapproved Pages roles and raw commands.
pub fn render_workflow_document(
    ir: &WorkflowIr,
    ctx: &WorkflowDocumentContext,
) -> Result<String, RenderError> {
    ir.validate().map_err(RenderError::Contract)?;
    crate::marker::validate_version(&ctx.generator_version)?;
    crate::source_helper::validate_registry(&ctx.source_helpers, &ctx.generator_version)?;
    crate::support::check_native_token_hygiene(
        ir,
        &ctx.source_helpers,
        &ctx.action_credential_approvals,
    )?;
    if ir.jobs.values().flat_map(|job| &job.steps).any(|step| {
        matches!(
            step.kind,
            StepKind::Shell { .. } | StepKind::Internal { .. }
        )
    }) {
        return Err(RenderError::InvalidWorkflow(
            "native_workflow_requires_closed_steps".to_owned(),
        ));
    }
    let context = RenderContext {
        generator_version: ctx.generator_version.clone(),
        runs_on: String::new(),
        staged_binary: String::new(),
        request_dir: String::new(),
        checkout_uses: String::new(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        plan_consumer_env: BTreeMap::new(),
        source_helpers: ctx.source_helpers.clone(),
        native_pages_approvals: ctx.native_pages_approvals.clone(),
        native_publish_approvals: ctx.native_publish_approvals.clone(),
    };
    let document = crate::document::workflow_to_yaml(ir, &ir.jobs, &context, false)?;
    let document = crate::yaml::quote_run_values_in_yaml(document);
    let text =
        crate::marker::with_marker(&ctx.generator_version, &crate::yaml::render_yaml(&document))?;
    crate::steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

#[cfg(test)]
#[path = "workflow_document_tests.rs"]
mod tests;
