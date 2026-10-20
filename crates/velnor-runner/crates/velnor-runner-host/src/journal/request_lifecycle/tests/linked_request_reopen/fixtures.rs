//! Test helpers for linked request reservation recovery.

use super::super::helpers::{
    Scratch, binding, created_scope, event_source, finish_initialized_listener_delivery,
    initialize_reserved_request, runner_name,
};
use super::{
    assert_runner_request_association, authorize, reject_wrong_scope_and_runner, replay_link,
    reserve_linked_request,
};
use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::journal::{
    AssignedAcquireResult, AssignmentKey, CapacityLimits, ConnectionJournal, Journal,
    RequestAcquireState,
};
use velnor_runner_github::RunnerReference;

mod orphan_link;
pub(super) use orphan_link::reject_orphaned_reservation_link;
#[path = "fixtures/settlement.rs"]
mod settlement;
pub(super) use settlement::check_cleanup_and_settlement;

pub(super) struct LinkedLifecycle {
    scratch: Scratch,
    path: std::path::PathBuf,
    repository: RepositoryConnection,
    journal: Journal,
    pub(super) scope: ConnectionJournal,
    witness: crate::journal::AssignmentIdentity,
    pub(super) physical: crate::journal::AssignmentIdentity,
    pub(super) witness_request_id: i64,
    pub(super) actual_request_id: i64,
    pub(super) nonce: char,
    pub(super) reservation_id: i64,
    pub(super) intent_id: i64,
    provisioned: super::super::helpers::ProvisionedRunner,
    pub(super) runner: RunnerReference,
    pub(super) source: velnor_runner_github::SourceMetadata,
    pub(super) job_id: String,
}

pub(super) async fn prepare_linked_lifecycle(
    witness_request_id: i64,
    actual_request_id: i64,
    nonce: char,
) -> Result<LinkedLifecycle, HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity_with_operational_limits(
            CapacityLimits {
                jobs: 1,
                cpu_millicores: 2_000,
                memory_bytes: 25_769_803_776,
            },
            crate::journal::OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let repository = binding("essential", 101)?;
    let scope = created_scope(&journal, &repository, 501).await?;
    let witness = authorize(&scope, witness_request_id).await?;
    if actual_request_id != witness_request_id {
        super::record_acquired(&scope, actual_request_id).await?;
    }
    let (physical, reservation_id, intent_id, root_digest, sample_generation_id) =
        reserve_linked_request(&scope, &witness, nonce).await?;
    scope
        .record_request_acquire(
            &witness,
            AssignedAcquireResult::Returned(vec![witness_request_id]),
        )
        .await?;
    let (recorded_intent_id, fresh) = scope.begin_launch(&physical).await?;
    assert_eq!(recorded_intent_id, intent_id);
    assert!(!fresh);
    let initialized = initialize_reserved_request(
        &scope,
        witness.clone(),
        physical.clone(),
        nonce,
        reservation_id,
        intent_id,
        root_digest,
        sample_generation_id,
    )
    .await?;
    let provisioned = initialized.provisioned.clone();
    let runner = RunnerReference {
        id: 701,
        name: runner_name(nonce),
        runner_scale_set_id: 501,
    };
    finish_initialized_listener_delivery(
        &scope,
        &repository,
        &initialized,
        &runner,
        witness_request_id,
    )
    .await?;
    let source = event_source(101, actual_request_id + 1_000);
    let job_id = format!("opaque-job-{actual_request_id}");
    Ok(LinkedLifecycle {
        scratch,
        path,
        repository,
        journal,
        scope,
        witness,
        physical,
        witness_request_id,
        actual_request_id,
        nonce,
        reservation_id,
        intent_id,
        provisioned,
        runner,
        source,
        job_id,
    })
}

pub(super) async fn reopen(fixture: LinkedLifecycle) -> Result<LinkedLifecycle, HostError> {
    let LinkedLifecycle {
        scratch,
        path,
        repository,
        journal,
        scope,
        witness,
        physical,
        witness_request_id,
        actual_request_id,
        nonce,
        reservation_id,
        intent_id,
        provisioned,
        runner,
        source,
        job_id,
    } = fixture;
    drop(scope);
    drop(journal);
    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&repository).await?;
    Ok(LinkedLifecycle {
        scratch,
        path,
        repository,
        journal,
        scope,
        witness,
        physical,
        witness_request_id,
        actual_request_id,
        nonce,
        reservation_id,
        intent_id,
        provisioned,
        runner,
        source,
        job_id,
    })
}

pub(super) async fn check_started_replay(fixture: &LinkedLifecycle) -> Result<(), HostError> {
    let scope = &fixture.scope;
    replay_link(
        scope,
        fixture.witness_request_id,
        fixture.nonce,
        'c',
        fixture.reservation_id,
        fixture.intent_id,
    )
    .await?;
    assert_runner_request_association(
        scope,
        fixture.reservation_id,
        fixture.witness_request_id,
        None,
    )
    .await?;
    reject_wrong_scope_and_runner(
        &fixture.journal,
        scope,
        fixture.actual_request_id,
        &fixture.runner,
    )
    .await?;
    assert_runner_request_association(
        scope,
        fixture.reservation_id,
        fixture.witness_request_id,
        None,
    )
    .await?;
    let actual = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    assert_eq!(
        scope
            .record_job_started(
                &actual,
                901,
                &fixture.source,
                &fixture.runner,
                Some(&fixture.job_id),
            )
            .await?,
        fixture.intent_id
    );
    assert_runner_request_association(
        scope,
        fixture.reservation_id,
        fixture.witness_request_id,
        Some(fixture.actual_request_id),
    )
    .await?;
    assert_eq!(
        scope
            .record_job_started(
                &actual,
                901,
                &fixture.source,
                &fixture.runner,
                Some(&fixture.job_id),
            )
            .await?,
        fixture.intent_id,
        "The exact Started replay must keep its original intent."
    );
    replay_link(
        scope,
        fixture.witness_request_id,
        fixture.nonce,
        'd',
        fixture.reservation_id,
        fixture.intent_id,
    )
    .await
}

pub(super) async fn check_completed_replay(fixture: &LinkedLifecycle) -> Result<(), HostError> {
    let scope = &fixture.scope;
    replay_link(
        scope,
        fixture.witness_request_id,
        fixture.nonce,
        'e',
        fixture.reservation_id,
        fixture.intent_id,
    )
    .await?;
    let actual = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    assert_eq!(
        scope
            .record_job_started(
                &actual,
                901,
                &fixture.source,
                &fixture.runner,
                Some(&fixture.job_id),
            )
            .await?,
        fixture.intent_id
    );
    assert_runner_request_association(
        scope,
        fixture.reservation_id,
        fixture.witness_request_id,
        Some(fixture.actual_request_id),
    )
    .await?;
    super::record_completed(fixture, &actual).await?;
    super::record_completed(fixture, &actual).await
}

pub(super) async fn check_final_replay(fixture: &LinkedLifecycle) -> Result<(), HostError> {
    assert_eq!(
        fixture
            .scope
            .request_acquire_state(&fixture.witness)
            .await?,
        Some(RequestAcquireState::Acquired)
    );
    replay_link(
        &fixture.scope,
        fixture.witness_request_id,
        fixture.nonce,
        '8',
        fixture.reservation_id,
        fixture.intent_id,
    )
    .await?;
    let actual = fixture
        .scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: fixture.actual_request_id,
        })
        .await?;
    assert_eq!(
        fixture
            .scope
            .record_job_started(
                &actual,
                901,
                &fixture.source,
                &fixture.runner,
                Some(&fixture.job_id),
            )
            .await?,
        fixture.intent_id
    );
    assert_eq!(
        fixture
            .scope
            .record_job_completed(
                &actual,
                902,
                &fixture.source,
                Some("success"),
                Some(&fixture.runner),
                Some(&fixture.job_id),
            )
            .await?,
        Some(fixture.intent_id)
    );
    Ok(())
}
