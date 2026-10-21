use super::admission::JOB_LIMIT;
use super::helpers::{
    ProvisionedRunner, Scratch, binding, created_scope, event_source,
    finish_initialized_listener_delivery, provision_acquired_request_with_initialization, receipt,
    record_observed_storage_cleanup, record_owned_network_cleanup, record_owned_storage_cleanup,
    record_runner_absent_for_cleanup, record_runner_completion_for_cleanup, runner_name,
    settle_storage_metadata,
};
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
    pub(super) provisioned: ProvisionedRunner,
}

pub(super) async fn ready() -> Result<Ready, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity_with_operational_limits(
            JOB_LIMIT,
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let binding = binding("essential", 101)?;
    let scope = created_scope(&journal, &binding, 501).await?;
    let initialized = provision_acquired_request_with_initialization(&scope, 70, '8', '8').await?;
    let provisioned = initialized.provisioned.clone();
    let physical = provisioned.assignment.clone();
    let intent = provisioned.intent_id;
    let runner = RunnerReference {
        id: 770,
        name: runner_name('8'),
        runner_scale_set_id: 501,
    };
    finish_initialized_listener_delivery(&scope, &binding, &initialized, &runner, 70).await?;
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
        provisioned,
    })
}

#[tokio::test]
async fn review_exact_started_replay_survives_completion_and_restart() -> Result<(), HostError> {
    let f = ready().await?;
    assert_eq!(
        f.scope
            .record_job_started(&f.request, 701, &event_source(101, 1070), &f.runner, None)
            .await?,
        f.intent
    );
    assert_eq!(
        f.scope
            .record_job_started(&f.request, 701, &event_source(101, 1070), &f.runner, None)
            .await,
        Ok(f.intent)
    );
    assert_eq!(
        f.scope
            .record_job_completed(
                &f.request,
                704,
                &event_source(101, 1070),
                Some("success"),
                Some(&f.runner),
                None,
            )
            .await?,
        Some(f.intent)
    );
    assert_eq!(
        f.scope
            .record_job_started(&f.request, 701, &event_source(101, 1070), &f.runner, None)
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
            .record_job_started(&request, 701, &event_source(101, 1070), &f.runner, None)
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
            .record_job_completed(
                &request,
                702,
                &event_source(101, 1071),
                Some("cancelled"),
                None,
                None,
            )
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
            .record_job_completed(
                &f.request,
                703,
                &event_source(101, 1070),
                Some("success"),
                Some(&f.runner),
                None,
            )
            .await?,
        Some(f.intent)
    );
    let reservation = f
        .scope
        .reservation(&f.physical)
        .await?
        .ok_or(HostError::Journal)?;
    record_owned_storage_cleanup(&f.scope, &f.provisioned).await?;
    f.scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    f.scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    assert!(!f.scope.intent(f.intent).await?.cleanup_proven);
    assert_eq!(
        f.scope.reservation(&f.physical).await?.map(|row| row.state),
        Some(crate::journal::ReservationState::Reserved)
    );
    let path = f.scratch.file();
    drop(f.scope);
    drop(f.journal);
    assert!(
        Journal::open(&path).await.is_ok(),
        "completed runner cleanup made journal reopen fail"
    );
    Ok(())
}

#[path = "review_counterexamples/cleanup_recovery.rs"]
mod cleanup_recovery;
