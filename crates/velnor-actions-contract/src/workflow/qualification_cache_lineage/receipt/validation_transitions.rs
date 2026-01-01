//! Cache observation checks for one phase transition.

use crate::errors::ContractError;
use crate::workflow::QualificationPhase;

use super::super::super::super::identity::{QualificationCacheLayer, QualificationCacheSlot};
use super::super::super::errors::invalid;
use super::super::super::lineage::{find_latest_saved_entry, find_saved_entry};
use super::super::super::types::{
    AdmissionNode, QualificationCacheBackendEntry, QualificationCacheBackendObservation,
    QualificationCacheLayerReceipt, QualificationCacheReceipt, QualificationCacheRestore,
    QualificationCacheRestoreResult, QualificationCacheRunMetadata, QualificationCacheSave,
    QualificationCacheSaveActionResult,
};

pub(super) fn validate_disabled_layer(
    layer: &QualificationCacheLayerReceipt,
) -> Result<(), ContractError> {
    if layer.active
        || layer.runtime_identity.is_some()
        || layer.state_digest.is_some()
        || layer.restore.result != QualificationCacheRestoreResult::Disabled
        || layer.restore.requested_key.is_some()
        || layer.restore.matched_key.is_some()
        || layer.restore.matched_cache != QualificationCacheBackendObservation::NotQueried
        || layer.save.action != QualificationCacheSaveActionResult::Disabled
        || layer.save.requested_key.is_some()
        || layer.save.before != QualificationCacheBackendObservation::NotQueried
        || layer.save.after != QualificationCacheBackendObservation::NotQueried
    {
        return Err(invalid("disabled_layer_observed_access"));
    }
    Ok(())
}

pub(super) fn phase_restore(
    phase: QualificationPhase,
    previous: Option<&AdmissionNode>,
    matrix_key: &str,
    layer: QualificationCacheLayer,
) -> Result<
    (
        QualificationCacheSlot,
        Option<QualificationCacheBackendEntry>,
    ),
    ContractError,
> {
    match phase {
        QualificationPhase::Cold => Ok((QualificationCacheSlot::K1, None)),
        QualificationPhase::Warm => Ok((
            QualificationCacheSlot::K1,
            Some(find_saved_entry(
                previous,
                matrix_key,
                layer,
                QualificationCacheSlot::K1,
            )?),
        )),
        QualificationPhase::Third | QualificationPhase::UsefulDelta => {
            let found = find_latest_saved_entry(previous, matrix_key, layer)?;
            Ok((found.0, Some(found.1)))
        }
        QualificationPhase::Control => Err(invalid("disabled_phase_has_no_restore")),
    }
}

pub(super) fn save_slot(phase: QualificationPhase) -> Option<QualificationCacheSlot> {
    match phase {
        QualificationPhase::Cold => Some(QualificationCacheSlot::K1),
        QualificationPhase::Warm => Some(QualificationCacheSlot::K2),
        QualificationPhase::UsefulDelta => Some(QualificationCacheSlot::K3),
        QualificationPhase::Third | QualificationPhase::Control => None,
    }
}

pub(super) fn validate_restore(
    restore: &QualificationCacheRestore,
    requested_key: &str,
    expected: Option<&QualificationCacheBackendEntry>,
) -> Result<(), ContractError> {
    if restore.requested_key.as_deref() != Some(requested_key) {
        return Err(invalid("restore_requested_key_mismatch"));
    }
    match (expected, &restore.matched_cache) {
        (None, QualificationCacheBackendObservation::Absent)
            if restore.matched_key.is_none()
                && restore.result == QualificationCacheRestoreResult::Miss =>
        {
            Ok(())
        }
        (Some(expected), QualificationCacheBackendObservation::Found(actual))
            if restore.matched_key.as_deref() == Some(requested_key)
                && restore.result == QualificationCacheRestoreResult::Hit
                && expected.key == requested_key
                && actual == expected =>
        {
            Ok(())
        }
        _ => Err(invalid("restore_result_or_cache_object_mismatch")),
    }
}

pub(super) fn validate_save(
    save: &QualificationCacheSave,
    expected_key: Option<&str>,
    current_state: &str,
    previous_state: Option<&str>,
    metadata: &QualificationCacheRunMetadata,
    receipt: &QualificationCacheReceipt,
) -> Result<(), ContractError> {
    let Some(key) = expected_key else {
        return if save.action == QualificationCacheSaveActionResult::Disabled
            && save.requested_key.is_none()
            && save.before == QualificationCacheBackendObservation::NotQueried
            && save.after == QualificationCacheBackendObservation::NotQueried
        {
            Ok(())
        } else {
            Err(invalid("unexpected_save"))
        };
    };
    if previous_state == Some(current_state) {
        return if save.action == QualificationCacheSaveActionResult::NotRequired
            && save.requested_key.is_none()
            && save.before == QualificationCacheBackendObservation::NotQueried
            && save.after == QualificationCacheBackendObservation::NotQueried
        {
            Ok(())
        } else {
            Err(invalid("unchanged_layer_was_rewritten"))
        };
    }
    if save.requested_key.as_deref() != Some(key)
        || save.action != QualificationCacheSaveActionResult::Succeeded
    {
        return Err(invalid("save_action_failed_or_wrong_key"));
    }
    let QualificationCacheBackendObservation::Found(after) = &save.after else {
        return Err(invalid("archive_not_observed"));
    };
    if save.before != QualificationCacheBackendObservation::Absent
        || after.id == 0
        || after.key != key
        || after.git_ref != metadata.git_ref
        || after.size_bytes == 0
        || receipt.run.run_id != metadata.run.run_id
    {
        return Err(invalid("archive_creation_not_proven"));
    }
    Ok(())
}

pub(super) fn validate_useful_delta_progress(
    node: &AdmissionNode,
    previous: Option<&AdmissionNode>,
) -> Result<(), ContractError> {
    let previous = previous.ok_or_else(|| invalid("useful_delta_missing_third_receipt"))?;
    let persisted = node.receipt.lanes.iter().any(|current| {
        previous
            .receipt
            .lanes
            .iter()
            .find(|prior| prior.matrix_key == current.matrix_key)
            .is_some_and(|prior| lane_delta_persisted(current, prior))
    });
    if persisted {
        Ok(())
    } else {
        Err(invalid("useful_delta_not_observed_and_persisted"))
    }
}

fn lane_delta_persisted(
    current: &super::super::super::types::QualificationCacheLaneReceipt,
    previous: &super::super::super::types::QualificationCacheLaneReceipt,
) -> bool {
    current.closure_digest != previous.closure_digest
        && current.useful_state_digest != previous.useful_state_digest
        && current.layers.iter().any(|layer| {
            previous
                .layers
                .iter()
                .find(|prior| prior.layer == layer.layer)
                .is_some_and(|prior| {
                    layer.active
                        && layer.state_digest != prior.state_digest
                        && layer.save.action == QualificationCacheSaveActionResult::Succeeded
                        && matches!(
                            &layer.save.after,
                            QualificationCacheBackendObservation::Found(_)
                        )
                })
        })
}

pub(super) fn validate_third_observations(
    node: &AdmissionNode,
    previous: Option<&AdmissionNode>,
) -> Result<(), ContractError> {
    let previous = previous.ok_or_else(|| invalid("third_missing_warm_receipt"))?;
    for current in &node.receipt.lanes {
        let prior = previous
            .receipt
            .lanes
            .iter()
            .find(|lane| lane.matrix_key == current.matrix_key)
            .ok_or_else(|| invalid("third_missing_warm_lane"))?;
        if current.closure_digest != prior.closure_digest
            || current.useful_state_digest != prior.useful_state_digest
        {
            return Err(invalid("third_run_changed_useful_state"));
        }
        for layer in &current.layers {
            let prior_layer = prior
                .layers
                .iter()
                .find(|candidate| candidate.layer == layer.layer)
                .ok_or_else(|| invalid("third_missing_warm_layer"))?;
            if layer.active && layer.state_digest != prior_layer.state_digest {
                return Err(invalid("third_layer_state_changed"));
            }
        }
    }
    Ok(())
}
