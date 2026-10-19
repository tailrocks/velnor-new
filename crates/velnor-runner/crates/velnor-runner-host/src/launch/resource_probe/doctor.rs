//! Run one durable Doctor probe request.

use bollard::Docker;
use serde_json::Value;

use crate::error::HostError;
use crate::journal::{
    DoctorProbeOutcome, DoctorProbeState, DoctorProbeTicket, Journal, ProbePhase,
};

use super::ReleaseReadSecret;
use super::provider::ReleaseImageProvider;

pub(super) async fn run(
    docker: &Docker,
    journal: &Journal,
    secret: &ReleaseReadSecret,
    ticket: &DoctorProbeTicket,
) -> Result<Option<Value>, HostError> {
    let record = journal
        .doctor_probe_request_by_nonce(ticket.nonce())
        .await?
        .ok_or(HostError::Journal)?;
    if record.state() != DoctorProbeState::Queued {
        let child = record.child().ok_or(HostError::Journal)?;
        if matches!(child.phase, ProbePhase::Removed | ProbePhase::Aborted) {
            journal
                .finish_doctor_probe_request(ticket, &child.operation_id, DoctorProbeOutcome::Stale)
                .await?;
            return Ok(None);
        }
        super::lifecycle::recover_doctor_request(docker, journal, ticket).await?;
        return Ok(None);
    }
    let result = async {
        let token = secret.as_provider_token()?;
        let provider =
            ReleaseImageProvider::from_compiled_source(token).map_err(|_| HostError::Docker)?;
        super::lifecycle::collect_for_doctor(docker, journal, &provider, ticket).await
    }
    .await;
    match result {
        Ok(Some(observation)) => {
            let report = observation.doctor_report().ok_or(HostError::Identity)?;
            journal
                .finish_doctor_probe_request(
                    ticket,
                    observation.sample_generation_id(),
                    DoctorProbeOutcome::Completed,
                )
                .await?;
            Ok(Some(report))
        }
        Ok(None) => {
            fail_unstarted(journal, ticket).await?;
            Ok(None)
        }
        Err(error) => {
            record_failure(journal, ticket).await;
            Err(error)
        }
    }
}

async fn fail_unstarted(journal: &Journal, ticket: &DoctorProbeTicket) -> Result<(), HostError> {
    let record = journal
        .doctor_probe_request_by_nonce(ticket.nonce())
        .await?
        .ok_or(HostError::Journal)?;
    if record.state() == DoctorProbeState::Queued {
        journal.fail_queued_doctor_probe_request(ticket).await?;
    }
    Ok(())
}

async fn record_failure(journal: &Journal, ticket: &DoctorProbeTicket) {
    let Ok(Some(record)) = journal.doctor_probe_request_by_nonce(ticket.nonce()).await else {
        return;
    };
    let Some(child) = record.child() else {
        if record.state() == DoctorProbeState::Queued {
            let _ = journal.fail_queued_doctor_probe_request(ticket).await;
        }
        return;
    };
    let _ = journal
        .mark_doctor_probe_uncertain(ticket, &child.operation_id)
        .await;
}
