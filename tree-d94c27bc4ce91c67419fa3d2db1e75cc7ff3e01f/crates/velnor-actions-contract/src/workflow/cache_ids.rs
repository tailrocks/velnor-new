//! Per-entry cache identity digests recorded in the plan (cache §1).
use crate::canonical::validate_digest;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
/// Workspace/lane/platform/toolchain/format digests for one entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryCacheIds {
    /// Workspace identity digest.
    pub workspace_id: String,
    /// Lane identity digest.
    pub lane_id: String,
    /// Platform identity digest.
    pub platform_id: String,
    /// Toolchain identity digest.
    pub toolchain_id: String,
    /// Cache-format identity digest.
    pub cache_format_id: String,
}
impl EntryCacheIds {
    /// Validate every recorded cache identity digest.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        for value in [
            self.workspace_id.as_str(),
            self.lane_id.as_str(),
            self.platform_id.as_str(),
            self.toolchain_id.as_str(),
            self.cache_format_id.as_str(),
        ] {
            validate_digest(value)?;
        }
        Ok(())
    }
}
