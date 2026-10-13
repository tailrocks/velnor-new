//! Native measurement delivery stays within an existing managed session.
use crate::measurement_reliability::{AdmissionClosed, Delivery, DeliveryFailure, EventKind};
use mbx_cache_core::{AdapterKind, AgentRequest, AgentResponse, MeasurementEvent};
use std::io::Write;

pub(super) fn ignore_sink(_event: MeasurementEvent) {}

pub(super) fn enroll(adapter: AdapterKind, kind: EventKind) -> Option<Delivery> {
    if !configured_measurements() {
        return None;
    }
    if crate::session::session_socket().is_none() {
        unavailable(adapter, kind, "session_unavailable");
        return None;
    }
    match Delivery::enroll(adapter, kind) {
        Ok(delivery) => delivery,
        Err(error) => {
            let reason = if error.downcast_ref::<AdmissionClosed>().is_some() {
                "late_admission_closed"
            } else {
                "enrollment_write_failed"
            };
            unavailable(adapter, kind, reason);
            None
        }
    }
}

fn configured_measurements() -> bool {
    std::env::var_os("MBX_STATS_REPORT_DIR").is_some()
}

pub(super) fn deliver(event: MeasurementEvent, delivery: Option<Delivery>) {
    let (adapter, kind) = match &event {
        MeasurementEvent::Invocation { adapter, .. } => (*adapter, EventKind::Invocation),
        MeasurementEvent::Process { adapter, .. } => (*adapter, EventKind::Process),
        MeasurementEvent::Output { adapter, .. } => (*adapter, EventKind::Output),
    };
    if !configured_measurements() {
        if delivery.is_some() {
            unavailable(adapter, kind, "report_scope_unavailable");
        }
        return;
    }
    if delivery.is_none() {
        unavailable(adapter, kind, "enrollment_missing");
    }
    let result = match crate::session::request_session_agent(&[AgentRequest::RecordMeasurement {
        event: event.clone(),
    }]) {
        Ok(Some(responses))
            if matches!(responses.as_slice(), [AgentResponse::MeasurementRecorded]) =>
        {
            delivery.map(|delivery| delivery.acknowledge(&event))
        }
        Ok(Some(_)) => {
            delivery.map(|delivery| delivery.fail(DeliveryFailure::UnexpectedAcknowledgement))
        }
        Ok(None) => {
            unavailable(adapter, kind, "session_unavailable");
            delivery.map(|delivery| delivery.fail(DeliveryFailure::NoSessionSocket))
        }
        Err(_) => delivery.map(|delivery| delivery.fail(DeliveryFailure::RequestFailed)),
    };
    if result.is_some_and(|result| result.is_err()) {
        unavailable(adapter, kind, "receipt_write_failed");
    }
}

fn unavailable(adapter: AdapterKind, kind: EventKind, reason: &str) {
    use crate::session::completed_report::{
        CORRELATION_ID_ENV, ROOT_SESSION_ID_ENV, SESSION_ID_ENV, valid_uuid, validate_correlation,
    };
    let session = std::env::var(SESSION_ID_ENV)
        .ok()
        .filter(|value| valid_uuid(value));
    let root = std::env::var(ROOT_SESSION_ID_ENV)
        .ok()
        .filter(|value| valid_uuid(value));
    let correlation = std::env::var(CORRELATION_ID_ENV)
        .ok()
        .filter(|value| validate_correlation(Some(value)).is_ok());
    let diagnostic = serde_json::json!({
        "schema_version": 1,
        "scope": "mbx_owned_adapters",
        "reason": reason,
        "adapter": adapter,
        "event_kind": kind,
        "session_id": session,
        "root_session_id": root,
        "caller_correlation": correlation,
    });
    if writeln!(
        std::io::stderr().lock(),
        "MBX_MEASUREMENT_UNAVAILABLE {diagnostic}"
    )
    .is_err()
    {
        log::debug!("measurement availability diagnostic could not be written");
    }
}
