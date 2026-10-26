// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::canonical::Digest;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[expect(
    clippy::struct_field_names,
    reason = "wire field names are fixed by the cache contract"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "EntryCacheIdsUnchecked")]
pub struct EntryCacheIds {
    workspace_id: Digest,

    lane_id: Digest,

    platform_id: Digest,

    toolchain_id: Digest,

    cache_format_id: Digest,
}

#[expect(
    clippy::struct_field_names,
    reason = "wire field names are fixed by the cache contract"
)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryCacheIdsUnchecked {
    workspace_id: String,

    lane_id: String,

    platform_id: String,

    toolchain_id: String,

    cache_format_id: String,
}
impl EntryCacheIds {
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

    #[must_use]
    pub fn workspace_id(&self) -> &str {
        self.workspace_id.as_str()
    }

    #[must_use]
    pub fn lane_id(&self) -> &str {
        self.lane_id.as_str()
    }

    #[must_use]
    pub fn platform_id(&self) -> &str {
        self.platform_id.as_str()
    }

    #[must_use]
    pub fn toolchain_id(&self) -> &str {
        self.toolchain_id.as_str()
    }

    #[must_use]
    pub fn cache_format_id(&self) -> &str {
        self.cache_format_id.as_str()
    }

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
