use super::versions::invalid_version;
use super::{PinnedTool, ToolCatalog, validate_exact_version};
use crate::error::MiseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MbxProvisioning {
    CatalogInstall,
    ActionExactVersion,
    PreinstalledTool { tool_path: String },
}

impl MbxProvisioning {
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
    pub fn reconcile_action_mbx(&self, reported: &str) -> Result<(), MiseError> {
        validate_exact_version(PinnedTool::MrBoxington.tool_name(), reported)?;
        (reported == self.version(PinnedTool::MrBoxington))
            .then_some(())
            .ok_or_else(|| invalid_version(PinnedTool::MrBoxington.tool_name(), reported))
    }
}
