use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::error::HostError;
use crate::journal::{
    CapacityLimits, DoctorProbeOutcome, DoctorProbePrepareResult, DoctorProbeStartResult, Journal,
    OperationalCapacityLimits, ProbePhase, ProbeSeed,
};
use crate::launch::VerifiedProbeCleanup;

use super::DoctorProbeState;

pub(super) const ENGINE_ID: &str = "probe-engine";
pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-doctor-probe-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    pub(super) fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) async fn open(path: &std::path::Path) -> Result<Journal, HostError> {
    let journal = Journal::open(path).await?;
    journal.bind_engine(ENGINE_ID).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 4 * 1024 * 1024 * 1024,
            },
            OperationalCapacityLimits {
                cpu_millicores: 100,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    Ok(journal)
}

pub(super) async fn seed(journal: &Journal, operation: char) -> Result<ProbeSeed, HostError> {
    let operation_id = operation.to_string().repeat(32);
    Ok(ProbeSeed {
        operation_name: format!("velnor-resource-probe-{operation_id}"),
        operation_id,
        instance_id: journal.instance_id().await?,
        engine_id: ENGINE_ID.to_owned(),
        docker_root_digest: "a".repeat(64),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        projection_digest: "e".repeat(64),
        purpose: crate::journal::ResourceProbePurpose::Capacity,
        request_sha256: "5d76d86293261804706a16a73e0491cf4caa859f1b876cda706e568592b85cdf"
            .to_owned(),
    })
}

pub(super) async fn created_ticket(
    journal: &Journal,
    nonce: char,
) -> Result<super::DoctorProbeTicket, HostError> {
    match journal
        .start_doctor_probe_request(&nonce.to_string().repeat(32), ENGINE_ID, &"a".repeat(64))
        .await?
    {
        DoctorProbeStartResult::Created(ticket) => Ok(ticket),
        DoctorProbeStartResult::Replayed(_) | DoctorProbeStartResult::Busy { .. } => {
            Err(HostError::Journal)
        }
    }
}

#[tokio::test]
async fn ticket_nonce_is_idempotent_and_only_one_request_can_be_active() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = open(&scratch.file()).await?;
    let nonce = "1".repeat(32);
    let first = journal
        .start_doctor_probe_request(&nonce, ENGINE_ID, &"a".repeat(64))
        .await?;
    let DoctorProbeStartResult::Created(ticket) = first else {
        return Err(HostError::Journal);
    };
    let replay = journal
        .start_doctor_probe_request(&nonce, ENGINE_ID, &"a".repeat(64))
        .await?;
    let DoctorProbeStartResult::Replayed(replayed) = replay else {
        return Err(HostError::Journal);
    };
    assert_eq!(ticket.request_id(), replayed.request_id());
    assert!(
        journal
            .start_doctor_probe_request(&nonce, "other-engine", &"a".repeat(64))
            .await
            .is_err()
    );

    let next = journal
        .start_doctor_probe_request(&"2".repeat(32), ENGINE_ID, &"a".repeat(64))
        .await?;
    assert!(matches!(
        next,
        DoctorProbeStartResult::Busy {
            active_request_id
        } if active_request_id == ticket.request_id()
    ));
    let status = journal
        .doctor_probe_request_by_nonce(&nonce)
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(status.state(), DoctorProbeState::Queued);
    assert!(status.child().is_none());
    let active = journal
        .active_doctor_probe_request()
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(active.request_id(), ticket.request_id());
    assert_eq!(active.state(), DoctorProbeState::Queued);
    assert!(active.child().is_none());
    assert!(
        journal
            .prepare_probe(seed(&journal, '9').await?)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn child_link_and_probe_lease_commit_once_and_exact_cleanup_finishes_ticket()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = open(&scratch.file()).await?;
    let ticket = created_ticket(&journal, '3').await?;
    let child = seed(&journal, '4').await?;
    let prepared = journal
        .prepare_probe_for_doctor_request(&ticket, child.clone())
        .await?;
    let DoctorProbePrepareResult::Prepared(row) = prepared else {
        return Err(HostError::Journal);
    };
    assert_eq!(row.phase, ProbePhase::Prepared);
    assert_eq!(row.lease_state, crate::journal::ProbeLeaseState::Reserved);
    assert_eq!(
        journal.charged_header_capacity().await?,
        0,
        "probe charge is separate from Scale Set header capacity"
    );

    let replay = journal
        .prepare_probe_for_doctor_request(&ticket, child.clone())
        .await?;
    let DoctorProbePrepareResult::AlreadyLinked(replayed) = replay else {
        return Err(HostError::Journal);
    };
    assert_eq!(replayed.operation_id, child.operation_id);
    let mut changed = child.clone();
    changed.projection_digest = "f".repeat(64);
    assert!(
        journal
            .prepare_probe_for_doctor_request(&ticket, changed)
            .await
            .is_err()
    );
    assert!(
        journal
            .prepare_probe_for_doctor_request(&ticket, seed(&journal, '5').await?)
            .await
            .is_err()
    );
    assert!(journal.active_probe().await?.is_none());

    let recovery = journal.begin_doctor_probe_recovery(&ticket).await?;
    assert_eq!(recovery.operation_id, child.operation_id);
    journal.abort_prepared_probe(&child.operation_id).await?;
    journal
        .finish_doctor_probe_request(&ticket, &child.operation_id, DoctorProbeOutcome::Failed)
        .await?;
    let status = journal
        .doctor_probe_request_by_nonce(ticket.nonce())
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(status.state(), DoctorProbeState::Failed);
    assert_eq!(
        status.child().map(|row| row.phase),
        Some(ProbePhase::Aborted)
    );
    assert!(journal.active_doctor_probe_request().await?.is_none());
    Ok(())
}

async fn finish_ticket(
    journal: &Journal,
    nonce: char,
    operation: char,
    container: char,
) -> Result<(), HostError> {
    let ticket = created_ticket(journal, nonce).await?;
    let child = seed(journal, operation).await?;
    journal
        .prepare_probe_for_doctor_request(&ticket, child.clone())
        .await?;
    journal
        .transition_probe(
            &child.operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    let container_id = container.to_string().repeat(64);
    journal
        .bind_probe_container(&child.operation_id, &container_id)
        .await?;
    journal
        .transition_probe(
            &child.operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::RemoveRequested,
        )
        .await?;
    journal
        .retire_probe(VerifiedProbeCleanup::test_only(
            child.operation_id.clone(),
            ENGINE_ID.to_owned(),
            child.docker_root_digest.clone(),
            ProbePhase::RemoveRequested,
            child.operation_name.clone(),
            Some(container_id),
        ))
        .await?;
    journal
        .finish_doctor_probe_request(&ticket, &child.operation_id, DoctorProbeOutcome::Completed)
        .await
}

#[cfg(unix)]
#[tokio::test]
async fn startup_recovers_only_linked_child_and_marks_completed_ticket_stale()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = open(&path).await?;
    finish_ticket(&journal, '6', '7', '8').await?;
    let active = created_ticket(&journal, '9').await?;
    let child = seed(&journal, 'a').await?;
    journal
        .prepare_probe_for_doctor_request(&active, child.clone())
        .await?;
    journal
        .transition_probe(
            &child.operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    drop(active);
    drop(journal);

    let canonical_path = canonical_journal_path(&path)?;
    let daemon_lock_path = scratch.0.join("daemon.lock");
    let daemon_lock = DaemonLock::try_acquire(&daemon_lock_path)?;
    let journal = Journal::open(&path).await?;
    journal.bind_engine(ENGINE_ID).await?;
    let guard = EngineLineageGuard::acquire_at_for_tests(ENGINE_ID, &scratch.0)?;
    let instance_id = journal.instance_id().await?;
    let revision = journal.revision().await?;
    guard.verify_lineage(&canonical_path, &instance_id, revision)?;
    journal.attach_lineage_guard(guard.clone())?;
    let authority = crate::launch::SessionRefreshStartupAuthority::test_only(
        &daemon_lock,
        &daemon_lock_path,
        &canonical_path,
        &instance_id,
        ENGINE_ID,
        revision,
        &guard,
    )?;
    journal.recover_interrupted_host_work(authority).await?;

    let completed = journal
        .doctor_probe_request_by_nonce(&"6".repeat(32))
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(completed.state(), DoctorProbeState::Stale);
    assert_eq!(
        completed.child().map(|row| row.lease_state),
        Some(crate::journal::ProbeLeaseState::Released)
    );
    let recovering = journal
        .doctor_probe_request_by_nonce(&"9".repeat(32))
        .await?
        .ok_or(HostError::Journal)?;
    assert_eq!(recovering.state(), DoctorProbeState::Recovering);
    let row = recovering.child().ok_or(HostError::Journal)?;
    assert_eq!(row.operation_id, child.operation_id);
    assert_eq!(row.phase, ProbePhase::CreateRequested);
    assert_eq!(row.lease_state, crate::journal::ProbeLeaseState::Recovering);
    assert!(journal.active_probe().await?.is_none());
    let exact_ticket = journal
        .start_doctor_probe_request(&"9".repeat(32), ENGINE_ID, &"a".repeat(64))
        .await?;
    let DoctorProbeStartResult::Replayed(exact_ticket) = exact_ticket else {
        return Err(HostError::Journal);
    };
    assert_eq!(
        journal
            .begin_doctor_probe_recovery(&exact_ticket)
            .await?
            .lease_state,
        crate::journal::ProbeLeaseState::Recovering
    );
    Ok(())
}

#[tokio::test]
async fn doctor_ticket_waits_while_an_unlinked_probe_owns_the_lease() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = open(&scratch.file()).await?;
    let generic = seed(&journal, 'b').await?;
    journal.prepare_probe(generic).await?;
    let ticket = created_ticket(&journal, 'c').await?;
    let result = journal
        .prepare_probe_for_doctor_request(&ticket, seed(&journal, 'd').await?)
        .await?;
    assert!(matches!(result, DoctorProbePrepareResult::WaitingForProbe));
    assert_eq!(
        journal
            .doctor_probe_request_by_nonce(ticket.nonce())
            .await?
            .ok_or(HostError::Journal)?
            .state(),
        DoctorProbeState::Queued
    );
    Ok(())
}
