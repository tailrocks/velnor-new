//! Process budgets and test sharding policy.
use crate::canonical::validate_digest;
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

/// Timing evidence permitting a shard-count change (par §8).
///
/// Shard counts change only after timing evidence; `manifest` names the
/// changed scope (`None` covers `default_shards`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShardTimingEvidence {
    /// Changed manifest scope, or `None` for the default count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    /// Digest of the timing report justifying the change.
    pub timing_digest: String,
}

impl ShardTimingEvidence {
    /// Validate the manifest scope and timing digest.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if let Some(manifest) = &self.manifest {
            manifest_key_for_cargo_manifest(manifest).map_err(|_| {
                ContractError::config(
                    file,
                    format!("shard_timing_evidence.{manifest}"),
                    "non_relative_manifest",
                )
            })?;
        }
        validate_digest(&self.timing_digest)?;
        Ok(())
    }
}

/// Require timing evidence for every shard-count change (par §8).
/// # Errors
pub fn validate_shard_changes_need_evidence(
    previous: &TestShardingConfig,
    next: &TestShardingConfig,
    evidence: &[ShardTimingEvidence],
    file: &str,
) -> Result<(), ContractError> {
    for entry in evidence {
        entry.validate(file)?;
    }
    if previous.default_shards != next.default_shards
        && !evidence.iter().any(|entry| entry.manifest.is_none())
    {
        return Err(ContractError::config(
            file,
            "test_sharding.default_shards",
            "shard_change_without_timing_evidence",
        ));
    }
    for (manifest, shards) in &next.by_manifest {
        if previous.by_manifest.get(manifest) != Some(shards)
            && !evidence
                .iter()
                .any(|entry| entry.manifest.as_deref() == Some(manifest.as_str()))
        {
            return Err(ContractError::config(
                file,
                format!("test_sharding.by_manifest.{manifest}"),
                "shard_change_without_timing_evidence",
            ));
        }
    }
    Ok(())
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
