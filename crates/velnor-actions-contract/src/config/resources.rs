use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::ids::manifest_key_for_cargo_manifest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcesConfig {
    pub compiler_process_budget: u32,

    pub test_process_budget: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestShardingConfig {
    pub default_shards: u32,

    #[serde(default)]
    pub by_manifest: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShardTimingEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,

    pub timing_digest: String,
}

impl ShardTimingEvidence {
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

impl super::VelnorConfig {
    pub fn check_shard_budgets(&self, file: &str) -> Result<(), ContractError> {
        let budget = self.resources.test_process_budget;
        if self.test_sharding.default_shards > budget {
            return Err(ContractError::config(
                file,
                "test_sharding.default_shards",
                "exceeds_test_process_budget",
            ));
        }
        for (manifest, shards) in &self.test_sharding.by_manifest {
            if *shards > budget {
                return Err(ContractError::config(
                    file,
                    format!("test_sharding.by_manifest.{manifest}"),
                    "exceeds_test_process_budget",
                ));
            }
        }
        Ok(())
    }
}
