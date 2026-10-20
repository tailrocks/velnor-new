use std::path::PathBuf;

use super::helpers::{
    TEST_ENGINE_ID, event_source, provision_acquired_request_with_initialization,
    record_observed_storage_cleanup, record_owned_network_cleanup, record_owned_storage_cleanup,
    record_runner_absent_for_cleanup, record_runner_completion_for_cleanup, runner_name,
    settle_storage_metadata,
};
use super::review_counterexamples::ready;
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, Journal, ReservationRequest, ReservationResult, ReservationState,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

#[path = "review_migration/v10_fixture.rs"]
mod v10_fixture;
use v10_fixture::make_version_ten_jit_schema;
#[path = "review_migration/v10_assertions.rs"]
mod v10_assertions;
use v10_assertions::{assert_cleaned_runner_identity, restore_legacy_runner_state};

// Version 10 has no storage-provisioning history for a second settlement.

#[tokio::test]
async fn version_ten_migration_preserves_ready_runner_and_capacity_charge() -> Result<(), HostError>
{
    let fixture = ready().await?;
    let path = fixture.scratch.file();
    drop(fixture.scope);
    drop(fixture.journal);
    let path = make_version_ten_jit_schema(&path).await?;

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&fixture.binding).await?;
    let reservation = scope.reservation(&fixture.physical).await?;
    assert_eq!(
        reservation.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_eq!(
        scope
            .record_job_started(
                &fixture.request,
                701,
                &event_source(101, 1070),
                &fixture.runner,
                None,
            )
            .await?,
        fixture.intent
    );
    Ok(())
}

#[tokio::test]
async fn version_ten_known_cleaned_runner_states_allow_replacement_jit() -> Result<(), HostError> {
    for (index, legacy_state) in [
        "bound",
        "start_requested",
        "start_uncertain",
        "ready",
        "started",
    ]
    .into_iter()
    .enumerate()
    {
        assert_known_cleaned_state_allows_replacement(legacy_state, index).await?;
    }
    Ok(())
}

async fn assert_known_cleaned_state_allows_replacement(
    legacy_state: &str,
    index: usize,
) -> Result<(), HostError> {
    let fixture = ready().await?;
    let (_scratch, journal, scope) =
        close_and_reopen_legacy_fixture(fixture, legacy_state, index).await?;
    let nonce = char::from(b'a' + u8::try_from(index).map_err(|_| HostError::Journal)?);
    let replacement =
        provision_acquired_request_with_initialization(&scope, 72, nonce, nonce).await?;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    assert!(
        scope
            .claim_runner_jit_for_request(
                replacement.intent_id,
                replacement.reservation_id,
                72,
                &runner_name(nonce),
                generation,
                &digest
            )
            .await?
            .is_some(),
        "known runner in {legacy_state} with exact released cleanup proof blocked replacement"
    );
    drop(scope);
    drop(journal);
    Ok(())
}

async fn close_and_reopen_legacy_fixture(
    fixture: super::review_counterexamples::Ready,
    legacy_state: &str,
    index: usize,
) -> Result<
    (
        super::helpers::Scratch,
        Journal,
        crate::journal::ConnectionJournal,
    ),
    HostError,
> {
    if legacy_state == "started" {
        fixture
            .scope
            .record_job_started(
                &fixture.request,
                701,
                &event_source(101, 1070),
                &fixture.runner,
                None,
            )
            .await?;
    }
    let reservation = fixture
        .scope
        .reservation(&fixture.physical)
        .await?
        .ok_or(HostError::Journal)?;
    fixture
        .scope
        .mark_recovering(reservation.reservation_id)
        .await?;
    record_observed_storage_cleanup(&fixture.scope, reservation.reservation_id).await?;
    fixture
        .scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    fixture
        .scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    record_owned_network_cleanup(&fixture.scope, reservation.reservation_id).await?;
    assert_eq!(
        settle_storage_metadata(
            &fixture.scope,
            &fixture.provisioned,
            char::from(b'0' + u8::try_from(index).map_err(|_| HostError::Journal)?),
        )
        .await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    let path = fixture.scratch.file();
    let intent = fixture.intent;
    let physical = fixture.physical.clone();
    let binding = fixture.binding.clone();
    let scratch = fixture.scratch;
    drop(fixture.scope);
    drop(fixture.journal);
    restore_legacy_runner_state(&path, intent, legacy_state).await?;
    let path = make_version_ten_jit_schema(&path).await?;

    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(super::admission::JOB_LIMIT)
        .await?;
    journal
        .configure_storage_profile(crate::journal::fixture_provisioning_profile())
        .await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Released)
    );
    let config = crate::config::HostConfig::test_with_job_authority_policy(&binding)?;
    scope.install_job_authority_policy(&config).await?;
    Ok((scratch, journal, scope))
}

#[tokio::test]
async fn version_ten_partial_cleanup_keeps_capacity_held() -> Result<(), HostError> {
    let fixture = ready().await?;
    let reservation = fixture
        .scope
        .reservation(&fixture.physical)
        .await?
        .ok_or(HostError::Journal)?;
    fixture
        .scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    let path = fixture.scratch.file();
    let physical = fixture.physical.clone();
    let binding = fixture.binding.clone();
    drop(fixture.scope);
    drop(fixture.journal);
    let path = make_version_ten_jit_schema(&path).await?;

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    let assignment = scope
        .assignment_identity(AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "f".repeat(32),
        })
        .await?;
    let permit = StartPermit::test_fixture_with_engine_id(&"f".repeat(32), TEST_ENGINE_ID);
    let generation = permit.sample_generation_id().to_owned();
    let root_digest = permit.docker_root_digest().to_owned();
    let storage = fixture_storage_provisioning_admission(&permit)?;
    assert_eq!(
        scope
            .try_reserve_with_provisioning(
                ReservationRequest {
                    assignment: &assignment,
                    engine_id: TEST_ENGINE_ID,
                    docker_root_digest: &root_digest,
                    cpu_millicores: super::admission::JOB_LIMIT.cpu_millicores,
                    memory_bytes: 8_589_934_592,
                    sample_generation_id: &generation,
                },
                permit,
                storage,
            )
            .await?,
        ReservationResult::AtCapacity
    );
    Ok(())
}

struct CompletedLegacyFixture {
    path: PathBuf,
    binding: crate::config::RepositoryConnection,
    physical: crate::journal::AssignmentIdentity,
    runner: velnor_runner_github::RunnerReference,
}

#[tokio::test]
async fn version_ten_cleaned_runner_migrates_and_allows_new_jit() -> Result<(), HostError> {
    let (_scratch, migrated) = prepare_completed_legacy_fixture(ready().await?).await?;
    let journal = Journal::open(&migrated.path).await?;
    journal
        .configure_capacity(super::admission::JOB_LIMIT)
        .await?;
    journal
        .configure_storage_profile(crate::journal::fixture_provisioning_profile())
        .await?;
    let scope = journal.connection_scope(&migrated.binding).await?;
    let config = crate::config::HostConfig::test_with_job_authority_policy(&migrated.binding)?;
    scope.install_job_authority_policy(&config).await?;
    assert_eq!(
        scope
            .reservation(&migrated.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Released)
    );
    assert_cleaned_runner_identity(&migrated.path, &migrated.runner).await?;
    let next = provision_acquired_request_with_initialization(&scope, 72, 'f', 'f').await?;
    let (generation, digest) = scope.test_active_authority_identity().await?;
    assert!(
        scope
            .claim_runner_jit_for_request(
                next.intent_id,
                next.reservation_id,
                72,
                &runner_name('f'),
                generation,
                &digest
            )
            .await?
            .is_some()
    );
    Ok(())
}

async fn prepare_completed_legacy_fixture(
    fixture: super::review_counterexamples::Ready,
) -> Result<(super::helpers::Scratch, CompletedLegacyFixture), HostError> {
    fixture
        .scope
        .record_job_started(
            &fixture.request,
            701,
            &event_source(101, 1070),
            &fixture.runner,
            None,
        )
        .await?;
    fixture
        .scope
        .record_job_completed(
            &fixture.request,
            703,
            &event_source(101, 1070),
            Some("success"),
            Some(&fixture.runner),
            None,
        )
        .await?;
    let cleanup_generation = record_runner_completion_for_cleanup(
        &fixture.scope,
        &fixture.request,
        fixture.intent,
        None,
        703,
        &event_source(101, 1070),
        &fixture.runner,
    )
    .await?;
    record_owned_storage_cleanup(&fixture.scope, &fixture.provisioned).await?;
    let reservation = fixture
        .scope
        .reservation(&fixture.physical)
        .await?
        .ok_or(HostError::Journal)?;
    fixture
        .scope
        .mark_recovering(reservation.reservation_id)
        .await?;
    fixture
        .scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    fixture
        .scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    record_owned_network_cleanup(&fixture.scope, reservation.reservation_id).await?;
    record_runner_absent_for_cleanup(&fixture.scope, fixture.intent, cleanup_generation).await?;
    assert_eq!(
        settle_storage_metadata(&fixture.scope, &fixture.provisioned, '5').await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    let path = fixture.scratch.file();
    let mut migrated = CompletedLegacyFixture {
        path: path.clone(),
        binding: fixture.binding.clone(),
        physical: fixture.physical.clone(),
        runner: fixture.runner.clone(),
    };
    let scratch = fixture.scratch;
    drop(fixture.scope);
    drop(fixture.journal);
    migrated.path = make_version_ten_jit_schema(&path).await?;
    Ok((scratch, migrated))
}
