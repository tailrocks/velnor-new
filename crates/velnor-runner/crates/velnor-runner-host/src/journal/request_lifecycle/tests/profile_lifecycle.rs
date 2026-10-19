use super::helpers::{TEST_ENGINE_ID, record_owned_storage_cleanup, settle_storage_metadata};
use super::review_counterexamples::ready;
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, Journal, MetadataSettlementResult, OperationalCapacityLimits, ProbeSeed,
    ReservationRequest, ReservationResult, ReservationState, StorageProfile, fixture_admission,
    fixture_profile,
};
use crate::launch::StartPermit;

fn next_profile(current: &StorageProfile) -> Result<StorageProfile, HostError> {
    StorageProfile::new(
        "f".repeat(32),
        current.pools().to_vec(),
        current.roles().to_vec(),
    )
}

#[tokio::test]
async fn profile_generation_change_waits_for_hold_settlement_across_reopen() -> Result<(), HostError>
{
    let fixture = ready().await?;
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

    let previous = fixture
        .scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let next = next_profile(&previous)?;
    fixture
        .journal
        .configure_storage_profile(previous.clone())
        .await?;
    assert!(
        fixture
            .journal
            .configure_storage_profile(next.clone())
            .await
            .is_err(),
        "An active capacity reservation must keep its storage profile current."
    );
    assert_eq!(
        fixture
            .scope
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)?
            .generation_id(),
        previous.generation_id()
    );

    let path = fixture.scratch.file();
    let binding = fixture.binding.clone();
    drop(fixture.scope);
    drop(fixture.journal);

    let journal = Journal::open(&path).await?;
    let scope = journal.connection_scope(&binding).await?;
    assert_eq!(
        scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_eq!(
        settle_storage_metadata(&scope, &fixture.provisioned, 'd').await?,
        MetadataSettlementResult::Settled
    );
    assert_eq!(
        scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Released)
    );
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    journal.configure_storage_profile(next.clone()).await?;
    assert_eq!(
        journal
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)?
            .generation_id(),
        next.generation_id()
    );
    Ok(())
}

#[tokio::test]
async fn profile_switch_serializes_with_settlement_claim() -> Result<(), HostError> {
    let fixture = ready().await?;
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
    fixture
        .journal
        .configure_capacity_with_operational_limits(
            super::admission::JOB_LIMIT,
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let previous = fixture
        .scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    let next = next_profile(&previous)?;
    let operation_id = "9".repeat(32);
    let seed = ProbeSeed {
        operation_id: operation_id.clone(),
        instance_id: fixture.journal.instance_id().await?,
        engine_id: TEST_ENGINE_ID.to_owned(),
        docker_root_digest: fixture.provisioned.docker_root_digest.clone(),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name: format!("velnor-resource-probe-{operation_id}"),
        projection_digest: "e".repeat(64),
        purpose: crate::journal::ResourceProbePurpose::Capacity,
        request_sha256: "5d76d86293261804706a16a73e0491cf4caa859f1b876cda706e568592b85cdf"
            .to_owned(),
    };

    let (profile_result, claim_result) = tokio::join!(
        fixture.journal.configure_storage_profile(next),
        fixture
            .scope
            .prepare_storage_metadata_settlement_probe(reservation.reservation_id, seed),
    );
    assert!(profile_result.is_err());
    let claim = claim_result?;
    assert_eq!(claim.profile_generation_id(), previous.generation_id());
    assert_eq!(
        fixture
            .scope
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)?
            .generation_id(),
        previous.generation_id()
    );
    Ok(())
}

#[tokio::test]
async fn profile_switch_holds_every_charged_reservation_without_provisioning()
-> Result<(), HostError> {
    for state in ["reserved", "uncertain", "recovering"] {
        assert_charged_unprovisioned_state_blocks_profile_switch(state).await?;
    }
    Ok(())
}

#[tokio::test]
async fn first_profile_keeps_legacy_capacity_charged_and_blocks_admission() -> Result<(), HostError>
{
    let scratch = super::helpers::Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(super::admission::JOB_LIMIT)
        .await?;
    journal.bind_engine(TEST_ENGINE_ID).await?;
    let binding = super::helpers::binding("profile-initial-legacy", 102)?;
    let scope = journal.connection_scope(&binding).await?;
    scope
        .bind_scale_set(502, "integration", SetOwnership::Adopted)
        .await?;
    scope
        .bind_engine_context(TEST_ENGINE_ID, "unix:///var/run/docker.sock", "linux/amd64")
        .await?;
    insert_legacy_reservation(&journal, &scope).await?;

    let profile = fixture_profile();
    journal.configure_storage_profile(profile).await?;
    let assignment = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 78 })
        .await?;
    let generation = "8".repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&generation, TEST_ENGINE_ID);
    let root = permit.docker_root_digest().to_owned();
    let storage = fixture_admission(&permit)?;
    let result = scope
        .try_reserve_with_storage(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &root,
                cpu_millicores: 2_000,
                memory_bytes: 8_u64 << 30,
                sample_generation_id: &generation,
            },
            permit,
            storage,
        )
        .await?;
    assert_eq!(result, ReservationResult::AtCapacity);
    assert_eq!(count_active_reservations(&journal).await?, 1);

    let current = journal
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    assert!(
        journal
            .configure_storage_profile(next_profile(&current)?)
            .await
            .is_err()
    );
    Ok(())
}

async fn insert_legacy_reservation(
    journal: &Journal,
    scope: &crate::journal::ConnectionJournal,
) -> Result<i64, HostError> {
    let assignment = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 77 })
        .await?;
    let assignment_key = assignment.storage_key();
    let connection_id = scope.connection_id().as_str();
    let sample = "7".repeat(32);
    let connection = journal.connection().await?;
    connection
        .execute(
            "INSERT INTO connection_intents (connection_id, kind, subject, state, scale_set_id, runner_request_id, runner_name, assignment_key) VALUES (?1, 'launch', ?2, 'pending', ?3, 77, 'v77', ?2)",
            (connection_id, assignment_key.as_str(), assignment.scale_set_id()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let intent_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO capacity_reservations (intent_id, connection_id, assignment_key, engine_id, docker_root_digest, binding_digest, sample_generation_id, cpu_millicores, memory_bytes, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 2000, ?8, 'reserved')",
            (
                intent_id,
                connection_id,
                assignment_key.as_str(),
                TEST_ENGINE_ID,
                "a".repeat(64),
                scope.binding_digest(),
                sample.as_str(),
                i64::try_from(8_u64 << 30).map_err(|_| HostError::Journal)?,
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let reservation_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO capacity_reservation_samples (reservation_id, sample_generation_id, purpose) VALUES (?1, ?2, 'initial')",
            (reservation_id, sample.as_str()),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(reservation_id)
}

async fn count_active_reservations(journal: &Journal) -> Result<i64, HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservations WHERE state != 'released'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

#[tokio::test]
async fn profile_configuration_rejects_an_existing_profile_without_current_pointer()
-> Result<(), HostError> {
    let scratch = super::helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(super::admission::JOB_LIMIT)
        .await?;
    let current = fixture_profile();
    journal.configure_storage_profile(current.clone()).await?;
    let connection = journal.connection().await?;
    connection
        .execute("DROP TRIGGER host_storage_profile_current_no_delete", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "DELETE FROM host_storage_profile_current WHERE singleton = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);

    assert!(journal.current_storage_profile().await?.is_none());
    assert!(
        journal
            .configure_storage_profile(next_profile(&current)?)
            .await
            .is_err()
    );
    assert!(journal.configure_storage_profile(current).await.is_err());
    Ok(())
}

async fn assert_charged_unprovisioned_state_blocks_profile_switch(
    requested_state: &str,
) -> Result<(), HostError> {
    let scratch = super::helpers::Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(super::admission::JOB_LIMIT)
        .await?;
    let binding = super::helpers::binding("profile-legacy", 101)?;
    let scope = super::helpers::created_scope(&journal, &binding, 501).await?;
    let old = fixture_profile();
    journal.configure_storage_profile(old.clone()).await?;

    let assignment = scope
        .assignment_identity(AssignmentKey::AssignedRequest { request_id: 77 })
        .await?;
    let generation = "9".repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&generation, TEST_ENGINE_ID);
    let root = permit.docker_root_digest().to_owned();
    let storage = fixture_admission(&permit)?;
    let result = scope
        .try_reserve_with_storage(
            ReservationRequest {
                assignment: &assignment,
                engine_id: TEST_ENGINE_ID,
                docker_root_digest: &root,
                cpu_millicores: 2_000,
                memory_bytes: 8_u64 << 30,
                sample_generation_id: &generation,
            },
            permit,
            storage,
        )
        .await?;
    let ReservationResult::Reserved { reservation_id } = result else {
        return Err(HostError::Journal);
    };
    match requested_state {
        "reserved" => {}
        "uncertain" => scope.mark_uncertain(reservation_id).await?,
        "recovering" => scope.mark_recovering(reservation_id).await?,
        _ => return Err(HostError::Journal),
    }

    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM capacity_reservation_provisioning WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let count = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(count, 0, "fixture must lack a provisioning record");
    drop(connection);

    journal.configure_storage_profile(old.clone()).await?;
    let next = next_profile(&old)?;
    drop(scope);
    drop(journal);

    let journal = Journal::open(&path).await?;
    assert!(
        journal.configure_storage_profile(next).await.is_err(),
        "{requested_state} reservation without provisioning still holds capacity"
    );
    assert_eq!(
        journal
            .current_storage_profile()
            .await?
            .ok_or(HostError::Journal)?
            .generation_id(),
        old.generation_id()
    );
    Ok(())
}
