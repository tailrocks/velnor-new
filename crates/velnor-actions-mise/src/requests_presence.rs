//! Fail-closed presence preflight for pinned tools.
//!
//! Mise prints `WARN missing:` on stderr and still executes the payload
//! when a selected tool is not installed, so a nonzero-exit classifier
//! can never see the gap. [`ToolPresence`] queries installed versions
//! first (`mise <globals> ls --json <tool>` under the same isolation
//! overlay as `exec`) and fails closed naming the unsatisfied spec
//! before any payload can run.

use serde::Deserialize;

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use velnor_actions_contract::parse_strict_json;

/// Require installed presence for pinned tools before any payload spawn.
///
/// The query is read-only: `mise ls` lists versions, installs nothing.
/// Scoped to plain catalog tools addressed by their registry name, with
/// no backend aliasing: `mise ls --json <tool>` and
/// `mise exec <tool>@<exact>` read the same installs directory under the
/// same isolation overlay, so the list is a faithful proxy for what exec
/// selects. Backend-qualified `aqua:` selectors are deliberately NOT
/// covered here: their aliasing made `mise ls` miss versions that `exec`
/// resolves (proven on CI), so their presence is never gated on `ls`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPresence {
    /// Tools that must already be installed.
    tools: Vec<PinnedTool>,
}

impl ToolPresence {
    /// Require installed presence for the given pinned tools.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyToolchain`] for zero tools.
    pub fn new(tools: Vec<PinnedTool>) -> Result<Self, MiseError> {
        if tools.is_empty() {
            return Err(MiseError::EmptyToolchain);
        }
        Ok(Self { tools })
    }

    /// Installed-version query for one pinned tool.
    #[must_use]
    pub fn command(&self, tool: PinnedTool) -> IsolatedCommand {
        IsolatedCommand::mise_ls_json(tool.tool_name())
    }

    /// Fail unless every pinned tool is installed; never spawns a payload.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::MissingToolPin`] naming the first unsatisfied
    /// spec and the versions actually installed, and propagates spawn,
    /// nonzero-exit, or unparsable-output failures of the read-only
    /// `mise ls` query itself (absence of evidence fails closed too).
    pub fn run(&self, catalog: &ToolCatalog) -> Result<(), MiseError> {
        for tool in &self.tools {
            let output = self.command(*tool).run()?;
            output.require_success("mise")?;
            let json = output.stdout_text("mise")?;
            let installed = installed_versions(&json)?;
            let version = catalog.version(*tool);
            if installed.iter().any(|seen| seen == version) {
                continue;
            }
            return Err(MiseError::MissingToolPin {
                spec: catalog.tool_spec(*tool),
                installed,
            });
        }
        Ok(())
    }
}

/// One `mise ls --json` entry.
#[derive(Deserialize)]
struct InstalledTool {
    /// Installed version selector.
    version: String,
    /// Whether the version is installed (present and complete).
    #[serde(default)]
    installed: bool,
}

/// Sorted installed versions reported by `mise ls --json <tool>`.
///
/// Any shape drift fails closed with an empty list, which [`MiseError::MissingToolPin`]
/// reports instead of silently passing.
fn installed_versions(json: &str) -> Result<Vec<String>, MiseError> {
    let value = parse_strict_json(json).map_err(|err| MiseError::Contract {
        problem: format!("mise_ls_unparsable:{err}"),
    })?;
    let entries: Vec<InstalledTool> =
        serde_json::from_value(value).map_err(|err| MiseError::Contract {
            problem: format!("mise_ls_unparsable:{err}"),
        })?;
    let mut versions: Vec<String> = entries
        .into_iter()
        .filter(|entry| entry.installed)
        .map(|entry| entry.version)
        .collect();
    versions.sort();
    Ok(versions)
}

#[cfg(test)]
#[path = "requests_presence_tests.rs"]
mod tests;
