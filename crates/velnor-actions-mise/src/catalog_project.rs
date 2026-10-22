//! Project-declared tool versions (hard decision 5).
//!
//! The project's Mise config (`mise.toml` plus `mise.lock`, and the
//! `rust-toolchain.toml` channel Mise reads for Rust) is the version
//! authority for every tool it declares. Generated workflows never copy
//! such a version: an emission catalog ([`ToolCatalog::for_emission`])
//! renders it as the shell reference `${VELNOR_TOOL_<TOOL>}`, which the
//! generated resolver step fills at run time from the stored config value
//! (`velnor_actions_contract::config::project_tool_resolver`). Only tools
//! the project does not declare keep a Velnor pin in generated text.

use std::collections::BTreeSet;

pub use velnor_actions_contract::config::project_tool_refs::{
    has_project_tool_ref, is_project_tool_ref, project_tool_refs_to_expressions,
    without_project_tool_refs,
};
use velnor_actions_contract::config::project_tool_resolver::project_tool_keys;

use super::{PinnedTool, ToolCatalog};
use crate::error::MiseError;

/// Prefix of the run-time environment variable carrying one project version.
pub const PROJECT_TOOL_ENV_PREFIX: &str = "VELNOR_TOOL_";

impl PinnedTool {
    /// Run-time environment variable carrying the project-declared version.
    #[must_use]
    pub const fn project_env(self) -> &'static str {
        match self {
            Self::Rust => "VELNOR_TOOL_RUST",
            Self::MrBoxington => "VELNOR_TOOL_MR_BOXINGTON",
            Self::Gh => "VELNOR_TOOL_GH",
            Self::Jactionlint => "VELNOR_TOOL_JACTIONLINT",
            Self::Shellcheck => "VELNOR_TOOL_SHELLCHECK",
            Self::Zizmor => "VELNOR_TOOL_ZIZMOR",
            Self::Nextest => "VELNOR_TOOL_NEXTEST",
            Self::Opentofu => "VELNOR_TOOL_OPENTOFU",
            Self::ReleasePlz => "VELNOR_TOOL_RELEASE_PLZ",
            Self::Crane => "VELNOR_TOOL_CRANE",
            Self::Bun => "VELNOR_TOOL_BUN",
        }
    }

    /// Every `[tools]` key that declares this tool, canonical spelling first.
    #[must_use]
    pub fn mise_config_keys(self) -> &'static [&'static str] {
        project_tool_keys(self.project_env())
    }
}

/// Shell reference `${VELNOR_TOOL_<TOOL>}` used in `run:` text and task argv.
#[must_use]
pub fn project_tool_shell_ref(tool: PinnedTool) -> String {
    format!("${{{}}}", tool.project_env())
}

/// GitHub expression `${{ env.VELNOR_TOOL_<TOOL> }}` for `with:`, `env:`, and keys.
#[must_use]
pub fn project_tool_expression(tool: PinnedTool) -> String {
    format!("${{{{ env.{} }}}}", tool.project_env())
}

impl ToolCatalog {
    /// Record a project-declared tool with the exact version generation
    /// executes locally (the run-time resolver re-reads the project config).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidToolVersion`] for a non-exact version.
    pub fn with_project_tool(mut self, tool: PinnedTool, version: &str) -> Result<Self, MiseError> {
        super::validate_concrete_exact_version(tool.tool_name(), version)?;
        version.clone_into(self.version_slot(tool));
        self.project.insert(tool);
        Ok(self)
    }

    /// Project-declared tools, in stable order.
    #[must_use]
    pub const fn project_tools(&self) -> &BTreeSet<PinnedTool> {
        &self.project
    }

    /// Whether the project's Mise config declares `tool`.
    #[must_use]
    pub fn is_project_declared(&self, tool: PinnedTool) -> bool {
        self.project.contains(&tool)
    }

    /// Whether this catalog renders project versions as run-time references.
    #[must_use]
    pub const fn is_emission(&self) -> bool {
        self.emission
    }

    /// Catalog for generated text: every project-declared version becomes
    /// its `${VELNOR_TOOL_<TOOL>}` reference; Velnor pins stay literal.
    #[must_use]
    pub fn for_emission(&self) -> Self {
        let mut out = self.clone();
        for tool in &self.project {
            *out.version_slot(*tool) = project_tool_shell_ref(*tool);
        }
        out.emission = true;
        out
    }

    /// Replace every project reference in `text` with this catalog's
    /// concrete version (generation-time audits over emitted text).
    #[must_use]
    pub fn materialize(&self, text: &str) -> String {
        velnor_actions_contract::config::project_tool_refs::map_project_tool_refs(text, |name| {
            PinnedTool::ALL
                .iter()
                .find(|tool| tool.project_env() == name)
                .filter(|tool| !self.emission && self.is_project_declared(**tool))
                .map_or_else(
                    || format!("${{{name}}}"),
                    |tool| self.version(*tool).to_owned(),
                )
        })
    }

    fn version_slot(&mut self, tool: PinnedTool) -> &mut String {
        match tool {
            PinnedTool::Rust => &mut self.rust,
            PinnedTool::MrBoxington => &mut self.mr_boxington,
            PinnedTool::Gh => &mut self.gh,
            PinnedTool::Jactionlint => &mut self.jactionlint,
            PinnedTool::Shellcheck => &mut self.shellcheck,
            PinnedTool::Zizmor => &mut self.zizmor,
            PinnedTool::Nextest => &mut self.nextest,
            PinnedTool::Opentofu => &mut self.opentofu,
            PinnedTool::ReleasePlz => &mut self.release_plz,
            PinnedTool::Crane => &mut self.crane,
            PinnedTool::Bun => &mut self.bun,
        }
    }
}

#[cfg(test)]
#[path = "catalog_project_tests.rs"]
mod tests;
