//! Qualified interpreter and Buildx identities for native APT delivery.

use velnor_actions_workflow_renderer::{RenderError, delivery_tools::DeliveryToolContext};

/// Validated generation identity for the complete native APT graph.
#[derive(Debug, Clone)]
pub struct AptWorkflowContext {
    /// Marker version shared by every generated source.
    pub generator_version: String,
    /// Exact qualified runner label.
    pub runs_on: String,
}

impl AptWorkflowContext {
    /// Validate marker and runner identities.
    /// # Errors
    /// Rejects unqualified identities before graph construction.
    pub fn validate(&self) -> Result<(), RenderError> {
        velnor_actions_workflow_renderer::marker::validate_version(&self.generator_version)?;
        velnor_actions_workflow_renderer::guard::validate_runs_on(&self.runs_on)
    }
}

/// Generator-resolved tools; credentials never enter rendering inputs.
#[derive(Debug, Clone)]
pub struct AptRenderContext {
    /// Marker version and qualified Linux runner image family.
    pub workflow: AptWorkflowContext,
    /// Catalog-pinned Mise, Python, and GitHub CLI.
    pub tools: DeliveryToolContext,
    /// Exact official `docker/setup-buildx-action` SHA reference.
    pub buildx_action: String,
    /// Qualified exact Buildx binary release.
    pub buildx_version: String,
    /// Qualified immutable BuildKit image used by the setup action.
    pub buildkit_image: String,
}

impl AptRenderContext {
    /// Reject unqualified tool selectors before emitting executable steps.
    /// # Errors
    /// Returns an error for malformed workflow or tool identities.
    pub fn validate(&self) -> Result<(), RenderError> {
        self.workflow.validate()?;
        self.tools.validate()?;
        velnor_actions_workflow_renderer::steps::validate_uses(&self.buildx_action)?;
        if !self
            .buildx_action
            .starts_with("docker/setup-buildx-action@")
        {
            return Err(RenderError::InvalidWorkflow(
                "apt_wrong_buildx_action".to_owned(),
            ));
        }
        let Some(version) = self.buildx_version.strip_prefix('v') else {
            return Err(RenderError::InvalidWorkflow(
                "apt_unpinned_buildx".to_owned(),
            ));
        };
        if version.split('.').count() != 3
            || !version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(RenderError::InvalidWorkflow(
                "apt_unpinned_buildx".to_owned(),
            ));
        }
        let Some(digest) = self.buildkit_image.strip_prefix("moby/buildkit@sha256:") else {
            return Err(RenderError::InvalidWorkflow(
                "apt_unpinned_buildkit".to_owned(),
            ));
        };
        if !velnor_actions_contract::ids::is_lower_hex_len(digest, 64) {
            return Err(RenderError::InvalidWorkflow(
                "apt_unpinned_buildkit".to_owned(),
            ));
        }
        Ok(())
    }
}
