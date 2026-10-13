//! Ordered lookup helpers for bounded receipt chains.

use crate::errors::ContractError;
use crate::workflow::QualificationPhase;

use super::super::identity::{QualificationCacheLayer, QualificationCacheSlot};
use super::errors::invalid;
use super::types::{
    AdmissionNode, QualificationCacheBackendEntry, QualificationCacheBackendObservation,
    QualificationCacheLayerReceipt, QualificationCacheSaveActionResult,
};

pub(super) fn find_saved_entry(
    mut node: Option<&AdmissionNode>,
    matrix_key: &str,
    layer: QualificationCacheLayer,
    slot: QualificationCacheSlot,
) -> Result<QualificationCacheBackendEntry, ContractError> {
    let expected_phase = match slot {
        QualificationCacheSlot::K1 => QualificationPhase::Cold,
        QualificationCacheSlot::K2 => QualificationPhase::Warm,
        QualificationCacheSlot::K3 => QualificationPhase::UsefulDelta,
    };
    while let Some(current) = node {
        if current.receipt.phase == expected_phase {
            let observed = find_layer_receipt(Some(current), matrix_key, layer)?;
            return saved_entry(observed);
        }
        node = current.previous.as_deref();
    }
    Err(invalid("predecessor_slot_not_in_lineage"))
}

pub(super) fn find_latest_saved_entry(
    mut node: Option<&AdmissionNode>,
    matrix_key: &str,
    layer: QualificationCacheLayer,
) -> Result<(QualificationCacheSlot, QualificationCacheBackendEntry), ContractError> {
    while let Some(current) = node {
        if let Some(slot) = save_slot(current.receipt.phase) {
            let observed = find_layer_receipt(Some(current), matrix_key, layer)?;
            if observed.save.action == QualificationCacheSaveActionResult::Succeeded {
                return Ok((slot, saved_entry(observed)?));
            }
        }
        node = current.previous.as_deref();
    }
    Err(invalid("predecessor_archive_not_observed"))
}

fn saved_entry(
    layer: &QualificationCacheLayerReceipt,
) -> Result<QualificationCacheBackendEntry, ContractError> {
    match &layer.save.after {
        QualificationCacheBackendObservation::Found(entry) => Ok(entry.clone()),
        _ => Err(invalid("predecessor_archive_not_observed")),
    }
}

fn save_slot(phase: QualificationPhase) -> Option<QualificationCacheSlot> {
    match phase {
        QualificationPhase::Cold => Some(QualificationCacheSlot::K1),
        QualificationPhase::Warm => Some(QualificationCacheSlot::K2),
        QualificationPhase::UsefulDelta => Some(QualificationCacheSlot::K3),
        QualificationPhase::Third | QualificationPhase::Control => None,
    }
}

pub(super) fn find_layer_receipt<'a>(
    node: Option<&'a AdmissionNode>,
    matrix_key: &str,
    layer: QualificationCacheLayer,
) -> Result<&'a QualificationCacheLayerReceipt, ContractError> {
    let node = node.ok_or_else(|| invalid("predecessor_receipt_missing"))?;
    let lane = node
        .receipt
        .lanes
        .iter()
        .find(|lane| lane.matrix_key == matrix_key)
        .ok_or_else(|| invalid("predecessor_lane_missing"))?;
    lane.layers
        .iter()
        .find(|record| record.layer == layer)
        .ok_or_else(|| invalid("predecessor_layer_missing"))
}

pub(super) fn layer_applies(
    entry: &crate::workflow::MatrixEntry,
    layer: QualificationCacheLayer,
) -> bool {
    match (entry.stack_id.as_str(), layer) {
        ("rust", QualificationCacheLayer::CargoSources | QualificationCacheLayer::MiseTools)
        | ("tofu", QualificationCacheLayer::MiseTools | QualificationCacheLayer::TofuProviders) => {
            true
        }
        ("rust", QualificationCacheLayer::MbxObjects) => {
            entry.adapter_metadata["compile_driver"].as_str() == Some("mbx")
        }
        _ => false,
    }
}
