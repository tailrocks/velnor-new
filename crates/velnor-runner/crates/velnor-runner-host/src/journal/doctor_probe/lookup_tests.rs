use crate::error::HostError;
use crate::journal::{DoctorProbePrepareResult, ProbePhase};

use super::DoctorProbeState;
use super::tests::{Scratch, created_ticket, open, seed};

#[tokio::test]
async fn queued_ticket_resumes_after_restart_and_duplicate_readers_cannot_start_twice()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = open(&path).await?;
    let original = created_ticket(&journal, '1').await?;
    let request_id = original.request_id();
    drop(journal);

    let journal = open(&path).await?;
    let first = journal
        .active_doctor_probe_request()
        .await?
        .ok_or(HostError::Journal)?;
    let second = journal
        .active_doctor_probe_request()
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(first.request_id(), request_id);
    assert_eq!(second.request_id(), request_id);
    assert_eq!(first.state(), DoctorProbeState::Queued);
    assert_eq!(second.state(), DoctorProbeState::Queued);

    let ticket = first.ticket();
    let duplicate_ticket = second.ticket();
    let child = seed(&journal, '2').await?;
    let prepared = journal
        .prepare_probe_for_doctor_request(&ticket, child.clone())
        .await?;
    assert!(matches!(prepared, DoctorProbePrepareResult::Prepared(_)));
    let replayed = journal
        .prepare_probe_for_doctor_request(&duplicate_ticket, child.clone())
        .await?;
    assert!(matches!(
        replayed,
        DoctorProbePrepareResult::AlreadyLinked(row) if row.operation_id == child.operation_id
    ));

    journal
        .transition_probe(
            &child.operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    assert!(
        journal
            .transition_probe(
                &child.operation_id,
                ProbePhase::Prepared,
                ProbePhase::CreateRequested,
            )
            .await
            .is_err()
    );
    let active = journal
        .active_doctor_probe_request()
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(active.state(), DoctorProbeState::Running);
    assert_eq!(
        active.child().map(|row| row.operation_id.as_str()),
        Some(child.operation_id.as_str())
    );

    Ok(())
}
