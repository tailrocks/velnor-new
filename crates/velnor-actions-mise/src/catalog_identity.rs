use velnor_actions_contract::ToolIdentity;

use super::{
    OPENTOFU_SHA256_LINUX_AMD64, OPENTOFU_VERSION, PLACEHOLDER_DIGEST, PinnedTool, TOOL_PLATFORMS,
    ToolCatalog, jactionlint_archive_sha256, tool_source,
};
use crate::error::MiseError;

impl ToolCatalog {
    #[must_use]
    pub fn tool_identity(&self, tool: PinnedTool) -> ToolIdentity {
        ToolIdentity {
            name: tool.tool_name().to_owned(),
            version: self.version(tool).to_owned(),
            source: tool_source(tool, self.version(tool)),
            platforms: TOOL_PLATFORMS.iter().map(ToString::to_string).collect(),
            digest: self.tool_digest(tool).to_owned(),
        }
    }

    fn tool_digest(&self, tool: PinnedTool) -> &str {
        if tool == PinnedTool::Opentofu && self.version(tool) == OPENTOFU_VERSION {
            OPENTOFU_SHA256_LINUX_AMD64
        } else if tool == PinnedTool::Jactionlint {
            jactionlint_archive_sha256(self.version(tool), "linux-x64")
                .unwrap_or(PLACEHOLDER_DIGEST)
        } else {
            PLACEHOLDER_DIGEST
        }
    }

    pub fn validate_identities(&self) -> Result<(), MiseError> {
        for tool in PinnedTool::ALL {
            self.tool_identity(tool)
                .validate("catalog")
                .map_err(|err| MiseError::Contract {
                    problem: err.to_string(),
                })?;
        }
        Ok(())
    }
}
