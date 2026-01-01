//! MBX provisioning modes (P07-10 contract).
//!
//! Declared from `catalog`; re-exported there so
//! `catalog::MbxProvisioning` keeps working.

use velnor_actions_mise_core::error::MiseError;

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
