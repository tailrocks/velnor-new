use super::helpers::{
    TEST_ENGINE_ID, event_source, record_owned_network_cleanup, record_owned_storage_cleanup,
    record_runner_absent_for_cleanup, record_runner_completion_for_cleanup,
};
use super::review_counterexamples::ready;
use crate::error::HostError;
use crate::journal::{Journal, ReservationState, StorageRole};
use crate::launch::bootstrap::test_verified_storage_cleanup;
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};

mod database;
mod historical_v18;

#[tokio::test]
async fn missing_wrong_and_uncertain_metadata_absence_keep_the_hold_after_reopen()
-> Result<(), HostError> {
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
    fixture
        .scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;

    let names = WorkerIdentityNames::from_nonce(&fixture.provisioned.nonce)?;
    let legacy_names = vec![
        names.volume_name(WorkerVolumeRole::Socket),
        names.volume_name(WorkerVolumeRole::Home),
        names.volume_name(WorkerVolumeRole::Work),
        names.volume_name(WorkerVolumeRole::DindData),
    ];
    let missing_metadata = cleanup_proof(&fixture, legacy_names)?;
    assert!(
        fixture
            .scope
            .record_storage_cleanup(missing_metadata)
            .await
            .is_err(),
        "Four-object recovery proof must not establish cleanup for a topology 2 capacity reservation."
    );

    assert!(
        cleanup_proof(&fixture, Vec::new()).is_err(),
        "The proof constructor must reject an empty object list without indexing or a panic."
    );

    let mut wrong_metadata_names = names_for_topology_two(&names);
    wrong_metadata_names[4] = format!("{}-metadata-other", names.private_volume());
    assert!(
        cleanup_proof(&fixture, wrong_metadata_names).is_err(),
        "the typed proof rejects an unowned metadata volume name"
    );

    let wrong_owner = test_verified_storage_cleanup(
        fixture.provisioned.reservation_id,
        fixture.provisioned.intent_id + 1,
        TEST_ENGINE_ID.to_owned(),
        fixture.provisioned.docker_root_digest.clone(),
        fixture.provisioned.runner_container_id.clone(),
        fixture.provisioned.dind_container_id.clone(),
        names_for_topology_two(&names),
        fixture.provisioned.instance_binding_digest.clone(),
        StorageRole::PHYSICAL.to_vec(),
        true,
        true,
        true,
        true,
    )?;
    assert!(
        fixture
            .scope
            .record_storage_cleanup(wrong_owner)
            .await
            .is_err(),
        "The journal must reject a correct object inventory from another operation."
    );

    let uncertain = test_verified_storage_cleanup(
        fixture.provisioned.reservation_id,
        fixture.provisioned.intent_id,
        TEST_ENGINE_ID.to_owned(),
        fixture.provisioned.docker_root_digest.clone(),
        fixture.provisioned.runner_container_id.clone(),
        fixture.provisioned.dind_container_id.clone(),
        names_for_topology_two(&names),
        fixture.provisioned.instance_binding_digest.clone(),
        StorageRole::PHYSICAL.to_vec(),
        true,
        false,
        true,
        true,
    );
    assert!(
        uncertain.is_err(),
        "Unverified volume absence must not produce cleanup proof."
    );

    assert_eq!(
        fixture
            .scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_cleanup_flag(&fixture.journal, fixture.provisioned.reservation_id, 0).await?;

    let path = fixture.scratch.file();
    let binding = fixture.binding.clone();
    let physical = fixture.physical.clone();
    let provisioned = fixture.provisioned.clone();
    drop(fixture.scope);
    drop(fixture.journal);

    let reopened = Journal::open(&path).await?;
    let reopened_scope = reopened.connection_scope(&binding).await?;
    assert_eq!(
        reopened_scope
            .reservation(&physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    assert_cleanup_flag(&reopened, provisioned.reservation_id, 0).await?;
    assert!(
        super::helpers::create_cleaned_settlement_probe(&reopened_scope, &provisioned, '6')
            .await
            .is_err(),
        "metadata settlement cannot start without complete owned-object cleanup"
    );
    Ok(())
}

#[tokio::test]
async fn persisted_four_object_cleanup_cannot_set_v19_cleanup_proven() -> Result<(), HostError> {
    let fixture = ready().await?;
    let names = WorkerIdentityNames::from_nonce(&fixture.provisioned.nonce)?;
    let connection = fixture.journal.connection().await?;
    let inserted = connection
        .execute(
            "INSERT INTO capacity_reservation_provisioning_cleanups (reservation_id, runner_container_id, dind_container_id, shared_run_volume, runner_home_volume, runner_work_volume, dind_data_volume, metadata_volume, instance_binding_digest, role_mask, containers_absent, volumes_absent, mounts_detached, cgroups_absent) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, 2047, 1, 1, 1, 1)",
            (
                fixture.provisioned.reservation_id,
                fixture.provisioned.runner_container_id.as_str(),
                fixture.provisioned.dind_container_id.as_str(),
                names.volume_name(WorkerVolumeRole::Socket),
                names.volume_name(WorkerVolumeRole::Home),
                names.volume_name(WorkerVolumeRole::Work),
                names.volume_name(WorkerVolumeRole::DindData),
                fixture.provisioned.instance_binding_digest.as_str(),
            ),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    assert_eq!(inserted, 1);

    let promoted = connection
        .execute(
            "UPDATE capacity_reservation_provisioning_state SET storage_cleanup_proven = 1 WHERE reservation_id = ?1",
            [fixture.provisioned.reservation_id],
        )
        .await;
    assert!(
        promoted.is_err(),
        "database guard rejects missing metadata proof"
    );
    drop(connection);

    let path = fixture.scratch.file();
    drop(fixture.scope);
    drop(fixture.journal);
    assert!(
        Journal::open(&path).await.is_err(),
        "Journal::open must reject a topology 2 cleanup row with missing object proof."
    );
    Ok(())
}

#[tokio::test]
async fn exact_five_object_cleanup_settles_only_after_all_owned_absence() -> Result<(), HostError> {
    let fixture = ready().await?;
    let reservation_id = fixture.provisioned.reservation_id;
    let cleanup_generation = record_runner_completion_for_cleanup(
        &fixture.scope,
        &fixture.request,
        fixture.intent,
        Some(701),
        703,
        &event_source(101, 1070),
        &fixture.runner,
    )
    .await?;
    record_owned_storage_cleanup(&fixture.scope, &fixture.provisioned).await?;
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved),
        "owned-object absence alone does not release capacity"
    );
    fixture.scope.record_remote_cleanup(reservation_id).await?;
    fixture.scope.record_local_cleanup(reservation_id).await?;
    record_owned_network_cleanup(&fixture.scope, reservation_id).await?;
    record_runner_absent_for_cleanup(&fixture.scope, fixture.intent, cleanup_generation).await?;
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Reserved),
        "metadata settlement remains required after local and remote cleanup"
    );
    let settlement =
        super::helpers::settle_storage_metadata(&fixture.scope, &fixture.provisioned, '7').await?;
    assert_eq!(
        settlement,
        crate::journal::MetadataSettlementResult::Settled
    );
    assert_eq!(
        fixture
            .scope
            .reservation(&fixture.physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Released)
    );
    Ok(())
}

fn cleanup_proof(
    fixture: &super::review_counterexamples::Ready,
    volume_names: Vec<String>,
) -> Result<crate::launch::bootstrap::VerifiedStorageCleanup, HostError> {
    test_verified_storage_cleanup(
        fixture.provisioned.reservation_id,
        fixture.provisioned.intent_id,
        TEST_ENGINE_ID.to_owned(),
        fixture.provisioned.docker_root_digest.clone(),
        fixture.provisioned.runner_container_id.clone(),
        fixture.provisioned.dind_container_id.clone(),
        volume_names,
        fixture.provisioned.instance_binding_digest.clone(),
        StorageRole::PHYSICAL.to_vec(),
        true,
        true,
        true,
        true,
    )
}

fn names_for_topology_two(names: &WorkerIdentityNames) -> Vec<String> {
    vec![
        names.volume_name(WorkerVolumeRole::Socket),
        names.volume_name(WorkerVolumeRole::Home),
        names.volume_name(WorkerVolumeRole::Work),
        names.volume_name(WorkerVolumeRole::DindData),
        names.volume_name(WorkerVolumeRole::RunnerMetadata),
    ]
}

async fn assert_cleanup_flag(
    journal: &Journal,
    reservation_id: i64,
    expected: i64,
) -> Result<(), HostError> {
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT storage_cleanup_proven FROM capacity_reservation_provisioning_state WHERE reservation_id = ?1",
            [reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let value = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get::<i64>(0)
        .map_err(|_| HostError::Journal)?;
    assert_eq!(value, expected);
    Ok(())
}
