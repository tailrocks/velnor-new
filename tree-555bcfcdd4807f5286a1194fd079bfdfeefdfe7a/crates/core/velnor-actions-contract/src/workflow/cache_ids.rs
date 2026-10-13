//! Per-entry cache identity digests recorded in the plan (cache §1).
use crate::canonical::Digest;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
/// Workspace/lane/platform/toolchain/format digests for one entry.
///
/// Constructible only via [`EntryCacheIds::new`] or validated
/// deserialization; every digest is a parsed [`Digest`].
#[expect(
    clippy::struct_field_names,
    reason = "wire field names are fixed by the cache contract"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "EntryCacheIdsUnchecked")]
pub struct EntryCacheIds {
    /// Workspace identity digest.
    workspace_id: Digest,
    /// Lane identity digest.
    lane_id: Digest,
    /// Platform identity digest.
    platform_id: Digest,
    /// Toolchain identity digest.
    toolchain_id: Digest,
    /// Cache-format identity digest.
    cache_format_id: Digest,
}
/// Unvalidated cache-IDs wire shape (never exposed).
#[expect(
    clippy::struct_field_names,
    reason = "wire field names are fixed by the cache contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryCacheIdsUnchecked {
    /// Workspace identity digest.
    workspace_id: String,
    /// Lane identity digest.
    lane_id: String,
    /// Platform identity digest.
    platform_id: String,
    /// Toolchain identity digest.
    toolchain_id: String,
    /// Cache-format identity digest.
    cache_format_id: String,
}
impl EntryCacheIds {
    /// Build cache IDs from five validated digests.
    /// # Errors
    pub fn new(
        workspace_id: &str,
        lane_id: &str,
        platform_id: &str,
        toolchain_id: &str,
        cache_format_id: &str,
    ) -> Result<Self, ContractError> {
        Ok(Self {
            workspace_id: Digest::parse(workspace_id)?,
            lane_id: Digest::parse(lane_id)?,
            platform_id: Digest::parse(platform_id)?,
            toolchain_id: Digest::parse(toolchain_id)?,
            cache_format_id: Digest::parse(cache_format_id)?,
        })
    }

    /// Workspace identity digest.
    #[must_use]
    pub fn workspace_id(&self) -> &str {
        self.workspace_id.as_str()
    }

    /// Lane identity digest.
    #[must_use]
    pub fn lane_id(&self) -> &str {
        self.lane_id.as_str()
    }

    /// Platform identity digest.
    #[must_use]
    pub fn platform_id(&self) -> &str {
        self.platform_id.as_str()
    }

    /// Toolchain identity digest.
    #[must_use]
    pub fn toolchain_id(&self) -> &str {
        self.toolchain_id.as_str()
    }

    /// Cache-format identity digest.
    #[must_use]
    pub fn cache_format_id(&self) -> &str {
        self.cache_format_id.as_str()
    }

    /// Re-validate stored digests (construction already validated).
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        Self::new(
            self.workspace_id.as_str(),
            self.lane_id.as_str(),
            self.platform_id.as_str(),
            self.toolchain_id.as_str(),
            self.cache_format_id.as_str(),
        )?;
        Ok(())
    }
}
impl TryFrom<EntryCacheIdsUnchecked> for EntryCacheIds {
    type Error = ContractError;
    fn try_from(raw: EntryCacheIdsUnchecked) -> Result<Self, Self::Error> {
        Self::new(
            &raw.workspace_id,
            &raw.lane_id,
            &raw.platform_id,
            &raw.toolchain_id,
            &raw.cache_format_id,
        )
    }
}

#[cfg(test)]
mod tests;
