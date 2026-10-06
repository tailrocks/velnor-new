//! Bounded protected baseline ancestry, shared by planning and merge.

use std::collections::BTreeSet;

use velnor_actions_contract::{canonical_json_bytes, digest_b3};

use super::{baseline_artifact_name, is_unverifiable_generator_sha, reject, validate_task_entry};
use crate::merge::BaselineManifest;
use crate::merge::required_evidence::BaselineTaskEntry;

/// Maximum manifests including the currently observed protected run.
const MAX_LINEAGE_DEPTH: usize = 32;
/// Same bound as exact artifact retrieval; publication must remain readable.
const MAX_LINEAGE_BYTES: usize = 1_048_576;

/// Refuse coverage before another carrying run would exceed ancestry bounds.
pub(crate) fn baseline_can_carry(manifest: &BaselineManifest) -> bool {
    let mut current = Some(manifest);
    let mut depth = 0;
    while let Some(node) = current {
        depth += 1;
        if depth >= MAX_LINEAGE_DEPTH {
            return false;
        }
        current = node.parent.as_deref();
    }
    canonical_json_bytes(manifest).is_ok_and(|bytes| bytes.len() < MAX_LINEAGE_BYTES / 2)
}

/// A trusted current publisher attests its immutable parent manifest.
/// Every carried proof binds that immediate ancestor; the chain terminates
/// at direct execution with the original run ID, never an arbitrary old ID.
pub(crate) fn validate_manifest_lineage(manifest: &BaselineManifest) -> Result<(), String> {
    validate_manifest_lineage_at(manifest, crate::cover_baseline::unix_now())
}

/// Explicit clock keeps merge and planning expiry invariants identical.
pub(crate) fn validate_manifest_lineage_at(
    manifest: &BaselineManifest,
    now_unix: u64,
) -> Result<(), String> {
    let mut current = Some(manifest);
    let mut runs = BTreeSet::new();
    let mut depth = 0;
    while let Some(node) = current {
        depth += 1;
        reject(depth <= MAX_LINEAGE_DEPTH, "baseline_lineage_limit")?;
        reject(runs.insert(node.run_id), "baseline_lineage_cycle")?;
        validate_node(node)?;
        reject(
            !crate::decisions::baseline_expired(node.expires_at_unix, now_unix),
            "cache_expired",
        )?;
        if let Some(parent) = node.parent.as_deref() {
            reject(same_scope(node, parent), "baseline_lineage_scope")?;
            reject(parent.run_id < node.run_id, "baseline_lineage_cycle")?;
        }
        for task in &node.tasks {
            validate_task_entry(task, node.run_id)?;
            validate_origin(task, node)?;
        }
        current = node.parent.as_deref();
    }
    let bytes = canonical_json_bytes(manifest).map_err(|_| "bad_baseline_lineage".to_owned())?;
    reject(bytes.len() <= MAX_LINEAGE_BYTES, "baseline_lineage_limit")
}

/// Intrinsic checks on every previously qualified protected manifest.
fn validate_node(node: &BaselineManifest) -> Result<(), String> {
    let expected_name = baseline_artifact_name(&node.source_commit, &node.compatibility_id)?;
    reject(
        node.schema == crate::internal_plan::snapshot::CANONICAL_SCHEMA_VERSION,
        "bad_baseline_lineage",
    )?;
    reject(
        node.event == "push" && node.final_status == "passed",
        "untrusted_proof",
    )?;
    reject(
        node.run_id > 0 && node.run_attempt > 0,
        "bad_proof_identity",
    )?;
    reject(
        !is_unverifiable_generator_sha(&node.generator_sha256),
        "generator_unverifiable",
    )?;
    reject(
        node.artifact_name == expected_name
            && node.artifact_id
                == crate::cover_compat::baseline_artifact_numeric_id(&expected_name),
        "artifact_mismatch",
    )?;
    let mut tasks = BTreeSet::new();
    reject(
        node.tasks.iter().all(|task| tasks.insert(&task.task_id)),
        "duplicate_task_identity",
    )
}

/// Every ancestor stays inside the exact current trust and generator scope.
fn same_scope(child: &BaselineManifest, parent: &BaselineManifest) -> bool {
    child.repository_id == parent.repository_id
        && child.ref_ == parent.ref_
        && child.workflow_ref == parent.workflow_ref
        && child.generator_version == parent.generator_version
        && child.generator_sha256 == parent.generator_sha256
        && child.compatibility_id == parent.compatibility_id
}

/// Direct execution or exact unchanged task within the bound immediate parent.
fn validate_origin(task: &BaselineTaskEntry, node: &BaselineManifest) -> Result<(), String> {
    let Some(binding) = &task.carried_from else {
        return reject(
            task.proof_run_id == node.run_id,
            "originating_run_unverified",
        );
    };
    reject(task.proof_run_id != node.run_id, "proof_mismatch")?;
    let parent = node
        .parent
        .as_deref()
        .ok_or_else(|| "originating_run_unverified".to_owned())?;
    let bytes = canonical_json_bytes(parent).map_err(|_| "bad_baseline_lineage".to_owned())?;
    let bound = binding.source_commit() == parent.source_commit
        && binding.run_id() == parent.run_id
        && binding.artifact_id() == parent.artifact_id
        && binding.artifact_name() == parent.artifact_name
        && binding.manifest_digest() == digest_b3(&bytes);
    reject(bound, "proof_mismatch")?;
    let origin = parent
        .tasks
        .iter()
        .find(|entry| entry.task_id == task.task_id)
        .ok_or_else(|| "originating_run_unverified".to_owned())?;
    reject(same_task(task, origin)?, "proof_mismatch")
}

/// Immutable execution identity includes structured proof and freshness.
fn same_task(child: &BaselineTaskEntry, parent: &BaselineTaskEntry) -> Result<bool, String> {
    let proofs = canonical_json_bytes(&(&child.proof, &child.external_data))
        .and_then(|left| {
            canonical_json_bytes(&(&parent.proof, &parent.external_data)).map(|right| left == right)
        })
        .map_err(|_| "bad_baseline_lineage".to_owned())?;
    Ok(child.task_id == parent.task_id
        && child.task_digest == parent.task_digest
        && child.input_digest == parent.input_digest
        && child.closure_digest == parent.closure_digest
        && child.proof_run_id == parent.proof_run_id
        && proofs)
}

#[cfg(test)]
#[path = "provenance_lineage_tests.rs"]
mod tests;
