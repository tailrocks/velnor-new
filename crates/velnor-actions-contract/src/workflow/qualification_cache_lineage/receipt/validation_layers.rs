//! Lane, layer and transition observation checks.

use std::collections::BTreeMap;

use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::workflow::QualificationPhase;
use crate::workflow::plan::Plan;

use super::super::super::MAX_QUALIFICATION_CACHE_LANES;
use super::super::super::identity::{
    QualificationCacheLayer, key_for_slot, validate_runtime_identity,
};
use super::super::errors::invalid;
use super::super::lineage::find_layer_receipt;
use super::super::types::{
    AdmissionNode, QualificationCacheLaneReceipt, QualificationCacheLayerReceipt,
    QualificationCacheOwnerReceipt, QualificationCacheProducerContext,
    QualificationCacheProducerEvidence, QualificationCacheReceipt,
};
use transitions::{
    phase_restore, save_slot, validate_disabled_layer, validate_restore, validate_save,
    validate_third_observations, validate_useful_delta_progress,
};

#[path = "validation_transitions.rs"]
mod transitions;

pub(in crate::workflow::qualification_cache_lineage::receipt) fn validate_lanes(
    plan: &Plan,
    producer: &QualificationCacheProducerContext,
    evidence: LaneEvidence<'_>,
    previous: Option<&AdmissionNode>,
) -> Result<(), ContractError> {
    if evidence.lanes.len() != plan.matrix.include.len()
        || evidence.lanes.len() > MAX_QUALIFICATION_CACHE_LANES
    {
        return Err(invalid("lane_set_mismatch"));
    }
    let lanes = lane_index(evidence.lanes)?;
    for entry in &plan.matrix.include {
        let lane = lanes
            .get(entry.matrix_key.as_str())
            .ok_or_else(|| invalid("lane_set_mismatch"))?;
        validate_lane(plan, entry, lane, evidence, previous)?;
    }
    validate_owner_layers(plan, producer, evidence, previous)?;
    if evidence.phase == QualificationPhase::UsefulDelta {
        validate_useful_delta_progress(evidence.lanes, evidence.owners, previous)?;
    } else if evidence.phase == QualificationPhase::Third {
        validate_third_observations(evidence.lanes, evidence.owners, previous)?;
    }
    Ok(())
}

fn validate_lane(
    plan: &Plan,
    entry: &crate::workflow::MatrixEntry,
    lane: &QualificationCacheLaneReceipt,
    evidence: LaneEvidence<'_>,
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
    if lane.completed_task_ids != expected_tasks || lane.layers.len() != 5 {
        return Err(invalid("lane_execution_or_layer_set_mismatch"));
    }
    let owner = super::super::super::identity::owner_for_plan(plan, &entry.job_id)?;
    let expected_layers = [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ];
    for (layer, expected) in lane.layers.iter().zip(expected_layers) {
        if layer.layer != expected {
            return Err(invalid("layers_not_closed_and_sorted"));
        }
        let identity = super::super::super::identity::owner_layer_identity(plan, &owner, expected)?;
        let active = owner
            .entries
            .iter()
            .any(|member| super::super::lineage::layer_applies(member, expected))
            && expected != QualificationCacheLayer::TaskResult
            && evidence.phase != QualificationPhase::Control;
        if layer.identity_digest != identity
            || layer.active != active
            || layer.owner_id.as_deref() != active.then_some(owner.owner_id.as_str())
        {
            return Err(invalid("lane_owner_layer_reference_mismatch"));
        }
    }
    if evidence.phase == QualificationPhase::Third {
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

fn validate_owner_layers(
    plan: &Plan,
    producer: &QualificationCacheProducerContext,
    evidence: LaneEvidence<'_>,
    previous: Option<&AdmissionNode>,
) -> Result<(), ContractError> {
    let mut expected_ids = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.job_id.as_str())
        .collect::<Vec<_>>();
    expected_ids.sort_unstable();
    expected_ids.dedup();
    if evidence.owners.len() != expected_ids.len() {
        return Err(invalid("owner_set_mismatch"));
    }
    for (owner, owner_id) in evidence.owners.iter().zip(expected_ids) {
        let expected = super::super::super::identity::owner_for_plan(plan, owner_id)?;
        if owner.owner_id != expected.owner_id
            || owner.platform_id != expected.platform_id
            || !owner_members_match_plan(owner, &expected, plan, evidence.source_sha)?
            || owner.layers.len() != 5
        {
            return Err(invalid("owner_membership_or_identity_mismatch"));
        }
        let requirements =
            super::super::super::identity::owner_runtime_requirements(plan, &expected)?;
        let validation = OwnerLayerValidation {
            plan,
            expected_owner: &expected,
            producer,
            evidence,
            previous,
        };
        let expected_layers = [
            QualificationCacheLayer::MbxObjects,
            QualificationCacheLayer::CargoSources,
            QualificationCacheLayer::MiseTools,
            QualificationCacheLayer::TofuProviders,
            QualificationCacheLayer::TaskResult,
        ];
        for (record, layer) in owner.layers.iter().zip(expected_layers) {
            validate_owner_layer(validation, owner, record, layer, requirements.as_ref())?;
        }
    }
    Ok(())
}

fn owner_members_match_plan(
    owner: &QualificationCacheOwnerReceipt,
    expected: &super::super::super::identity::QualificationCacheOwner,
    plan: &Plan,
    source_sha: &str,
) -> Result<bool, ContractError> {
    validate_digest(&owner.member_digest)?;
    if source_sha
        == plan
            .qualification
            .as_ref()
            .ok_or_else(|| invalid("missing_qualification_context"))?
            .source_sha
    {
        return Ok(
            owner.member_digest == expected.member_digest && owner.members == expected.members
        );
    }
    if plan
        .qualification
        .as_ref()
        .is_none_or(|context| context.phase != QualificationPhase::UsefulDelta)
    {
        return Ok(false);
    }
    if owner.members.len() != expected.members.len() {
        return Ok(false);
    }
    Ok(owner
        .members
        .iter()
        .zip(&expected.members)
        .all(|(actual, planned)| {
            actual.matrix_key == planned.matrix_key
                && actual.stack_id == planned.stack_id
                && actual.task_id == planned.task_id
                && actual.task_digest == planned.task_digest
        }))
}

#[derive(Clone, Copy)]
struct OwnerLayerValidation<'a> {
    plan: &'a Plan,
    expected_owner: &'a super::super::super::identity::QualificationCacheOwner,
    producer: &'a QualificationCacheProducerContext,
    evidence: LaneEvidence<'a>,
    previous: Option<&'a AdmissionNode>,
}

fn validate_owner_layer(
    validation: OwnerLayerValidation<'_>,
    owner: &QualificationCacheOwnerReceipt,
    layer: &QualificationCacheLayerReceipt,
    expected_layer: QualificationCacheLayer,
    requirements: Option<&super::super::super::runtime::QualificationRuntimeIdentityRequirements>,
) -> Result<(), ContractError> {
    if layer.layer != expected_layer
        || layer.owner_id != owner.owner_id
        || layer.platform_id != owner.platform_id
        || layer.member_digest != owner.member_digest
    {
        return Err(invalid("owner_layers_not_closed_and_sorted"));
    }
    let identity = super::super::super::identity::owner_layer_identity(
        validation.plan,
        validation.expected_owner,
        layer.layer,
    )?;
    let active = validation
        .expected_owner
        .entries
        .iter()
        .any(|entry| super::super::lineage::layer_applies(entry, layer.layer))
        && layer.layer != QualificationCacheLayer::TaskResult
        && validation.evidence.phase != QualificationPhase::Control;
    if layer.identity_digest != identity || layer.active != active {
        return Err(invalid("owner_layer_identity_mismatch"));
    }
    if !active {
        return validate_disabled_layer(layer);
    }
    validate_layer(
        validation.plan,
        validation.expected_owner,
        layer,
        requirements,
        validation.producer,
        validation.evidence,
        validation.previous,
    )
}

fn validate_layer(
    plan: &Plan,
    owner: &super::super::super::identity::QualificationCacheOwner,
    layer: &QualificationCacheLayerReceipt,
    requirements: Option<&super::super::super::runtime::QualificationRuntimeIdentityRequirements>,
    producer: &QualificationCacheProducerContext,
    evidence: LaneEvidence<'_>,
    previous: Option<&AdmissionNode>,
) -> Result<(), ContractError> {
    let phase = evidence.phase;
    let identity = super::super::super::identity::owner_layer_identity(plan, owner, layer.layer)?;
    let requirements =
        requirements.ok_or_else(|| invalid("active_runtime_requirements_missing"))?;
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
        phase_restore(phase, previous, &owner.owner_id, layer.layer)?;
    let restore_key = key_for_slot(
        evidence.campaign,
        layer.layer,
        &identity,
        &runtime_digest,
        restore_slot,
    )?;
    validate_restore(&layer.restore, &restore_key, expected_archive.as_ref())?;
    if let Some(previous) = previous {
        let prior_layer = find_layer_receipt(Some(previous), &owner.owner_id, layer.layer)?;
        if prior_layer.runtime_identity.as_ref() != Some(runtime) {
            return Err(invalid("runtime_identity_changed_in_lineage"));
        }
    }
    let save_slot = save_slot(phase);
    let save_key = save_slot
        .map(|slot| {
            key_for_slot(
                evidence.campaign,
                layer.layer,
                &identity,
                &runtime_digest,
                slot,
            )
        })
        .transpose()?;
    let previous_state = previous
        .map(|previous| find_layer_receipt(Some(previous), &owner.owner_id, layer.layer))
        .transpose()?
        .and_then(|prior| prior.state_digest.as_deref());
    validate_save(
        &layer.save,
        save_key.as_deref(),
        state_digest,
        previous_state,
        producer,
    )
}

#[derive(Clone, Copy)]
pub(in crate::workflow::qualification_cache_lineage::receipt) struct LaneEvidence<'a> {
    campaign: &'a str,
    source_sha: &'a str,
    phase: QualificationPhase,
    lanes: &'a [QualificationCacheLaneReceipt],
    owners: &'a [QualificationCacheOwnerReceipt],
}

impl<'a> LaneEvidence<'a> {
    pub(in crate::workflow::qualification_cache_lineage::receipt) fn producer(
        evidence: &'a QualificationCacheProducerEvidence,
    ) -> Self {
        Self {
            campaign: &evidence.campaign,
            source_sha: &evidence.source_sha,
            phase: evidence.phase,
            lanes: &evidence.lanes,
            owners: &evidence.owners,
        }
    }

    pub(in crate::workflow::qualification_cache_lineage::receipt) fn receipt(
        receipt: &'a QualificationCacheReceipt,
    ) -> Self {
        Self {
            campaign: &receipt.campaign,
            source_sha: &receipt.source_sha,
            phase: receipt.phase,
            lanes: &receipt.lanes,
            owners: &receipt.owners,
        }
    }
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
