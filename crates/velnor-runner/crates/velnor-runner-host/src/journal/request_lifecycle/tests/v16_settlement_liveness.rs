use std::time::Instant;

use super::helpers::{
    create_cleaned_settlement_probe, record_owned_network_cleanup, record_owned_storage_cleanup,
    settlement_sample,
};
use super::review_counterexamples::ready;
use crate::error::HostError;
use crate::journal::{
    Journal, MetadataSettlementResult, ReservationState, StorageAdmissionSample,
    StorageFilesystemKind, StorageMedium, StoragePoolObservation, StorageSampleIdentity,
};

#[path = "v16_settlement_liveness/v16_fixture.rs"]
mod v16_fixture;

// Use separate tests for the version 16 migration and current settlement behavior.
#[tokio::test]
async fn v16_migration_preserves_two_word_filesystem_identity() -> Result<(), HostError> {
    let scratch = super::helpers::Scratch::new()?;
    v16_fixture::preserve_filesystem_words(&scratch.file()).await
}

#[tokio::test]
async fn fresh_settlement_releases_hold_and_survives_restart() -> Result<(), HostError> {
    let super::review_counterexamples::Ready {
        scratch,
        journal,
        binding,
        scope,
        physical,
        provisioned,
        ..
    } = ready().await?;
    let path = scratch.file();
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );

    let reservation = scope
        .reservation(&physical)
        .await?
        .ok_or(HostError::Journal)?;
    record_owned_storage_cleanup(&scope, &provisioned).await?;
    scope
        .record_remote_cleanup(reservation.reservation_id)
        .await?;
    scope
        .record_local_cleanup(reservation.reservation_id)
        .await?;
    record_owned_network_cleanup(&scope, reservation.reservation_id).await?;
    reject_stale_or_mismatched_samples(&scope, &provisioned, &physical).await?;

    let fresh = create_cleaned_settlement_probe(&scope, &provisioned, '4').await?;
    let fresh_sample = settlement_sample(&scope, &fresh).await?;
    assert_eq!(
        scope
            .record_storage_metadata_settlement(&fresh, fresh_sample)
            .await?,
        MetadataSettlementResult::Settled
    );
    assert_eq!(
        scope.reservation(&physical).await?.map(|row| row.state),
        Some(ReservationState::Released)
    );
    drop(scope);
    drop(journal);

    let reopened = Journal::open(&path).await?;
    let reopened_scope = reopened.connection_scope(&binding).await?;
    assert_eq!(
        reopened_scope
            .reservation(&physical)
            .await?
            .map(|row| row.state),
        Some(ReservationState::Released)
    );
    Ok(())
}

async fn reject_stale_or_mismatched_samples(
    scope: &crate::journal::ConnectionJournal,
    provisioned: &super::helpers::ProvisionedRunner,
    physical: &crate::journal::AssignmentIdentity,
) -> Result<(), HostError> {
    let disk_pool = disk_pool_id(scope).await?;
    let wrong_digest = create_cleaned_settlement_probe(scope, provisioned, '1').await?;
    let sample = settlement_sample_with_pool(
        &wrong_digest,
        disk_pool.clone(),
        "b".repeat(64),
        Instant::now(),
    )?;
    assert_settlement_rejected(scope, physical, &wrong_digest, sample).await?;

    let wrong_pool = create_cleaned_settlement_probe(scope, provisioned, '2').await?;
    let sample =
        settlement_sample_with_pool(&wrong_pool, "f".repeat(64), "a".repeat(64), Instant::now())?;
    assert_ne!(disk_pool, sample.pools()[0].pool_id());
    assert_settlement_rejected(scope, physical, &wrong_pool, sample).await?;

    let stale_observed_at = Instant::now();
    let stale = create_cleaned_settlement_probe(scope, provisioned, '3').await?;
    let sample = settlement_sample_with_pool(&stale, disk_pool, "a".repeat(64), stale_observed_at)?;
    assert_settlement_rejected(scope, physical, &stale, sample).await
}

async fn assert_settlement_rejected(
    scope: &crate::journal::ConnectionJournal,
    physical: &crate::journal::AssignmentIdentity,
    claim: &crate::journal::StorageMetadataSettlementClaim,
    sample: StorageAdmissionSample,
) -> Result<(), HostError> {
    assert!(
        scope
            .record_storage_metadata_settlement(claim, sample)
            .await
            .is_err()
    );
    assert_reservation_stays_charged(scope, physical).await
}

async fn assert_reservation_stays_charged(
    scope: &crate::journal::ConnectionJournal,
    physical: &crate::journal::AssignmentIdentity,
) -> Result<(), HostError> {
    assert_eq!(
        scope.reservation(physical).await?.map(|row| row.state),
        Some(ReservationState::Reserved)
    );
    Ok(())
}

async fn disk_pool_id(scope: &crate::journal::ConnectionJournal) -> Result<String, HostError> {
    let profile = scope
        .current_storage_profile()
        .await?
        .ok_or(HostError::Journal)?;
    profile
        .pools()
        .iter()
        .find(|pool| pool.medium() == StorageMedium::Disk)
        .map(|pool| pool.pool_id().to_owned())
        .ok_or(HostError::Journal)
}

fn settlement_sample_with_pool(
    claim: &crate::journal::StorageMetadataSettlementClaim,
    pool_id: String,
    filesystem_instance_digest: String,
    observed_at: Instant,
) -> Result<StorageAdmissionSample, HostError> {
    let identity = StorageSampleIdentity::new(
        claim.probe_operation_id().to_owned(),
        claim.profile_generation_id().to_owned(),
        claim.profile_digest().to_owned(),
        claim.engine_id().to_owned(),
        claim.docker_root_digest().to_owned(),
    )?;
    let pool = StoragePoolObservation::new_with_filesystem(
        pool_id,
        filesystem_instance_digest,
        StorageMedium::Disk,
        StorageFilesystemKind::Btrfs,
        64_u64 << 30,
        60_u64 << 30,
        0,
        0,
    )?;
    StorageAdmissionSample::new(identity, observed_at, vec![pool], None)
}
