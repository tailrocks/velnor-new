use super::admission::{JOB_LIMIT, physical_intent};
use super::helpers::{Scratch, binding, created_scope, receipt, record_acquired, runner_name};
use crate::error::HostError;
use crate::journal::{AssignmentIdentity, AssignmentKey, ConnectionJournal, Journal};
use velnor_runner_github::RunnerReference;

pub(super) struct Ready {
    pub(super) scratch: Scratch,
    pub(super) journal: Journal,
    pub(super) binding: crate::config::RepositoryConnection,
    pub(super) scope: ConnectionJournal,
    pub(super) physical: AssignmentIdentity,
    pub(super) intent: i64,
    pub(super) request: AssignmentIdentity,
    pub(super) runner: RunnerReference,
}

pub(super) async fn ready() -> Result<Ready, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    record_acquired(&scope, 70).await?;
    let (physical, intent) = physical_intent(&scope, '8').await?;
    let jit = scope
        .claim_runner_jit(intent, &runner_name('8'))
        .await?
        .ok_or(HostError::Journal)?;
    let runner = RunnerReference {
        id: 770,
        name: runner_name('8'),
        runner_scale_set_id: 501,
    };
    scope.bind_jit_runner(jit, &runner).await?;
    let start = scope
        .claim_physical_runner_start(intent, &format!("w{}", "9".repeat(32)))
        .await?
        .ok_or(HostError::Journal)?;
    scope
        .record_physical_worker_ready(start, "runner-container", "dind-container")
        .await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 70 })
        .await?;
    Ok(Ready {
        scratch,
        journal,
        binding,
        scope,
        physical,
        intent,
        request,
        runner,
    })
}

#[tokio::test]
async fn review_exact_started_replay_survives_completion_and_restart() -> Result<(), HostError> {
    let f = ready().await?;
    assert_eq!(
        f.scope
            .record_job_started(&f.request, 701, &f.runner, None)
            .await?,
        f.intent
    );
    assert_eq!(
        f.scope
            .record_job_started(&f.request, 701, &f.runner, None)
            .await,
        Ok(f.intent)
    );
    assert_eq!(
        f.scope
            .record_job_completed(&f.request, 704, Some("success"), Some(&f.runner), None)
            .await?,
        Some(f.intent)
    );
    assert_eq!(
        f.scope
            .record_job_started(&f.request, 701, &f.runner, None)
            .await?,
        f.intent
    );
    let path = f.scratch.file();
    let binding = f.binding.clone();
    drop(f.scope);
    drop(f.journal);
    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 70 })
        .await?;
    assert_eq!(
        scope
            .record_job_started(&request, 701, &f.runner, None)
            .await?,
        f.intent
    );
    Ok(())
}

#[tokio::test]
async fn review_runnerless_completion_before_acquire_remains_reopenable() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 71 })
        .await?;
    let source = receipt(&scope, 71, 171, 101).await?;
    scope.record_source_authorization(&request, &source).await?;
    assert_eq!(
        scope
            .record_job_completed(&request, 702, Some("cancelled"), None, None)
            .await?,
        None
    );
    drop(scope);
    drop(journal);
    assert!(
        Journal::open(&path).await.is_ok(),
        "authorized runnerless completion made journal reopen fail"
    );
    Ok(())
}

#[tokio::test]
async fn review_completed_runner_cleanup_remains_reopenable() -> Result<(), HostError> {
    let f = ready().await?;
    assert_eq!(
        f.scope
            .record_job_completed(&f.request, 703, Some("success"), Some(&f.runner), None)
            .await?,
        Some(f.intent)
    );
    let reservation = f
        .scope
        .reservation(&f.physical)
        .await?
        .ok_or(HostError::Journal)?;
    f.scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    f.scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    assert!(f.scope.intent(f.intent).await?.cleanup_proven);
    let path = f.scratch.file();
    drop(f.scope);
    drop(f.journal);
    assert!(
        Journal::open(&path).await.is_ok(),
        "completed runner cleanup made journal reopen fail"
    );
    Ok(())
}

#[tokio::test]
async fn independent_recovered_unassigned_runner_allows_replacement_for_same_request()
-> Result<(), HostError> {
    let f = ready().await?;
    let reservation = f
        .scope
        .reservation(&f.physical)
        .await?
        .ok_or(HostError::Journal)?;
    f.scope.mark_recovering(reservation.reservation_id).await?;
    f.scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    f.scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    assert!(f.scope.intent(f.intent).await?.cleanup_proven);
    let path = f.scratch.file();
    let binding = f.binding.clone();
    drop(f.scope);
    drop(f.journal);
    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let (_physical, replacement) = physical_intent(&scope, 'f').await?;
    assert!(
        scope
            .claim_runner_jit(replacement, &runner_name('f'))
            .await?
            .is_some(),
        "cleaned unassigned runner still holds the only acquired request witness"
    );
    Ok(())
}

#[tokio::test]
async fn independent_completed_cleanup_allows_new_demand_after_restart() -> Result<(), HostError> {
    let f = ready().await?;
    assert_eq!(
        f.scope
            .record_job_started(&f.request, 701, &f.runner, None)
            .await?,
        f.intent
    );
    assert_eq!(
        f.scope
            .record_job_completed(&f.request, 703, Some("success"), Some(&f.runner), None)
            .await?,
        Some(f.intent)
    );
    let reservation = f
        .scope
        .reservation(&f.physical)
        .await?
        .ok_or(HostError::Journal)?;
    f.scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    f.scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    let path = f.scratch.file();
    let binding = f.binding.clone();
    drop(f.scope);
    drop(f.journal);
    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    let request = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 70 })
        .await?;
    assert_eq!(
        scope
            .record_job_started(&request, 701, &f.runner, None)
            .await?,
        f.intent
    );
    record_acquired(&scope, 72).await?;
    let (_physical, next) = physical_intent(&scope, 'f').await?;
    assert!(
        scope
            .claim_runner_jit(next, &runner_name('f'))
            .await?
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn independent_authorized_runnerless_cancel_keeps_physical_charge_after_restart()
-> Result<(), HostError> {
    let f = ready().await?;
    let cancel = f
        .scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 71 })
        .await?;
    let source = receipt(&f.scope, 71, 171, 101).await?;
    f.scope
        .record_source_authorization(&cancel, &source)
        .await?;
    assert_eq!(
        f.scope
            .record_job_completed(&cancel, 702, Some("cancelled"), None, None)
            .await?,
        None
    );
    let path = f.scratch.file();
    let binding = f.binding.clone();
    drop(f.scope);
    drop(f.journal);
    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        scope.reservation(&f.physical).await?.map(|r| r.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    assert!(!scope.intent(f.intent).await?.cleanup_proven);
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "f".repeat(32),
        })
        .await?;
    let permit = crate::launch::StartPermit::test_fixture();
    let generation = permit.sample_generation_id().to_owned();
    let digest = permit.docker_root_digest().to_owned();
    let result = scope
        .try_reserve(
            crate::journal::ReservationRequest {
                assignment: &assignment,
                engine_id: "test-engine",
                docker_root_digest: &digest,
                cpu_millicores: JOB_LIMIT.cpu_millicores,
                memory_bytes: JOB_LIMIT.memory_bytes,
                sample_generation_id: &generation,
            },
            permit,
        )
        .await?;
    assert!(matches!(
        result,
        crate::journal::ReservationResult::AtCapacity
    ));
    Ok(())
}
