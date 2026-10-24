use super::*;

#[tokio::test]
async fn first_profile_keeps_legacy_capacity_charged_and_blocks_admission() -> Result<(), HostError>
{
    let scratch = super::super::helpers::Scratch::new()?;
    let path = scratch.file();
    let journal = Journal::open(&path).await?;
    journal
        .configure_capacity(super::super::admission::JOB_LIMIT)
        .await?;
    journal.bind_engine(TEST_ENGINE_ID).await?;
    let binding = super::super::helpers::binding("profile-initial-legacy", 102)?;
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
    let scratch = super::super::helpers::Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal
        .configure_capacity(super::super::admission::JOB_LIMIT)
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
