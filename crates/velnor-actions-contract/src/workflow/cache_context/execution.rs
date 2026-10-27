// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use crate::errors::ContractError;
use crate::workflow::cache_identity::{GenerationId, check_schema, parse_strict_record};

use super::validation::require_absolute_path;
use super::{CacheCapability, CacheDriver, CompilerAdapterV1, JobBindingV1, LaneContextV1};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContextV1 {
    pub schema: u32,
    pub job: JobBindingV1,
    pub generation: GenerationId,
    pub requested_coverage_digest: Digest,
    pub source_binding_digest: Digest,
    pub workspace_root: String,
    pub owned_root: String,
    pub cargo_home: String,
    pub rustup_home: String,
    pub tool_root: String,
    pub transport_root: String,
    pub observation_root: String,
    pub lanes: Vec<LaneContextV1>,
    pub compiler_adapters: Vec<CompilerAdapterV1>,
    pub selected_driver: CacheDriver,
    pub mbx_target_views: bool,
    pub effective_cache_capability: CacheCapability,
    pub trusted_default_writer: bool,
    pub private_audience_digest: Digest,
}

impl ExecutionContextV1 {
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, "execution_context.schema")?;
        self.job.validate()?;
        require_absolute_path("execution_context.workspace_root", &self.workspace_root)?;
        require_absolute_path("execution_context.owned_root", &self.owned_root)?;
        require_absolute_path("execution_context.cargo_home", &self.cargo_home)?;
        require_absolute_path("execution_context.rustup_home", &self.rustup_home)?;
        require_absolute_path("execution_context.tool_root", &self.tool_root)?;
        require_absolute_path("execution_context.transport_root", &self.transport_root)?;
        require_absolute_path("execution_context.observation_root", &self.observation_root)?;
        let roots = [
            self.workspace_root.as_str(),
            self.owned_root.as_str(),
            self.cargo_home.as_str(),
            self.rustup_home.as_str(),
            self.tool_root.as_str(),
            self.transport_root.as_str(),
            self.observation_root.as_str(),
        ];
        let distinct: BTreeSet<&str> = roots.into_iter().collect();
        if distinct.len() != roots.len() {
            return Err(ContractError::identity(
                "execution_context.roots",
                "roots_must_be_distinct",
            ));
        }
        if self.lanes.is_empty() {
            return Err(ContractError::identity(
                "execution_context.lanes",
                "empty_lanes",
            ));
        }
        let mut lane_ids = BTreeSet::new();
        for lane in &self.lanes {
            lane.validate()?;
            if !lane_ids.insert(lane.lane_id.as_str()) {
                return Err(ContractError::identity(
                    "execution_context.lanes",
                    "duplicate_lane_id",
                ));
            }
        }
        if self.compiler_adapters.is_empty() {
            return Err(ContractError::identity(
                "execution_context.compiler_adapters",
                "empty_adapters",
            ));
        }
        let mut adapter_paths = BTreeSet::new();
        for adapter in &self.compiler_adapters {
            adapter.validate()?;
            if !adapter_paths.insert(adapter.stable_path.as_str()) {
                return Err(ContractError::identity(
                    "execution_context.compiler_adapters",
                    "duplicate_stable_path",
                ));
            }
        }
        if self.mbx_target_views {
            return Err(ContractError::identity(
                "execution_context.mbx_target_views",
                "must_be_false",
            ));
        }
        Ok(())
    }

    pub fn context_digest(&self) -> Result<Digest, ContractError> {
        self.validate()?;
        Ok(digest_b3_typed(&canonical_json_bytes(self)?))
    }

    pub fn parse_json(text: &str) -> Result<Self, ContractError> {
        let record: Self = parse_strict_record(text)?;
        record.validate()?;
        Ok(record)
    }

    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }
}
