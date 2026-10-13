//! Nextest sharding proofs, budgets, and exact-base baseline lookup.
//!
//! Shard proofs bind each partition to its plan obligation; the lookup finds
//! the exact-base successful run through pinned `gh`. Cargo-test profiles
//! never shard and never receive archives.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::validate_digest;

pub(crate) use super::shard_baseline::{BaselineLookup, resolve_manifests};
pub(crate) use velnor_actions_orchestrator_merge_ports::{
    ResourceLimits, ShardProof, TestIdentity, inventory_digest,
};

/// V1 retry budget: retries are always zero.
pub(crate) const MAX_RETRIES: u32 = 0;

/// Validate merge proofs for the sharded bases of one entry.
/// # Errors
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

/// Validate one base group, returning its empty-partition proof count.
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
    let nextest = velnor_actions_rust_core::TestRunner::parse(&first.runner)
        .is_ok_and(|runner| runner == velnor_actions_rust_core::TestRunner::CargoNextest);
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

/// Reject zero budgets, over-budget shards, and above-capacity concurrency.
/// # Errors
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

#[cfg(test)]
mod tests;
