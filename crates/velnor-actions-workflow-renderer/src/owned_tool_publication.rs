//! Generator-only, credential-free staging of approved source distributions.
//!
//! This sibling renderer deliberately emits candidate builds only. Signing and
//! publication require independently qualified downloaded artifacts.

use std::collections::BTreeSet;

use crate::{RenderError, RenderedFile, marker, render_yaml, steps};

#[path = "owned_tool_publication_yaml.rs"]
mod document;
#[path = "owned_tool_qualification_yaml.rs"]
mod qualification;

/// Closed source qualification event categories; neither grants publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceQualificationTrigger {
    /// Dispatch an infrastructure workflow already present on the default branch.
    DefaultBranchDispatch,
    /// Bootstrap only on the independently reviewed infrastructure branch.
    ReviewedInfrastructurePush,
}

/// Dedicated infrastructure branch; no caller-provided glob or ref is admitted.
pub const REVIEWED_INFRASTRUCTURE_BRANCH: &str = "owned-tool-candidates";

/// Opaque source data admitted by its owning adapter; renderer owns no recipe.
#[derive(Debug, Clone)]
pub struct SourceBuildBinding {
    /// Stable artifact/job label.
    pub label: String,
    /// Canonical serialized source approval from the owning adapter.
    pub source_json: String,
}

impl SourceBuildBinding {
    /// Validate only transport shape, never tool source or recipe semantics.
    /// # Errors
    /// Rejects unsafe labels and malformed or oversized JSON bindings.
    pub fn validate(&self) -> Result<(), RenderError> {
        if self.label.is_empty()
            || self.label.len() > 64
            || !self
                .label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || self.source_json.len() > 65536
            || self.source_json.contains("${{")
        {
            return Err(RenderError::InvalidWorkflow(
                "source_build_binding".to_owned(),
            ));
        }
        velnor_actions_contract::parse_strict_json(&self.source_json)
            .map_err(RenderError::Contract)?;
        Ok(())
    }
}

/// Explicit upstream bootstrap byte authority, per native host.
#[derive(Debug, Clone)]
pub struct OwnedBuildBootstrap {
    /// Native target triple.
    pub target: String,
    /// Versioned native hosted runner label.
    pub runner: String,
    /// Opaque canonical bootstrap asset authority admitted by its adapter.
    pub assets_json: String,
}

/// Typed generator infrastructure; consumers never receive this workflow.
#[derive(Debug, Clone)]
pub struct OwnedPublicationSpec {
    /// Closed source qualification trigger; no dispatch source/command inputs.
    pub trigger: SourceQualificationTrigger,
    /// Marker version.
    pub generator_version: String,
    /// Exact checkout action SHA.
    pub checkout_uses: String,
    /// Reviewed source literals, never arbitrary dispatch source/command input.
    pub sources: Vec<SourceBuildBinding>,
    /// Complete native bootstrap host set.
    pub bootstraps: Vec<OwnedBuildBootstrap>,
}

impl OwnedPublicationSpec {
    /// Validate source approvals and complete native host identities.
    /// # Errors
    /// Refuses incomplete hosts, mutable bootstrap transport, and duplicates.
    pub fn validate(&self) -> Result<(), RenderError> {
        marker::validate_version(&self.generator_version)?;
        steps::validate_uses(&self.checkout_uses)?;
        let hosts: BTreeSet<_> = self
            .bootstraps
            .iter()
            .map(|item| item.target.as_str())
            .collect();
        let expected = BTreeSet::from([
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin",
        ]);
        let tools: BTreeSet<_> = self
            .sources
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        if !self.checkout_uses.starts_with("actions/checkout@")
            || self.sources.is_empty()
            || tools.len() != self.sources.len()
            || self.bootstraps.len() != 3
            || hosts != expected
        {
            return Err(RenderError::InvalidWorkflow(
                "owned_build_host_set".to_owned(),
            ));
        }
        for source in &self.sources {
            source.validate()?;
        }
        for bootstrap in &self.bootstraps {
            let supported = matches!(
                (bootstrap.target.as_str(), bootstrap.runner.as_str()),
                ("x86_64-unknown-linux-gnu", "ubuntu-26.04")
                    | ("aarch64-unknown-linux-gnu", "ubuntu-24.04-arm")
                    | ("aarch64-apple-darwin", "macos-26")
            );
            if !supported
                || bootstrap.assets_json.len() > 65536
                || bootstrap.assets_json.contains("${{")
            {
                return Err(RenderError::InvalidWorkflow(
                    "owned_build_bootstrap".to_owned(),
                ));
            }
            velnor_actions_contract::parse_strict_json(&bootstrap.assets_json)
                .map_err(RenderError::Contract)?;
        }
        Ok(())
    }
}

/// Render credential-free candidate staging from approved generation-time inputs.
/// # Errors
/// Fails before YAML emission when source or bootstrap authority is incomplete.
pub fn render_owned_publication_files(
    spec: &OwnedPublicationSpec,
) -> Result<Vec<RenderedFile>, RenderError> {
    spec.validate()?;
    let yaml = document::document(spec);
    Ok(vec![RenderedFile {
        path: ".github/workflows/owned-tools.yml".to_owned(),
        bytes: marker::with_marker(&spec.generator_version, &render_yaml(&yaml))?,
    }])
}

#[cfg(test)]
#[path = "owned_tool_publication_tests.rs"]
mod tests;
