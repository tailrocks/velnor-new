use crate::error::HostError;
use crate::journal::{Journal, ReservationState, StorageRole};
use crate::launch::bootstrap::test_verified_storage_cleanup;
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

use super::super::helpers::TEST_ENGINE_ID;
use super::database::{query_i64, query_optional_text};
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
    let reservation_id = identity.reservation_id;
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_eq!(
        query_i64(
            &migrated,
            "SELECT topology_version FROM capacity_reservation_provisioning_topology WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        1
    );
    assert_eq!(
        query_i64(
            &migrated,
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
    scope.record_storage_cleanup(legacy_proof).await?;
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved),
        "the four-object cleanup proof leaves the metadata charge held"
    );
    scope.record_remote_cleanup(reservation_id).await?;
    scope.record_local_cleanup(reservation_id).await?;
    let settlement =
        historical_settlement_probe(&scope, reservation_id, &identity.docker_root_digest, '8')
            .await?;
    let sample = super::super::helpers::settlement_sample(&scope, &settlement).await?;
    assert_eq!(
        scope
            .record_storage_metadata_settlement(&settlement, sample)
            .await?,
        crate::journal::MetadataSettlementResult::Settled
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Released)
    );
    assert_eq!(
        query_optional_text(
            &migrated,
            "SELECT metadata_volume FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        None
    );
    drop(scope);
    drop(migrated);

    let reopened = Journal::open(&path).await?;
    let reopened_scope = reopened.connection_scope(&binding).await?;
    assert_eq!(
        reopened_scope
            .reservation(&physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Released)
    );
    assert_eq!(
        query_optional_text(
            &reopened,
            "SELECT metadata_volume FROM capacity_reservation_provisioning_cleanups WHERE reservation_id = ?1",
            reservation_id,
        )
        .await?,
        None,
        "After Journal::open, v18 cleanup keeps its historical four-object proof."
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

async fn historical_settlement_probe(
    scope: &crate::journal::ConnectionJournal,
    reservation_id: i64,
    docker_root_digest: &str,
    probe_nonce: char,
) -> Result<crate::journal::StorageMetadataSettlementClaim, HostError> {
    use crate::journal::{OperationalCapacityLimits, ProbePhase, ProbeSeed};
    use crate::launch::VerifiedProbeCleanup;

    let journal = scope.journal_ref();
    journal
        .configure_capacity_with_operational_limits(
            super::super::admission::JOB_LIMIT,
            OperationalCapacityLimits {
                cpu_millicores: 1_000,
                memory_bytes: 128 * 1024 * 1024,
            },
        )
        .await?;
    let operation_id = probe_nonce.to_string().repeat(32);
    let operation_name = format!("velnor-resource-probe-{operation_id}");
    let seed = ProbeSeed {
        operation_id: operation_id.clone(),
        instance_id: journal.instance_id().await?,
        engine_id: TEST_ENGINE_ID.to_owned(),
        docker_root_digest: docker_root_digest.to_owned(),
        source_revision: "b".repeat(40),
        runtime_image_id: format!("sha256:{}", "c".repeat(64)),
        image_binding_digest: "d".repeat(64),
        operation_name: operation_name.clone(),
        projection_digest: "e".repeat(64),
        purpose: crate::journal::ResourceProbePurpose::Capacity,
        request_sha256: crate::journal::capacity_request_sha256(),
    };
    let claim = scope
        .prepare_storage_metadata_settlement_probe(reservation_id, seed)
        .await?;
    journal
        .transition_probe(
            &operation_id,
            ProbePhase::Prepared,
            ProbePhase::CreateRequested,
        )
        .await?;
    let container_id = "f".repeat(64);
    journal
        .bind_probe_container(&operation_id, &container_id)
        .await?;
    journal
        .transition_probe(
            &operation_id,
            ProbePhase::ContainerCreated,
            ProbePhase::RemoveRequested,
        )
        .await?;
    journal
        .retire_probe(VerifiedProbeCleanup::test_only(
            operation_id,
            TEST_ENGINE_ID.to_owned(),
            docker_root_digest.to_owned(),
            ProbePhase::RemoveRequested,
            operation_name,
            Some(container_id),
        ))
        .await?;
    Ok(claim)
}
