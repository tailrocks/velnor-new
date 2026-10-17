//! Pinned identity records (ver §2) plus the trusted gate over them.
//!
//! Declared via `#[path]` from `catalog.rs` (400-line budget); the child
//! module keeps the private catalog field access.

use velnor_actions_contract::ToolIdentity;

use super::{
    OPENTOFU_SHA256_LINUX_AMD64, OPENTOFU_VERSION, PLACEHOLDER_DIGEST, PinnedTool, TOOL_PLATFORMS,
    ToolCatalog, tool_source,
};
use crate::error::MiseError;

impl ToolCatalog {
    /// Pinned identity record for one tool (ver §2).
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

    /// Qualified artifact digest for one tool, or the placeholder.
    fn tool_digest(&self, tool: PinnedTool) -> &str {
        if tool == PinnedTool::Opentofu && self.version(tool) == OPENTOFU_VERSION {
            OPENTOFU_SHA256_LINUX_AMD64
        } else {
            PLACEHOLDER_DIGEST
        }
    }

    /// Trusted gate over every catalog identity record.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::Contract`] for the first invalid identity.
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
