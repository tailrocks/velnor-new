// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

pub use velnor_actions_contract::config::project_tool_refs::{
    has_project_tool_ref, is_project_tool_ref, project_tool_refs_to_expressions,
    without_project_tool_refs,
};
use velnor_actions_contract::config::project_tool_resolver::project_tool_keys;

use super::{PinnedTool, ToolCatalog};
use crate::error::MiseError;

pub const PROJECT_TOOL_ENV_PREFIX: &str = "VELNOR_TOOL_";

impl PinnedTool {
    #[must_use]
    pub const fn project_env(self) -> &'static str {
        match self {
            Self::Rust => "VELNOR_TOOL_RUST",
            Self::MrBoxington => "VELNOR_TOOL_MR_BOXINGTON",
            Self::Gh => "VELNOR_TOOL_GH",
            Self::Jactionlint => "VELNOR_TOOL_JACTIONLINT",
            Self::Shellcheck => "VELNOR_TOOL_SHELLCHECK",
            Self::Nextest => "VELNOR_TOOL_NEXTEST",
            Self::Opentofu => "VELNOR_TOOL_OPENTOFU",
            Self::ReleasePlz => "VELNOR_TOOL_RELEASE_PLZ",
            Self::Crane => "VELNOR_TOOL_CRANE",
            Self::Bun => "VELNOR_TOOL_BUN",
        }
    }

    #[must_use]
    pub fn mise_config_keys(self) -> &'static [&'static str] {
        project_tool_keys(self.project_env())
    }
}

#[must_use]
pub fn project_tool_shell_ref(tool: PinnedTool) -> String {
    format!("${{{}}}", tool.project_env())
}

#[must_use]
pub fn project_tool_expression(tool: PinnedTool) -> String {
    format!("${{{{ env.{} }}}}", tool.project_env())
}

impl ToolCatalog {
    pub fn with_project_tool(mut self, tool: PinnedTool, version: &str) -> Result<Self, MiseError> {
        super::validate_concrete_exact_version(tool.tool_name(), version)?;
        version.clone_into(self.version_slot(tool));
        self.project.insert(tool);
        Ok(self)
    }

    #[must_use]
    pub const fn project_tools(&self) -> &BTreeSet<PinnedTool> {
        &self.project
    }

    #[must_use]
    pub fn is_project_declared(&self, tool: PinnedTool) -> bool {
        self.project.contains(&tool)
    }

    #[must_use]
    pub const fn is_emission(&self) -> bool {
        self.emission
    }

    #[must_use]
    pub fn for_emission(&self) -> Self {
        let mut out = self.clone();
        for tool in &self.project {
            *out.version_slot(*tool) = project_tool_shell_ref(*tool);
        }
        out.emission = true;
        out
    }

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
            PinnedTool::Nextest => &mut self.nextest,
            PinnedTool::Opentofu => &mut self.opentofu,
            PinnedTool::ReleasePlz => &mut self.release_plz,
            PinnedTool::Crane => &mut self.crane,
            PinnedTool::Bun => &mut self.bun,
        }
    }
}
