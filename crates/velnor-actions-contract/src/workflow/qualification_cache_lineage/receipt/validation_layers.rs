//! Lane, layer and transition observation checks.

use std::collections::BTreeMap;

use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::workflow::QualificationPhase;
use crate::workflow::plan::Plan;

use super::super::super::MAX_QUALIFICATION_CACHE_LANES;
use super::super::super::identity::{
    QualificationCacheLayer, identity_commitment, key_for_slot, runtime_requirements,
    validate_runtime_identity,
};
use super::super::errors::invalid;
use super::super::lineage::{find_layer_receipt, layer_applies};
use super::super::types::{
    AdmissionNode, QualificationCacheLaneReceipt, QualificationCacheLayerReceipt,
};
use transitions::{
    phase_restore, save_slot, validate_disabled_layer, validate_restore, validate_save,
    validate_third_observations, validate_useful_delta_progress,
};

#[path = "validation_transitions.rs"]
mod transitions;

pub(in crate::workflow::qualification_cache_lineage::receipt) fn validate_lanes(
    plan: &Plan,
    node: &AdmissionNode,
    previous: Option<&AdmissionNode>,
) -> Result<(), ContractError> {
    if node.receipt.lanes.len() != plan.matrix.include.len()
        || node.receipt.lanes.len() > MAX_QUALIFICATION_CACHE_LANES
    {
        return Err(invalid("lane_set_mismatch"));
    }
    let lanes = lane_index(&node.receipt.lanes)?;
    for entry in &plan.matrix.include {
        let lane = lanes
            .get(entry.matrix_key.as_str())
            .ok_or_else(|| invalid("lane_set_mismatch"))?;
        validate_lane(plan, entry, lane, node, previous)?;
    }
    if node.receipt.phase == QualificationPhase::UsefulDelta {
        validate_useful_delta_progress(node, previous)?;
    } else if node.receipt.phase == QualificationPhase::Third {
        validate_third_observations(node, previous)?;
    }
    Ok(())
}

fn validate_lane(
    plan: &Plan,
    entry: &crate::workflow::MatrixEntry,
    lane: &QualificationCacheLaneReceipt,
    node: &AdmissionNode,
    previous: Option<&AdmissionNode>,
) -> Result<(), ContractError> {
    if lane.matrix_key != entry.matrix_key
        || lane.stack_id != entry.stack_id
        || lane.task_id != entry.task_id
    {
        return Err(invalid("lane_identity_mismatch"));
    }
    validate_digest(&lane.closure_digest)?;
    validate_digest(&lane.useful_state_digest)?;
    let mut expected_tasks = entry
        .execute_task_ids
        .tasks
        .values()
        .flat_map(|task_ref| match task_ref {
            crate::workflow::ExecuteTaskRef::Single(task) => vec![task.clone()],
            crate::workflow::ExecuteTaskRef::Shards(tasks) => tasks.clone(),
        })
        .collect::<Vec<_>>();
    expected_tasks.sort();
    if lane.completed_task_ids != expected_tasks || lane.layers.len() != 6 {
        return Err(invalid("lane_execution_or_layer_set_mismatch"));
    }
    let requirements = runtime_requirements(plan, entry)?;
    let expected_layers = [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::MbxBundle,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ];
    for (layer, expected) in lane.layers.iter().zip(expected_layers) {
        if layer.layer != expected {
            return Err(invalid("layers_not_closed_and_sorted"));
        }
        validate_layer(plan, entry, layer, &requirements, node, previous)?;
    }
    if node.receipt.phase == QualificationPhase::Third {
        let prior = previous
            .and_then(|previous| {
                previous
                    .receipt
                    .lanes
                    .iter()
                    .find(|record| record.matrix_key == entry.matrix_key)
            })
            .ok_or_else(|| invalid("third_missing_warm_lane"))?;
        if lane.closure_digest != prior.closure_digest
            || lane.useful_state_digest != prior.useful_state_digest
        {
            return Err(invalid("third_run_changed_useful_state"));
        }
    }
    Ok(())
}

fn validate_layer(
    plan: &Plan,
    entry: &crate::workflow::MatrixEntry,
    layer: &QualificationCacheLayerReceipt,
    requirements: &super::super::super::runtime::QualificationRuntimeIdentityRequirements,
    node: &AdmissionNode,
    previous: Option<&AdmissionNode>,
) -> Result<(), ContractError> {
    let phase = node.receipt.phase;
    let applicable = layer_applies(entry, layer.layer);
    let identity = identity_commitment(plan, entry, layer.layer)?;
    let expected_active = applicable
        && layer.layer != QualificationCacheLayer::TaskResult
        && phase != QualificationPhase::Control;
    if layer.identity_digest != identity || layer.active != expected_active {
        return Err(invalid("layer_identity_mismatch"));
    }
    if !expected_active {
        return validate_disabled_layer(layer);
    }
    let state_digest = layer
        .state_digest
        .as_deref()
        .ok_or_else(|| invalid("active_layer_state_missing"))?;
    validate_digest(state_digest)?;
    let runtime = layer
        .runtime_identity
        .as_ref()
        .ok_or_else(|| invalid("missing_runtime_identity"))?;
    let runtime_digest = validate_runtime_identity(requirements, runtime)?;
    let (restore_slot, expected_archive) =
        phase_restore(phase, previous, &entry.matrix_key, layer.layer)?;
    let restore_key = key_for_slot(
        &node.receipt.campaign,
        layer.layer,
        &identity,
        &runtime_digest,
        restore_slot,
    )?;
    validate_restore(&layer.restore, &restore_key, expected_archive.as_ref())?;
    if let Some(previous) = previous {
        let prior_layer = find_layer_receipt(Some(previous), &entry.matrix_key, layer.layer)?;
        if prior_layer.runtime_identity.as_ref() != Some(runtime) {
            return Err(invalid("runtime_identity_changed_in_lineage"));
        }
    }
    let save_slot = save_slot(phase);
    let save_key = save_slot
        .map(|slot| {
            key_for_slot(
                &node.receipt.campaign,
                layer.layer,
                &identity,
                &runtime_digest,
                slot,
            )
        })
        .transpose()?;
    let previous_state = previous
        .map(|previous| find_layer_receipt(Some(previous), &entry.matrix_key, layer.layer))
        .transpose()?
        .and_then(|prior| prior.state_digest.as_deref());
    validate_save(
        &layer.save,
        save_key.as_deref(),
        state_digest,
        previous_state,
        &node.metadata,
        &node.receipt,
    )
}

fn lane_index(
    lanes: &[QualificationCacheLaneReceipt],
) -> Result<BTreeMap<&str, &QualificationCacheLaneReceipt>, ContractError> {
    let index: BTreeMap<_, _> = lanes
        .iter()
        .map(|lane| (lane.matrix_key.as_str(), lane))
        .collect();
    if index.len() == lanes.len() {
        Ok(index)
    } else {
        Err(invalid("lane_set_duplicate"))
    }
}

#[cfg(test)]
#[path = "validation_layers_tests.rs"]
mod tests;
