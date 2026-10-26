// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{canonical_json_bytes, digest_b3, validate_digest};

pub(crate) const MAX_RETRIES: u32 = 0;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct TestIdentity {
    pub(crate) package: String,
    pub(crate) target: String,
    pub(crate) features: Vec<String>,
    pub(crate) binary: String,
    pub(crate) name: String,
}

impl TestIdentity {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let fields = [&self.package, &self.target, &self.binary, &self.name];
        let clean = fields
            .iter()
            .all(|v| !v.trim().is_empty() && !v.starts_with('/'));
        let sorted = self.features.windows(2).all(|pair| pair[0] <= pair[1]);
        if clean && sorted {
            Ok(())
        } else {
            Err("malformed_test_identity".into())
        }
    }
}

#[must_use]
pub(crate) fn inventory_digest(tests: &[TestIdentity]) -> String {
    let mut sorted = tests.to_vec();
    sorted.sort();
    canonical_json_bytes(&sorted)
        .map(|bytes| digest_b3(&bytes))
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ShardProof {
    pub(crate) task_id: String,
    pub(crate) input_digest: String,
    pub(crate) runner: String,
    pub(crate) shard_index: u32,
    pub(crate) shard_count: u32,
    pub(crate) tests: Vec<TestIdentity>,
    pub(crate) inventory_digest: String,
    pub(crate) archive_digest: Option<String>,
    pub(crate) no_test_targets: bool,
}

pub(crate) fn check_entry_shards(
    bases: &BTreeSet<String>,
    empty_count: u32,
    proofs: &[ShardProof],
    obligations: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut empty = 0u32;
    for base in bases {
        let group: Vec<&ShardProof> = proofs
            .iter()
            .filter(|p| {
                velnor_actions_contract::split_shard_suffix(&p.task_id)
                    .is_some_and(|(proof_base, _, _)| proof_base == base.as_str())
            })
            .collect();
        empty += check_group(&group, obligations)?;
    }
    if empty == empty_count {
        Ok(())
    } else {
        Err("empty_partition_unproven".into())
    }
}

fn check_group(
    group: &[&ShardProof],
    obligations: &BTreeMap<String, String>,
) -> Result<u32, String> {
    let Some(first) = group.first() else {
        return Err("missing_shard".into());
    };
    for proof in group {
        if obligations.get(&proof.task_id) != Some(&proof.input_digest) {
            return Err("shard_input_mismatch".into());
        }
        if validate_digest(&proof.input_digest).is_err() {
            return Err("shard_input_mismatch".into());
        }
    }
    let nextest = velnor_actions_rust::TestRunner::parse(&first.runner)
        .is_ok_and(|runner| runner == velnor_actions_rust::TestRunner::CargoNextest);
    if !nextest {
        return Err("sharding_requires_nextest".into());
    }
    if velnor_actions_mise::requires_archive_transfer(first.shard_count)
        && first.archive_digest.is_none()
        && !first.no_test_targets
    {
        return Err("missing_archive".into());
    }
    for proof in group {
        let same = proof.runner == first.runner && proof.shard_count == first.shard_count;
        let same = same && proof.inventory_digest == first.inventory_digest;
        let same = same && proof.archive_digest == first.archive_digest;
        if !same {
            return Err("shard_group_mismatch".into());
        }
        for test in &proof.tests {
            test.validate()?;
        }
    }
    let mut indices: Vec<u32> = group.iter().map(|proof| proof.shard_index).collect();
    indices.sort_unstable();
    if indices != (1..=first.shard_count).collect::<Vec<_>>() {
        return Err("missing_shard".into());
    }
    let mut union: Vec<TestIdentity> = group.iter().flat_map(|p| p.tests.clone()).collect();
    union.sort();
    if union.len() != union.iter().collect::<BTreeSet<_>>().len() {
        return Err("duplicate_test".into());
    }
    if inventory_digest(&union) != first.inventory_digest {
        return Err("tampered_manifest".into());
    }
    if union.is_empty() && !first.no_test_targets {
        return Err("empty_inventory_unproven".into());
    }
    if union.is_empty() {
        return Ok(0);
    }
    let empty = group.iter().filter(|proof| proof.tests.is_empty()).count();
    Ok(u32::try_from(empty).unwrap_or(u32::MAX))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResourceLimits {
    pub(crate) compiler_budget: u32,
    pub(crate) test_budget: u32,
    pub(crate) max_parallel: u32,
    pub(crate) capacity: u32,
    pub(crate) shards: u32,
    pub(crate) retries: u32,
    #[serde(default)]
    pub(crate) resource_groups: Vec<String>,
}

pub(crate) fn validate_budgets(limits: &ResourceLimits) -> Result<(), String> {
    let budgets = [
        limits.compiler_budget,
        limits.test_budget,
        limits.max_parallel,
    ];
    if budgets.contains(&0) {
        return Err("budget_must_be_positive".into());
    }
    if limits.retries != MAX_RETRIES {
        return Err("retries_disabled".into());
    }
    if limits.shards > limits.test_budget {
        return Err("shards_exceed_test_budget".into());
    }
    if limits.max_parallel > limits.capacity {
        return Err("concurrency_above_capacity".into());
    }
    let mut groups = limits.resource_groups.clone();
    groups.sort();
    groups.dedup();
    if groups != limits.resource_groups || groups.iter().any(|group| group.trim().is_empty()) {
        return Err("resource_groups_unordered".into());
    }
    Ok(())
}
