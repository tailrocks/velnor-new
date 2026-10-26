// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::canonical::Digest;
use crate::errors::ContractError;
use crate::workflow::cache_identity::require_nonempty;

use super::validation::{require_absolute_path, require_relative_path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneContextV1 {
    pub lane_id: String,
    pub command_family: String,
    pub variant_digest: Digest,
    pub target_dir: String,
    pub build_dir: Option<String>,
}

impl LaneContextV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        require_nonempty("lane.lane_id", &self.lane_id)?;
        require_nonempty("lane.command_family", &self.command_family)?;
        require_relative_path("lane.target_dir", &self.target_dir)?;
        if let Some(build_dir) = &self.build_dir {
            require_relative_path("lane.build_dir", build_dir)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerAdapterV1 {
    pub stable_path: String,
    pub executable_identity: String,
}

impl CompilerAdapterV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        require_absolute_path("compiler_adapter.stable_path", &self.stable_path)?;
        require_nonempty(
            "compiler_adapter.executable_identity",
            &self.executable_identity,
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheCapability {
    Read,
    Write,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheDriver {
    Cargo,
    Mbx,
}
