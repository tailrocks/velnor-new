use std::path::{Path, PathBuf};

use super::helpers::{
    TEST_ENGINE_ID, event_source, provision_acquired_request_with_initialization,
    record_owned_storage_cleanup, runner_name, settle_storage_metadata,
};
use super::review_counterexamples::ready;
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, Journal, ReservationRequest, ReservationResult, ReservationState,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

#[tokio::test]
async fn version_ten_migration_preserves_ready_runner_and_capacity_charge() -> Result<(), HostError>
{
    let fixture = ready().await?;
    let path = fixture.scratch.file();
    drop(fixture.scope);
    drop(fixture.journal);
    make_version_ten_jit_schema(&path).await?;

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&fixture.binding).await?;
    assert_eq!(
        scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
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
    record_owned_storage_cleanup(&fixture.scope, &fixture.provisioned).await?;
    fixture
        .scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    fixture
        .scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
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
    let provisioned = fixture.provisioned.clone();
    let scratch = fixture.scratch;
    drop(fixture.scope);
    drop(fixture.journal);
    restore_legacy_runner_state(&path, intent, legacy_state).await?;
    make_version_ten_jit_schema(&path).await?;

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Uncertain)
    );
    assert_eq!(
        settle_storage_metadata(
            &scope,
            &provisioned,
            char::from(b'5' + u8::try_from(index).map_err(|_| HostError::Journal)?),
        )
        .await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Released)
    );
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
    make_version_ten_jit_schema(&path).await?;

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
    provisioned: super::helpers::ProvisionedRunner,
    request: crate::journal::AssignmentIdentity,
    runner: velnor_runner_github::RunnerReference,
    intent: i64,
}

#[tokio::test]
async fn version_ten_cleaned_runner_migrates_and_allows_new_jit() -> Result<(), HostError> {
    let (_scratch, migrated) = prepare_completed_legacy_fixture(ready().await?).await?;
    let journal = Journal::open(&migrated.path).await?;
    let scope = journal.connection_scope(&migrated.binding).await?;
    assert_eq!(
        scope
            .reservation(&migrated.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Uncertain)
    );
    assert_eq!(
        settle_storage_metadata(&scope, &migrated.provisioned, '6').await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    assert_eq!(
        scope
            .reservation(&migrated.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Released)
    );
    assert_eq!(
        scope
            .record_job_completed(
                &migrated.request,
                703,
                &event_source(101, 1070),
                Some("success"),
                Some(&migrated.runner),
                None,
            )
            .await?,
        Some(migrated.intent)
    );
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
    record_owned_storage_cleanup(&fixture.scope, &fixture.provisioned).await?;
    let reservation = fixture
        .scope
        .reservation(&fixture.physical)
        .await?
        .ok_or(HostError::Journal)?;
    fixture
        .scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    fixture
        .scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    assert_eq!(
        settle_storage_metadata(&fixture.scope, &fixture.provisioned, '5').await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    let path = fixture.scratch.file();
    let migrated = CompletedLegacyFixture {
        path: path.clone(),
        binding: fixture.binding.clone(),
        physical: fixture.physical.clone(),
        provisioned: fixture.provisioned.clone(),
        request: fixture.request.clone(),
        runner: fixture.runner.clone(),
        intent: fixture.intent,
    };
    let scratch = fixture.scratch;
    drop(fixture.scope);
    drop(fixture.journal);
    make_version_ten_jit_schema(&path).await?;
    Ok((scratch, migrated))
}

async fn make_version_ten_jit_schema(path: &Path) -> Result<(), HostError> {
    let path = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut rows = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'connection_runner_jits'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let sql: String = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)?;
    let version_ten_sql = sql
        .replace(
            "CREATE TABLE connection_runner_jits (",
            "CREATE TABLE connection_runner_jits_v10 (",
        )
        .replace(
            "CHECK (state != 'completed' OR completed_request_id IS NOT NULL)",
            "CHECK (state NOT IN ('completed', 'cleaned') OR completed_request_id IS NOT NULL)",
        );
    if version_ten_sql == sql {
        return Err(HostError::Journal);
    }
    connection
        .execute(&version_ten_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "INSERT INTO connection_runner_jits_v10 (intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state, runner_id, runner_name, started_request_id, completed_request_id) SELECT intent_id, connection_id, scale_set_id, witness_request_id, requested_name, state, runner_id, runner_name, started_request_id, completed_request_id FROM connection_runner_jits",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TABLE connection_runner_jits", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "ALTER TABLE connection_runner_jits_v10 RENAME TO connection_runner_jits",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    for index in [
        "CREATE UNIQUE INDEX connection_runner_jit_id ON connection_runner_jits(connection_id, scale_set_id, runner_id) WHERE runner_id IS NOT NULL",
        "CREATE UNIQUE INDEX connection_runner_jit_name ON connection_runner_jits(connection_id, scale_set_id, runner_name) WHERE runner_name IS NOT NULL AND state != 'cleaned'",
        "CREATE UNIQUE INDEX connection_runner_jit_witness ON connection_runner_jits(connection_id, scale_set_id, witness_request_id) WHERE state != 'cleaned'",
        "CREATE UNIQUE INDEX connection_runner_jit_started_request ON connection_runner_jits(connection_id, scale_set_id, started_request_id) WHERE started_request_id IS NOT NULL",
        "CREATE UNIQUE INDEX connection_runner_jit_completed_request ON connection_runner_jits(connection_id, scale_set_id, completed_request_id) WHERE completed_request_id IS NOT NULL",
    ] {
        connection
            .execute(index, ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("PRAGMA user_version = 10", ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}

async fn restore_legacy_runner_state(
    path: &Path,
    intent_id: i64,
    legacy_state: &str,
) -> Result<(), HostError> {
    if !matches!(
        legacy_state,
        "bound" | "start_requested" | "start_uncertain" | "ready" | "started"
    ) {
        return Err(HostError::Journal);
    }
    let path = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(path)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let changed = connection
        .execute(
            "UPDATE connection_runner_jits SET state = ?1 WHERE intent_id = ?2 AND state = 'cleaned' AND runner_id IS NOT NULL AND runner_name IS NOT NULL",
            (legacy_state, intent_id),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(HostError::Journal)
    }
}
