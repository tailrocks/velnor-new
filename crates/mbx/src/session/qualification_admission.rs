//! Original sealed admission inventory versus immutable event ACKs.
use super::{
    QualificationReason,
    inputs::{DeliveryStage, EventKind, Receipt},
};
use crate::dispatch_admission::{AdmissionClosure, AdmissionKind, TerminalOutcome};
use std::collections::BTreeMap;

pub(super) fn agrees(
    closure: &AdmissionClosure,
    receipts: &[Vec<u8>],
) -> Result<(), QualificationReason> {
    if !closure.closed_successfully()
        || closure.scope() != "mbx_session_admissions"
        || closure.lifetime() != "accepted_before_close"
    {
        return Err(QualificationReason::AdmissionClosureIncomplete);
    }
    let mut acknowledgments = BTreeMap::new();
    for raw in receipts {
        let receipt: Receipt = serde_json::from_slice(raw)
            .map_err(|_| QualificationReason::DeliveryReceiptMalformed)?;
        if let DeliveryStage::Acknowledged {
            event_sha256: Some(digest),
            ..
        } = receipt.delivery
        {
            if acknowledgments
                .insert(
                    receipt.event_id,
                    (receipt.adapter, receipt.event_kind, digest),
                )
                .is_some()
            {
                return Err(QualificationReason::DuplicateDeliveryStage);
            }
        }
    }
    let mut accepted = 0usize;
    for entry in closure.accepted_entries() {
        let kind = match entry.kind {
            AdmissionKind::Invocation => EventKind::Invocation,
            AdmissionKind::Process => EventKind::Process,
            AdmissionKind::Output => EventKind::Output,
            AdmissionKind::BuildScriptProvision | AdmissionKind::CcSelection => {
                return Err(QualificationReason::DynamicAdmissionEvidenceUnavailable);
            }
        };
        let Some(TerminalOutcome::Acknowledged { event_sha256 }) = &entry.terminal else {
            return Err(QualificationReason::AdmissionClosureIncomplete);
        };
        if acknowledgments.get(&entry.event_id)
            != Some(&(entry.adapter, kind, event_sha256.clone()))
        {
            return Err(QualificationReason::AdmissionReceiptMismatch);
        }
        accepted = accepted
            .checked_add(1)
            .ok_or(QualificationReason::AggregateOverflow)?;
    }
    if accepted != acknowledgments.len() {
        return Err(QualificationReason::AdmissionReceiptMismatch);
    }
    Ok(())
}

/// Apply the original closed token; parsed JSON never recreates this input.
pub(super) fn apply(
    result: &mut super::Qualification,
    reasons: &mut std::collections::BTreeSet<QualificationReason>,
    identity: &crate::session::completed_report::SessionIdentity,
    measurement: &mbx_cache_core::CompletedMeasurement,
    dispatch: Option<&crate::dispatch_identity::NativeDispatchWitness>,
    admission: Option<&AdmissionClosure>,
    receipts: &[Vec<u8>],
) {
    if let Some(closure) = admission {
        if closure.identity_matches(&identity.session_id, &identity.root_session_id) {
            match agrees(closure, receipts) {
                Ok(()) => {
                    reasons
                        .remove(&QualificationReason::ManagedDispatchAdmissionClosureUnavailable);
                    reasons.remove(&QualificationReason::DeliveryReceiptUnavailable);
                    if let (Some(witness), Some(generation)) =
                        (dispatch, measurement.measurement_package_generation)
                    {
                        result.admitted_evidence = Some(super::AdmittedEvidence {
                            scope: super::AdmissionScope::MbxSessionAdmissions,
                            lifetime: super::AdmissionLifetime::AcceptedBeforeClose,
                            ledger_inventory_sha256: closure.ledger_inventory_sha256().to_owned(),
                            adapters: measurement.adapters.clone(),
                            measurement_package_generation: generation,
                            excluded_routes: witness.excluded_routes(),
                        });
                    }
                }
                Err(reason) => {
                    reasons.insert(reason);
                    if reason != QualificationReason::DynamicAdmissionEvidenceUnavailable {
                        result.local_status = super::LocalEvidenceStatus::Unverified;
                        result.coverage.status =
                            mbx_cache_core::MeasurementCoverageStatus::Unverified;
                        result.coverage.reason =
                            Some(mbx_cache_core::MeasurementCoverageReason::DeliveryIncomplete);
                    }
                }
            }
        } else {
            reasons.insert(QualificationReason::AdmissionReceiptMismatch);
        }
    }
}
