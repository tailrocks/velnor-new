//! Action-owned MBX payload constructors and runtime path binding.

use std::ffi::{OsStr, OsString};

use crate::catalog::{PinnedTool, ToolCatalog};
use crate::command::{ActionOwnedMbxPath, EnvPolicy, IsolatedCommand};
use crate::error::MiseError;

use super::{
    ActionOwnedMbxMode, MBX_PROGRAM, MbxAuthority, PinnedToolExec, mbx_rust_selector,
    unpinned_mbx_error,
};

impl PinnedToolExec {
    /// Build a fixed candidate through Rust selected by Mise and MBX supplied
    /// by the workflow's separately pinned native action.
    pub(crate) fn new_action_owned_mbx(
        tools: Vec<PinnedTool>,
        args: Vec<OsString>,
    ) -> Result<Self, MiseError> {
        if !tools.contains(&PinnedTool::Rust) || tools.contains(&PinnedTool::MrBoxington) {
            return Err(unpinned_mbx_error(OsStr::new(MBX_PROGRAM)));
        }
        Self::new_with_authority(
            tools,
            OsStr::new(MBX_PROGRAM),
            args,
            Some(MbxAuthority::ActionOwned(ActionOwnedMbxMode::RustBound)),
        )
    }

    /// Build the action-owned MBX version probe without a compiler selector.
    pub(crate) fn new_action_owned_mbx_version_probe(
        tools: Vec<PinnedTool>,
    ) -> Result<Self, MiseError> {
        if !tools.contains(&PinnedTool::Rust) || tools.contains(&PinnedTool::MrBoxington) {
            return Err(unpinned_mbx_error(OsStr::new(MBX_PROGRAM)));
        }
        Self::new_with_authority(
            tools,
            OsStr::new(MBX_PROGRAM),
            vec![OsString::from("--version")],
            Some(MbxAuthority::ActionOwned(ActionOwnedMbxMode::VersionProbe)),
        )
    }

    /// Build an inspection template using the portable bare MBX token.
    /// Runtime callers must bind the token to a resolved action path first.
    pub(crate) fn command_template(
        &self,
        catalog: &ToolCatalog,
    ) -> Result<IsolatedCommand, MiseError> {
        let specs = catalog.tool_specs(&self.tools);
        let command = IsolatedCommand::mise_exec(&specs, &self.payload(catalog))?;
        if self.tools.as_slice() == [PinnedTool::Gh] {
            return Ok(command.with_policy(EnvPolicy::Baseline));
        }
        if self.mbx_authority.is_some() {
            return Ok(command.with_policy(EnvPolicy::Mbx));
        }
        Ok(command)
    }

    /// Build the Rust-only Mise invocation using one resolved action MBX.
    pub(crate) fn command_with_action_owned_path(
        &self,
        catalog: &ToolCatalog,
        path: &ActionOwnedMbxPath,
    ) -> Result<IsolatedCommand, MiseError> {
        let Some(MbxAuthority::ActionOwned(mode)) = self.mbx_authority else {
            return Err(unpinned_mbx_error(&self.program));
        };
        let mut payload = vec![path.as_os_str().to_owned()];
        if mode == ActionOwnedMbxMode::RustBound {
            payload.push(OsString::from(mbx_rust_selector(catalog)));
        }
        payload.extend(self.args.iter().cloned());
        let specs = catalog.tool_specs(&self.tools);
        IsolatedCommand::mise_exec(&specs, &payload)
            .map(|command| command.with_policy(EnvPolicy::Mbx))
    }
}
