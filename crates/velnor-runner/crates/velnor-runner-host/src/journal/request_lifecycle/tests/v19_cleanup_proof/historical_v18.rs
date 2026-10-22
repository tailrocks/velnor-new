use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::journal::{AssignmentIdentity, ConnectionJournal};
use crate::journal::{Journal, ReservationState, StorageRole};
use crate::launch::bootstrap::test_verified_storage_cleanup;
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

use super::super::helpers::TEST_ENGINE_ID;
use super::database::query_i64;
#[tokio::test]
async fn migrated_v18_cleanup_uses_four_owned_volumes_and_keeps_metadata_held_until_settlement()
-> Result<(), HostError> {
    let scratch = super::super::helpers::Scratch::new()?;
    let path = scratch.file();
    super::super::v18_topology_migration::support::install_v18_bound_fixture(&path)?;
    super::super::v18_topology_migration::support::assert_fixture_version_eighteen(&path).await?;
    let binding = super::super::helpers::binding("topology-migration", 303)?;
    let migrated = Journal::open(&path).await?;
    let scope = migrated.connection_scope(&binding).await?;
    let physical = scope
        .assignment_identity(crate::journal::AssignmentKey::Remote {
            kind: "runner".to_owned(),
            subject: "22222222222222222222222222222222".to_owned(),
        })
        .await?;
    let identity = read_v18_cleanup_identity(&migrated).await?;
    assert_legacy_cleanup_keeps_reservation_held(&migrated, &scope, &physical, &identity).await?;
    drop(scope);
    drop(migrated);
    assert_reopened_v18_cleanup_remains_unproven(&path, &binding, &physical, &identity).await?;
    Ok(())
}

async fn assert_legacy_cleanup_keeps_reservation_held(
    migrated: &Journal,
    scope: &ConnectionJournal,
    physical: &AssignmentIdentity,
    identity: &V18CleanupIdentity,
) -> Result<(), HostError> {
    let reservation_id = identity.reservation_id;
    assert_eq!(
        scope.reservation(physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_eq!(
        query_i64(
            migrated,
            "SELECT topology_version FROM capacity_reservation_provisioning_topology WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        1
    );
    assert_eq!(
        query_i64(
            migrated,
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_objects WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        4
    );

    let names = WorkerIdentityNames::from_nonce("22222222222222222222222222222222")?;
    let legacy_proof = test_verified_storage_cleanup(
        reservation_id,
        identity.intent_id,
        TEST_ENGINE_ID.to_owned(),
        identity.docker_root_digest.clone(),
        identity.runner_container_id.clone(),
        identity.dind_container_id.clone(),
        vec![
            names.volume_name(WorkerVolumeRole::Socket),
            names.volume_name(WorkerVolumeRole::Home),
            names.volume_name(WorkerVolumeRole::Work),
            names.volume_name(WorkerVolumeRole::DindData),
        ],
        identity.instance_binding_digest.clone(),
        StorageRole::PHYSICAL.to_vec(),
        true,
        true,
        true,
        true,
    )?;
    let cleanup_result = scope.record_storage_cleanup(legacy_proof).await;
    assert!(matches!(cleanup_result, Err(HostError::Journal)));
    assert_eq!(
        scope.reservation(physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved),
        "The v18 record has no cleanup observer witness. The storage reservation stays reserved."
    );
    assert_eq!(
        query_i64(
            migrated,
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        0,
        "The failed transaction must not create a cleanup receipt."
    );
    assert_eq!(
        query_i64(
            migrated,
            "SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        0,
        "The failed transaction must not record cleanup proof."
    );
    assert_eq!(
        scope.reservation(physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved),
        "The storage reservation remains reserved."
    );
    Ok(())
}

async fn assert_reopened_v18_cleanup_remains_unproven(
    path: &std::path::Path,
    binding: &RepositoryConnection,
    physical: &AssignmentIdentity,
    identity: &V18CleanupIdentity,
) -> Result<(), HostError> {
    let reopened = Journal::open(path).await?;
    let reopened_scope = reopened.connection_scope(binding).await?;
    let reservation_id = identity.reservation_id;
    assert_eq!(
        reopened_scope
            .reservation(physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_eq!(
        query_i64(
            &reopened,
            "SELECT COUNT(*) FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        0,
        "Reopening the journal must not add a cleanup receipt for a v18 row."
    );
    assert_eq!(
        query_i64(
            &reopened,
            "SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        0
    );
    Ok(())
}

struct V18CleanupIdentity {
    reservation_id: i64,
    intent_id: i64,
    docker_root_digest: String,
    runner_container_id: String,
    dind_container_id: String,
    instance_binding_digest: String,
}

async fn read_v18_cleanup_identity(journal: &Journal) -> Result<V18CleanupIdentity, HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT reservation.id, reservation.intent_id, reservation.docker_root_digest, state.runner_container_id, state.dind_container_id, state.instance_binding_digest FROM capacity_reservations AS reservation JOIN capacity_reservation_provisioning_state AS state ON state.reservation_id = reservation.id WHERE reservation.id = 1",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let row = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    let identity = V18CleanupIdentity {
        reservation_id: row.get(0).map_err(|_| HostError::Journal)?,
        intent_id: row.get(1).map_err(|_| HostError::Journal)?,
        docker_root_digest: row.get(2).map_err(|_| HostError::Journal)?,
        runner_container_id: row.get(3).map_err(|_| HostError::Journal)?,
        dind_container_id: row.get(4).map_err(|_| HostError::Journal)?,
        instance_binding_digest: row.get(5).map_err(|_| HostError::Journal)?,
    };
    if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
        return Err(HostError::Journal);
    }
    Ok(identity)
}
