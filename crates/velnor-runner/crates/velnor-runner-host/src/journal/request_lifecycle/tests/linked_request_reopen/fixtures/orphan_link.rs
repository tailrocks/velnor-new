use super::LinkedLifecycle;
use crate::error::HostError;
use crate::journal::{
    AssignmentKey, ConnectionJournal, Journal, ReservationRequest,
    fixture_storage_provisioning_admission,
};
use crate::launch::StartPermit;

use super::super::super::helpers::TEST_ENGINE_ID;

pub(in super::super) async fn reject_orphaned_reservation_link(
    fixture: LinkedLifecycle,
) -> Result<(), HostError> {
    let LinkedLifecycle {
        scratch,
        path,
        journal,
        scope,
        actual_request_id,
        physical,
        ..
    } = fixture;
    insert_orphaned_reservation_link(&path, actual_request_id).await?;
    assert_orphaned_link_replay_rejected(&scope, &physical, actual_request_id).await?;
    drop(scope);
    drop(journal);
    assert!(Journal::open(&path).await.is_err());
    drop(scratch);
    Ok(())
}

async fn insert_orphaned_reservation_link(
    path: &std::path::Path,
    request_id: i64,
) -> Result<(), HostError> {
    let path_text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    let mut foreign_keys = connection
        .query("PRAGMA foreign_keys", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(
        foreign_keys
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?,
        0
    );
    let mut trigger = connection
        .query(
            "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = 'connection_request_reservations_insert_guard'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let trigger_sql = trigger
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<String>(0)
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "DROP TRIGGER connection_request_reservations_insert_guard",
            (),
        )
        .await
        .map_err(|error| panic!("Turso did not remove the insert trigger: {error:?}."))?;
    connection
        .execute(
            "INSERT INTO connection_request_reservations (connection_id, scale_set_id, request_id, reservation_id, physical_nonce, binding_digest) SELECT connection_id, scale_set_id, request_id, 2147483647, ?1, binding_digest FROM connection_request_authorizations WHERE request_id = ?2 AND acquire_state = 'acquired' LIMIT 1",
            ("e".repeat(32), request_id),
        )
        .await
        .map_err(|error| panic!("Turso did not insert the request link: {error:?}."))?;
    connection
        .execute(&trigger_sql, ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut orphan = connection
        .query(
            "SELECT COUNT(*) FROM connection_request_reservations AS link LEFT JOIN capacity_reservations AS reservation ON reservation.id = link.reservation_id WHERE link.request_id = ?1 AND reservation.id IS NULL",
            [request_id],
        )
        .await
        .map_err(|error| panic!("Turso did not count request links: {error:?}."))?;
    assert_eq!(
        orphan
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?,
        1
    );
    drop(connection);
    drop(database);
    Ok(())
}

async fn assert_orphaned_link_replay_rejected(
    scope: &ConnectionJournal,
    physical: &crate::journal::AssignmentIdentity,
    actual_request_id: i64,
) -> Result<(), HostError> {
    let sample_generation = "9".repeat(32);
    let permit = StartPermit::test_fixture_with_engine_id(&sample_generation, TEST_ENGINE_ID);
    let root_digest = permit.docker_root_digest().to_owned();
    let sample_generation_id = permit.sample_generation_id().to_owned();
    let admission = fixture_storage_provisioning_admission(&permit)?;
    let actual_request = scope
        .assignment_identity(AssignmentKey::AssignedRequest {
            request_id: actual_request_id,
        })
        .await?;
    assert!(
        scope
            .try_reserve_and_claim_request_acquire(
                ReservationRequest {
                    assignment: physical,
                    engine_id: TEST_ENGINE_ID,
                    docker_root_digest: &root_digest,
                    cpu_millicores: 2_000,
                    memory_bytes: super::super::JOB_MEMORY,
                    sample_generation_id: &sample_generation_id,
                },
                permit,
                admission,
                &actual_request,
            )
            .await
            .is_err(),
        "The journal must reject the request link without a reservation."
    );
    Ok(())
}
