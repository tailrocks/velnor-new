//! Read only the supported completed-report and immutable delivery contracts.
use mbx_cache_core::{AdapterKind, CompletedMeasurement, MeasurementEvent};
use serde::Deserialize;

pub(super) const MAX_REPORT_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_RECEIPT_BYTES: usize = 16 * 1024;
pub(super) const MAX_RECEIPTS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct ReportIdentity {
    pub session_id: String,
    pub root_session_id: String,
    pub parent_session_id: Option<String>,
    pub caller_correlation: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Snapshot {
    pub schema_version: u8,
    pub completed: bool,
    pub mbx_version: String,
    pub source_base_version: String,
    pub identity: ReportIdentity,
    pub statistics: Statistics,
}

#[derive(Debug, Deserialize)]
pub(super) struct Statistics {
    pub measurement: CompletedMeasurement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EventKind {
    Invocation,
    Process,
    Output,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum DeliveryStage {
    Attempted,
    Acknowledged {
        event: Option<MeasurementEvent>,
        event_sha256: Option<String>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Deserialize)]
pub(super) struct Receipt {
    pub schema_version: u8,
    pub mbx_version: String,
    pub source_base_version: String,
    pub identity: ReportIdentity,
    pub event_id: String,
    pub adapter: AdapterKind,
    pub event_kind: EventKind,
    pub delivery: DeliveryStage,
}

#[derive(Debug, Deserialize)]
pub(super) struct UnavailableDiagnostic {
    pub schema_version: u8,
    pub scope: String,
    pub reason: String,
    pub session_id: Option<String>,
    pub root_session_id: Option<String>,
    pub caller_correlation: Option<String>,
}
