//! Process budgets and test sharding policy.
use crate::errors::ContractError;
use crate::ids::manifest_key_for_cargo_manifest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Process budgets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcesConfig {
    /// Compiler process budget.
    pub compiler_process_budget: u32,
    /// Test process budget.
    pub test_process_budget: u32,
}

/// Test sharding policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestShardingConfig {
    /// Default shard count.
    pub default_shards: u32,
    /// Per-manifest shard overrides keyed by repo-relative manifest path.
    #[serde(default)]
    pub by_manifest: BTreeMap<String, u32>,
}

impl ResourcesConfig {
    /// Validate process budgets.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.compiler_process_budget < 1 {
            return Err(ContractError::config(
                file,
                "resources.compiler_process_budget",
                "must_be_at_least_one",
            ));
        }
        if self.test_process_budget < 1 {
            return Err(ContractError::config(
                file,
                "resources.test_process_budget",
                "must_be_at_least_one",
            ));
        }
        Ok(())
    }
}

impl TestShardingConfig {
    /// Validate shard counts and manifest keys.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.default_shards < 1 {
            return Err(ContractError::config(
                file,
                "test_sharding.default_shards",
                "must_be_at_least_one",
            ));
        }
        for (manifest, shards) in &self.by_manifest {
            manifest_key_for_cargo_manifest(manifest).map_err(|_| {
                ContractError::config(
                    file,
                    format!("test_sharding.by_manifest.{manifest}"),
                    "non_relative_manifest",
                )
            })?;
            if *shards < 1 {
                return Err(ContractError::config(
                    file,
                    format!("test_sharding.by_manifest.{manifest}"),
                    "must_be_at_least_one",
                ));
            }
        }
        Ok(())
    }
}
