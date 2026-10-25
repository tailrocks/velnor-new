use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::read_transaction;

use super::admission::JOB_LIMIT;
use super::helpers::{Scratch, binding, created_scope};

#[path = "initialization/metadata_claim.rs"]
mod metadata_claim;
mod storage_lifecycle;

mod cleanup_guard;
mod late_initializer;
mod v21_migration;

#[tokio::test]
async fn migration_rejects_home_volume_without_matching_created_object() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-volume-corruption", 942)?, 1942).await?;
    let provisioned = Box::pin(super::helpers::provision_physical_intent(&scope, 'e')).await?;
    let path = scratch.file();
    drop(scope);
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA foreign_keys = OFF", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("DROP TRIGGER storage_initialization_identity_immutable", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservation_storage_initialization SET home_volume = 'foreign-home' WHERE reservation_id = ?1",
            [provisioned.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 19", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn migration_rejects_malformed_helper_id_in_uncertain_state() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-helper-corruption", 943)?, 1943).await?;
    let provisioned = Box::pin(super::helpers::provision_physical_intent(&scope, 'f')).await?;
    let path = scratch.file();
    drop(scope);
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA ignore_check_constraints = ON", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "DROP TRIGGER storage_initialization_create_transition_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservation_storage_initialization SET create_state = 'uncertain', helper_id = 'bad' WHERE reservation_id = ?1",
            [provisioned.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 19", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn migration_rejects_confirmed_home_and_work_that_share_one_root() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-root-corruption", 944)?, 1944).await?;
    let provisioned = Box::pin(super::helpers::provision_physical_intent(&scope, 'a')).await?;
    let path = scratch.file();
    drop(scope);
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA ignore_check_constraints = ON", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "DROP TRIGGER storage_initialization_result_transition_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservation_storage_initialization SET work_device_major = home_device_major, work_device_minor = home_device_minor, work_inode = home_inode, work_mount_id = home_mount_id WHERE reservation_id = ?1",
            [provisioned.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 19", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn migration_rejects_device_numbers_outside_u32() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-device-corruption", 946)?, 1946).await?;
    let provisioned = Box::pin(super::helpers::provision_physical_intent(&scope, '7')).await?;
    let path = scratch.file();
    drop(scope);
    drop(journal);

    let database = turso::Builder::new_local(path.to_str().ok_or(HostError::Path)?)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA ignore_check_constraints = ON", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "DROP TRIGGER storage_initialization_result_transition_guard",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "UPDATE capacity_reservation_storage_initialization SET home_device_major = 4294967296 WHERE reservation_id = ?1",
            [provisioned.reservation_id],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 19", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    Ok(())
}

#[tokio::test]
async fn initializer_phase_update_increments_journal_revision() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-revision", 945)?, 1945).await?;
    let reservation_id = super::helpers::prepare_physical_initializer(&scope, 'c').await?;
    let images = crate::launch::verified_images::test_worker_images();
    let image =
        crate::worker::storage_initialization::StorageInitializationImage::from_verified_release(
            &images,
        )?;
    let claim = scope
        .claim_storage_initialization(
            reservation_id,
            &image,
            super::helpers::test_initializer_limits()?,
        )
        .await?
        .ok_or(HostError::Journal)?;
    let before = journal.revision().await?;
    let response =
        crate::worker::storage_initialization::StorageInitializerCreateResponse::test_response(
            &claim,
            &"a".repeat(64),
        );

    scope
        .record_storage_initializer_create_dispatched(&claim)
        .await?;
    scope
        .record_storage_initializer_create_response_received(&claim)
        .await?;
    scope
        .record_storage_initializer_create_response(&claim, &response)
        .await?;

    assert_eq!(journal.revision().await?, before + 3);
    Ok(())
}

#[path = "initialization/recovery.rs"]
mod recovery;

#[path = "initialization/recovery_reopen.rs"]
mod recovery_reopen;
