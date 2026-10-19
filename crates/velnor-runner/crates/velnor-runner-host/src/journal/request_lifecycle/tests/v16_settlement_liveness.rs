use std::path::Path;
use std::time::Instant;

use super::helpers::{
    create_cleaned_settlement_probe, record_owned_storage_cleanup, settlement_sample,
};
use super::review_counterexamples::ready;
use crate::error::HostError;
use crate::journal::{
    Journal, MetadataSettlementResult, ReservationState, StorageAdmissionSample,
    StorageFilesystemKind, StorageMedium, StoragePoolObservation, StorageSampleIdentity,
};

#[tokio::test]
async fn v16_filesystem_words_remain_provenance_while_fresh_settlement_releases_hold()
-> Result<(), HostError> {
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
    drop(scope);
    drop(journal);
    restore_v16_filesystem_columns(&path).await?;

    let journal = Journal::open(&path).await?;
    assert_legacy_words(&journal, provisioned.reservation_id).await?;
    let scope = journal.connection_scope(&binding).await?;
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
    assert_legacy_words(&journal, provisioned.reservation_id).await?;
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
    assert_legacy_words(&reopened, provisioned.reservation_id).await?;
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

async fn assert_legacy_words(journal: &Journal, reservation_id: i64) -> Result<(), HostError> {
    let connection = journal.connection().await?;
    for table in [
        "capacity_reservation_bound_storage_attachments",
        "capacity_reservation_provisioning_publications",
    ] {
        let mut rows = connection
            .query(
                &format!("SELECT filesystem_id, v16_filesystem_id_0, v16_filesystem_id_1 FROM {table} WHERE reservation_id = ?1"),
                [reservation_id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let mut seen = 0_usize;
        while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
            assert_eq!(
                row.get::<Option<String>>(0)
                    .map_err(|_| HostError::Journal)?,
                None
            );
            assert_eq!(row.get::<i64>(1).map_err(|_| HostError::Journal)?, 123);
            assert_eq!(row.get::<i64>(2).map_err(|_| HostError::Journal)?, 456);
            seen = seen.checked_add(1).ok_or(HostError::Journal)?;
        }
        assert!(seen > 0, "expected migrated rows in {table}");
    }
    Ok(())
}

async fn restore_v16_filesystem_columns(path: &Path) -> Result<(), HostError> {
    let path_text = path.to_str().ok_or(HostError::Path)?;
    let database = turso::Builder::new_local(path_text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for table in [
        "capacity_reservation_bound_storage_attachments",
        "capacity_reservation_provisioning_publications",
    ] {
        replace_with_v16_columns(&connection, table).await?;
    }
    connection
        .execute("PRAGMA user_version = 16", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);
    Ok(())
}

async fn replace_with_v16_columns(
    connection: &turso::Connection,
    table: &str,
) -> Result<(), HostError> {
    let mut rows = connection
        .query(&format!("PRAGMA table_info({table})"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let name = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        match name.as_str() {
            "filesystem_id" => {}
            "v16_filesystem_id_0" => columns.push("123 AS filesystem_id_0".to_owned()),
            "v16_filesystem_id_1" => columns.push("456 AS filesystem_id_1".to_owned()),
            _ => columns.push(name),
        }
    }
    drop(rows);
    let temporary = format!("{table}_test_v16");
    connection
        .execute(
            &format!(
                "CREATE TABLE {temporary} AS SELECT {} FROM {table}",
                columns.join(",")
            ),
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("DROP TABLE {table}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(&format!("ALTER TABLE {temporary} RENAME TO {table}"), ())
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
