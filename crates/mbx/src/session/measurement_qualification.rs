//! Supported measurement reconciliation and native completion qualification.
//!
//! A receipt authenticates delivery of one enrolled event. It does not prove
//! all emitters were enrolled or establish the owning Cargo artifact stream.
//! Terminal payloads verify direct owning aggregates, not dispatch closure. No
//! parsed input can mint native authority or prove zero process work.
use super::completed_report::{valid_uuid, validate_correlation};
use mbx_cache_core::{
    CompletedMeasurement, MeasurementCoverage, MeasurementCoverageReason,
    MeasurementCoverageStatus, PackageOrigin,
};
use serde::Serialize;
use std::collections::BTreeSet;

#[path = "qualification_native.rs"]
mod native;
pub(crate) use native::evaluate_native;
#[path = "qualification_admission.rs"]
mod admission;
#[path = "qualification_artifacts.rs"]
mod artifacts;
#[path = "qualification_authority.rs"]
mod authority;

#[path = "qualification_inputs.rs"]
mod inputs;
#[path = "qualification_receipts.rs"]
mod receipts;
use inputs::{MAX_REPORT_BYTES, Snapshot};

/// Exact unavailable or inconsistent proof needed by the qualification gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QualificationReason {
    ReportUnavailable,
    ReportMalformed,
    UnsupportedReportSchema,
    InvalidSessionIdentity,
    QualifiedSourceAuthorityUnavailable,
    ManagedDispatchClosureUnavailable,
    CargoArtifactClosureUnavailable,
    EventPayloadFidelityUnavailable,
    UnitProvenanceAuthorityUnavailable,
    DiagnosticCaptureClosureUnavailable,
    DeliveryReceiptUnavailable,
    DeliveryReceiptMalformed,
    DeliveryIdentityMismatch,
    DuplicateDeliveryStage,
    DeliveryFailed,
    DeliveryIncomplete,
    AcknowledgmentWithoutEnrollment,
    AggregateCountMismatch,
    AggregateOverflow,
    UnitAggregateMismatch,
    UnitNumericFidelityUnavailable,
    InvalidTerminalEvent,
    TerminalDigestMismatch,
    UnavailableDiagnostic,
    DiagnosticMalformed,
    UnitAttributionIncomplete,
    UnitAttributionTruncated,
    PackageSnapshotUnavailable,
    ReceiptDirectoryUnavailable,
    DispatchSnapshotPinningUnavailable,
    DispatchIdentityChanged,
    ManagedDispatchAdmissionClosureUnavailable,
    NativeUnitArtifactBindingUnavailable,
    AdmissionClosureIncomplete,
    AdmissionReceiptMismatch,
    DynamicAdmissionEvidenceUnavailable,
    OutputAttributionTruncated,
}

/// Observed owning totals survive unavailable qualification.
#[derive(Debug, Serialize)]
pub(crate) struct Qualification {
    pub schema_version: u8,
    pub local_status: LocalEvidenceStatus,
    pub coverage: MeasurementCoverage,
    pub reasons: Vec<QualificationReason>,
    pub observed_measurement: Option<CompletedMeasurement>,
    /// Separate closed-admission evidence; never broad task-wide coverage.
    pub admitted_evidence: Option<AdmittedEvidence>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdmittedEvidence {
    pub scope: AdmissionScope,
    pub lifetime: AdmissionLifetime,
    pub ledger_inventory_sha256: String,
    pub adapters:
        std::collections::BTreeMap<mbx_cache_core::AdapterKind, mbx_cache_core::AdapterMeasurement>,
    pub measurement_package_generation: u64,
    /// Unverified or excluded routes; absence of their events proves no zero.
    pub excluded_routes: Vec<mbx_cache_core::AdapterKind>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AdmissionScope {
    MbxSessionAdmissions,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AdmissionLifetime {
    AcceptedBeforeClose,
}

/// Native integrity is independent of external distribution qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LocalEvidenceStatus {
    LocalEvidenceComplete,
    Unknown,
    Unverified,
}

/// Evaluate current supported artifacts without inventing positive authority.
/// Diagnostics must be raw JSON from the documented unavailable prefix; the
/// caller retains unknown/truncated log availability outside this gate.
pub(crate) fn evaluate(
    completed_report: &[u8],
    delivery_receipts: &[Vec<u8>],
    unavailable_diagnostics: &[Vec<u8>],
) -> Qualification {
    let mut reasons = BTreeSet::from([
        QualificationReason::QualifiedSourceAuthorityUnavailable,
        QualificationReason::ManagedDispatchClosureUnavailable,
        QualificationReason::CargoArtifactClosureUnavailable,
        QualificationReason::UnitProvenanceAuthorityUnavailable,
        QualificationReason::DiagnosticCaptureClosureUnavailable,
    ]);
    let snapshot = decode_report(completed_report, &mut reasons);
    let mut inconsistent = false;
    if let Some(snapshot) = &snapshot {
        inconsistent = receipts::check(
            snapshot,
            delivery_receipts,
            unavailable_diagnostics,
            &mut reasons,
        );
        check_attribution(&snapshot.statistics.measurement, &mut reasons);
    }
    let mut coverage = MeasurementCoverage::default();
    if inconsistent {
        coverage.status = MeasurementCoverageStatus::Unverified;
        coverage.reason = Some(if reasons.contains(&QualificationReason::DeliveryFailed) {
            MeasurementCoverageReason::DeliveryFailed
        } else {
            MeasurementCoverageReason::DeliveryIncomplete
        });
    }
    let observed_measurement = snapshot.map(|report| {
        let mut measurement = report.statistics.measurement;
        measurement.coverage = coverage;
        measurement
    });
    Qualification {
        schema_version: 1,
        local_status: if inconsistent {
            LocalEvidenceStatus::Unverified
        } else {
            LocalEvidenceStatus::Unknown
        },
        coverage,
        reasons: reasons.into_iter().collect(),
        observed_measurement,
        admitted_evidence: None,
    }
}

fn decode_report(raw: &[u8], reasons: &mut BTreeSet<QualificationReason>) -> Option<Snapshot> {
    if raw.is_empty() {
        reasons.insert(QualificationReason::ReportUnavailable);
        return None;
    }
    if raw.len() > MAX_REPORT_BYTES {
        reasons.insert(QualificationReason::ReportMalformed);
        return None;
    }
    let Ok(snapshot) = serde_json::from_slice::<Snapshot>(raw) else {
        reasons.insert(QualificationReason::ReportMalformed);
        return None;
    };
    if snapshot.schema_version != 1 || !snapshot.completed {
        reasons.insert(QualificationReason::UnsupportedReportSchema);
        return None;
    }
    let identity = &snapshot.identity;
    let ancestry = match identity.parent_session_id.as_deref() {
        Some(parent) => {
            valid_uuid(parent)
                && parent != identity.session_id
                && identity.session_id != identity.root_session_id
        }
        None => identity.session_id == identity.root_session_id,
    };
    if !valid_uuid(&identity.session_id)
        || !valid_uuid(&identity.root_session_id)
        || !ancestry
        || validate_correlation(identity.caller_correlation.as_deref()).is_err()
        || snapshot.mbx_version.is_empty()
        || snapshot.source_base_version.is_empty()
    {
        reasons.insert(QualificationReason::InvalidSessionIdentity);
        return None;
    }
    Some(snapshot)
}

fn check_attribution(
    measurement: &CompletedMeasurement,
    reasons: &mut BTreeSet<QualificationReason>,
) {
    for adapter in measurement.adapters.values() {
        if adapter.omitted_output_events != 0
            || adapter
                .units
                .iter()
                .any(|unit| unit.omitted_output_events != 0)
        {
            reasons.insert(QualificationReason::OutputAttributionTruncated);
        }
        if adapter.omitted_unit_events != 0 {
            reasons.insert(QualificationReason::UnitAttributionTruncated);
            reasons.insert(QualificationReason::UnitNumericFidelityUnavailable);
        }
        for unit in &adapter.units {
            if unit.identity.as_ref().is_none_or(|identity| {
                identity.origin == PackageOrigin::Unknown || identity.package_id.is_none()
            }) {
                reasons.insert(QualificationReason::UnitAttributionIncomplete);
            }
        }
        if (!adapter.invocations.is_empty() || !adapter.subprocesses.is_empty())
            && adapter.units.is_empty()
        {
            reasons.insert(QualificationReason::UnitAttributionIncomplete);
        }
    }
}

#[cfg(test)]
#[path = "measurement_qualification_tests.rs"]
mod tests;
