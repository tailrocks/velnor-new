//! MBX provisioning modes (P07-10 contract).
//!
//! Declared from `catalog.rs` (`#[path]`); re-exported there so
//! `catalog::MbxProvisioning` keeps working.

use super::versions::invalid_version;
use super::{PinnedTool, ToolCatalog, validate_exact_version};
use crate::error::MiseError;

/// How the effective MBX binary is provisioned (P07-10 contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MbxProvisioning {
    /// Mise installs the catalog pin; the action installs nothing.
    CatalogInstall,
    /// The action installs through its exact-version input (must equal the pin).
    ActionExactVersion,
    /// The action runs a preinstalled binary at this absolute path.
    PreinstalledTool {
        /// Absolute path to the preinstalled `mbx` binary.
        tool_path: String,
    },
}

impl MbxProvisioning {
    /// Preinstalled-tool mode for one validated absolute binary path.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidStepInput`] unless the path is absolute.
    pub fn preinstalled(tool_path: &str) -> Result<Self, MiseError> {
        if !tool_path.starts_with('/') || tool_path.contains('\0') {
            return Err(MiseError::InvalidStepInput {
                field: "tool_path".to_owned(),
                value: tool_path.to_owned(),
            });
        }
        Ok(Self::PreinstalledTool {
            tool_path: tool_path.to_owned(),
        })
    }
}

impl ToolCatalog {
    /// Fail unless the action-installed MBX equals the catalog pin (action SHA alone never proves it).
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::InvalidToolVersion`] for a loose or mismatched version.
    pub fn reconcile_action_mbx(&self, reported: &str) -> Result<(), MiseError> {
        validate_exact_version(PinnedTool::MrBoxington.tool_name(), reported)?;
        (reported == self.version(PinnedTool::MrBoxington))
            .then_some(())
            .ok_or_else(|| invalid_version(PinnedTool::MrBoxington.tool_name(), reported))
    }
}
