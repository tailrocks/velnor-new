use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::read_transaction;

use super::admission::JOB_LIMIT;
use super::helpers::{Scratch, binding, created_scope};

#[path = "initialization/metadata_claim.rs"]
mod metadata_claim;

mod cleanup_guard;
mod late_initializer;
mod v21_migration;

#[tokio::test]
async fn storage_initialization_is_durable_before_job_container_creation() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-initialization", 941)?, 1941).await?;
    let provisioned = Box::pin(super::helpers::provision_physical_intent(&scope, 'd')).await?;

    let state = Box::pin(read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection.query(
            "SELECT create_state, start_state, result_state, remove_state, operation_key, image_runtime_id, image_source_sha, engine_id, docker_root_digest, home_volume, work_volume, helper_id, home_inode, work_inode FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
            [provisioned.reservation_id],
        ).await.map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let state = (
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
            row.get::<String>(2).map_err(|_| HostError::Journal)?,
            row.get::<String>(3).map_err(|_| HostError::Journal)?,
        );
        let extra = (
            row.get::<String>(4).map_err(|_| HostError::Journal)?,
            row.get::<String>(5).map_err(|_| HostError::Journal)?,
            row.get::<String>(6).map_err(|_| HostError::Journal)?,
            row.get::<String>(7).map_err(|_| HostError::Journal)?,
            row.get::<String>(8).map_err(|_| HostError::Journal)?,
            row.get::<String>(9).map_err(|_| HostError::Journal)?,
            row.get::<String>(10).map_err(|_| HostError::Journal)?,
            row.get::<String>(11).map_err(|_| HostError::Journal)?,
        );
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        Ok((state, extra))
    }))
    .await?;

    assert_eq!(
        state.0,
        (
            "created".to_owned(),
            "started".to_owned(),
            "confirmed".to_owned(),
            "removed".to_owned()
        )
    );
    assert_eq!(state.1.0, format!("{}-1", provisioned.intent_id));
    assert!(state.1.1.starts_with("sha256:"));
    assert_eq!(state.1.2.len(), 40);
    assert_eq!(state.1.3, super::helpers::TEST_ENGINE_ID);
    assert_eq!(state.1.4, provisioned.docker_root_digest);
    assert!(state.1.5.ends_with("-home"));
    assert!(state.1.6.ends_with("-work"));
    assert_eq!(state.1.7.len(), 64);

    let handoff = Box::pin(read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection.query(
            "SELECT state, layout_digest, ready_digest, mount_evidence_digest, runner_container_id, dind_container_id FROM capacity_reservation_storage_handoffs WHERE reservation_id = ?1",
            [provisioned.reservation_id],
        ).await.map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        let values = (
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<String>(1).map_err(|_| HostError::Journal)?,
            row.get::<String>(2).map_err(|_| HostError::Journal)?,
            row.get::<String>(3).map_err(|_| HostError::Journal)?,
            row.get::<String>(4).map_err(|_| HostError::Journal)?,
            row.get::<String>(5).map_err(|_| HostError::Journal)?,
        );
        if rows.next().await.map_err(|_| HostError::Journal)?.is_some() {
            return Err(HostError::Journal);
        }
        Ok(values)
    })).await?;
    assert_eq!(handoff.0, "attached");
    assert_eq!(handoff.1.len(), 64);
    assert_eq!(handoff.2.len(), 64);
    assert_ne!(handoff.1, handoff.2);
    assert_eq!(handoff.3.len(), 64);
    assert_eq!(handoff.4, provisioned.runner_container_id);
    assert_eq!(handoff.5, provisioned.dind_container_id);

    drop(scope);
    drop(journal);
    let reopened = Journal::open(&scratch.file()).await?;
    let reopened_scope = reopened
        .connection_scope(&binding("storage-initialization", 941)?)
        .await?;
    let persisted = Box::pin(read_transaction(reopened_scope.journal_ref(), async |connection| {
        let mut rows = connection.query(
            "SELECT remove_state, home_inode, work_inode FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
            [provisioned.reservation_id],
        ).await.map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?.ok_or(HostError::Journal)?;
        Ok((
            row.get::<String>(0).map_err(|_| HostError::Journal)?,
            row.get::<i64>(1).map_err(|_| HostError::Journal)?,
            row.get::<i64>(2).map_err(|_| HostError::Journal)?,
        ))
    }))
    .await?;
    assert_eq!(persisted, ("removed".to_owned(), 61, 62));
    Ok(())
}

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
