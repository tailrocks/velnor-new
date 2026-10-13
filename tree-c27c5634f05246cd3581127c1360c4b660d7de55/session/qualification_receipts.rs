//! Receipt enrollment, identity and terminal-payload reconciliation.
use super::{QualificationReason, inputs::*};
use mbx_cache_core::{AdapterMeasurement, MeasurementEvent};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[path = "qualification_aggregate.rs"]
mod aggregate;

#[derive(Default)]
struct Stages {
    coordinate: Option<(mbx_cache_core::AdapterKind, EventKind)>,
    attempt: bool,
    acknowledgment: bool,
    failure: bool,
}

pub(super) fn check(
    snapshot: &Snapshot,
    raw: &[Vec<u8>],
    diagnostics: &[Vec<u8>],
    reasons: &mut BTreeSet<QualificationReason>,
) -> bool {
    if raw.is_empty() {
        reasons.insert(QualificationReason::DeliveryReceiptUnavailable);
    }
    let mut invalid = false;
    let mut stages = BTreeMap::new();
    let mut totals = BTreeMap::new();
    if raw.len() > MAX_RECEIPTS {
        reasons.insert(QualificationReason::DeliveryReceiptMalformed);
        return true;
    }
    for bytes in raw {
        if let Err(reason) = check_one(snapshot, bytes, &mut stages, &mut totals) {
            invalid = true;
            reasons.insert(reason);
        }
    }
    for state in stages.values() {
        if state.failure || !state.attempt || !state.acknowledgment {
            invalid = true;
            reasons.insert(if state.failure {
                QualificationReason::DeliveryFailed
            } else if !state.attempt {
                QualificationReason::AcknowledgmentWithoutEnrollment
            } else {
                QualificationReason::DeliveryIncomplete
            });
        }
    }
    if let Err(reason) = aggregate::check(&snapshot.statistics.measurement.adapters, &totals) {
        invalid = true;
        reasons.insert(reason);
    }
    invalid | check_diagnostics(snapshot, diagnostics, reasons)
}

fn check_one(
    snapshot: &Snapshot,
    raw: &[u8],
    states: &mut BTreeMap<String, Stages>,
    totals: &mut BTreeMap<mbx_cache_core::AdapterKind, AdapterMeasurement>,
) -> Result<(), QualificationReason> {
    if raw.len() > MAX_RECEIPT_BYTES {
        return Err(QualificationReason::DeliveryReceiptMalformed);
    }
    let receipt: Receipt =
        serde_json::from_slice(raw).map_err(|_| QualificationReason::DeliveryReceiptMalformed)?;
    if receipt.schema_version != 1
        || receipt.event_id.len() != 32
        || !receipt
            .event_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(QualificationReason::DeliveryReceiptMalformed);
    }
    if receipt.identity != snapshot.identity
        || receipt.mbx_version != snapshot.mbx_version
        || receipt.source_base_version != snapshot.source_base_version
    {
        return Err(QualificationReason::DeliveryIdentityMismatch);
    }
    let state = states.entry(receipt.event_id).or_default();
    let coordinate = (receipt.adapter, receipt.event_kind);
    if state
        .coordinate
        .is_some_and(|previous| previous != coordinate)
    {
        return Err(QualificationReason::DeliveryIdentityMismatch);
    }
    state.coordinate = Some(coordinate);
    let present = match receipt.delivery {
        DeliveryStage::Attempted => &mut state.attempt,
        DeliveryStage::Failed { reason } => {
            if !matches!(
                reason.as_str(),
                "no_session_socket" | "request_failed" | "unexpected_acknowledgement"
            ) {
                return Err(QualificationReason::DeliveryReceiptMalformed);
            }
            &mut state.failure
        }
        DeliveryStage::Acknowledged {
            event,
            event_sha256,
        } => {
            if state.acknowledgment {
                return Err(QualificationReason::DuplicateDeliveryStage);
            }
            let event = event.ok_or(QualificationReason::EventPayloadFidelityUnavailable)?;
            check_event(
                receipt.adapter,
                receipt.event_kind,
                &event,
                event_sha256.as_deref(),
            )?;
            aggregate::add(totals, &event)?;
            &mut state.acknowledgment
        }
    };
    if *present {
        return Err(QualificationReason::DuplicateDeliveryStage);
    }
    *present = true;
    Ok(())
}

fn check_event(
    adapter: mbx_cache_core::AdapterKind,
    kind: EventKind,
    event: &MeasurementEvent,
    digest: Option<&str>,
) -> Result<(), QualificationReason> {
    let (actual_adapter, actual_kind) = match event {
        MeasurementEvent::Invocation { adapter, .. } => (*adapter, EventKind::Invocation),
        MeasurementEvent::Process { adapter, .. } => (*adapter, EventKind::Process),
        MeasurementEvent::Output { adapter, .. } => (*adapter, EventKind::Output),
    };
    if actual_adapter != adapter || actual_kind != kind {
        return Err(QualificationReason::InvalidTerminalEvent);
    }
    let bytes = serde_json::to_vec(event).map_err(|_| QualificationReason::InvalidTerminalEvent)?;
    if digest != Some(hex::encode(Sha256::digest(bytes)).as_str()) {
        return Err(QualificationReason::TerminalDigestMismatch);
    }
    Ok(())
}

fn check_diagnostics(
    snapshot: &Snapshot,
    raw: &[Vec<u8>],
    reasons: &mut BTreeSet<QualificationReason>,
) -> bool {
    if raw.len() > MAX_RECEIPTS {
        reasons.insert(QualificationReason::DiagnosticMalformed);
        return true;
    }
    for bytes in raw {
        let diagnostic = (bytes.len() <= MAX_RECEIPT_BYTES)
            .then(|| serde_json::from_slice::<UnavailableDiagnostic>(bytes).ok())
            .flatten();
        if diagnostic.is_none_or(|diagnostic| {
            diagnostic.schema_version != 1
                || diagnostic.scope != "mbx_owned_adapters"
                || !matches!(
                    diagnostic.reason.as_str(),
                    "enrollment_write_failed"
                        | "receipt_write_failed"
                        | "session_unavailable"
                        | "report_scope_unavailable"
                        | "enrollment_missing"
                )
                || diagnostic
                    .session_id
                    .as_ref()
                    .is_some_and(|id| id != &snapshot.identity.session_id)
                || diagnostic
                    .root_session_id
                    .as_ref()
                    .is_some_and(|id| id != &snapshot.identity.root_session_id)
                || diagnostic.caller_correlation != snapshot.identity.caller_correlation
        }) {
            reasons.insert(QualificationReason::DiagnosticMalformed);
        } else {
            reasons.insert(QualificationReason::UnavailableDiagnostic);
        }
    }
    !raw.is_empty()
}
