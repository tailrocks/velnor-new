use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::storage::read_transaction;

use super::super::admission::JOB_LIMIT;
use super::super::helpers::{Scratch, binding, created_scope};

type InitializationState = (
    (String, String, String, String),
    (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ),
);
type StorageHandoff = (String, String, String, String, String, String);

#[tokio::test]
async fn storage_initialization_is_durable_before_job_container_creation() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_capacity(JOB_LIMIT).await?;
    let scope = created_scope(&journal, &binding("storage-initialization", 941)?, 1941).await?;
    let provisioned = Box::pin(super::super::helpers::provision_physical_intent(
        &scope, 'd',
    ))
    .await?;

    let state = read_initialization_state(&scope, provisioned.reservation_id).await?;
    assert_initialization_state(&state, &provisioned);

    let handoff = read_storage_handoff(&scope, provisioned.reservation_id).await?;
    assert_storage_handoff(&handoff, &provisioned);

    drop(scope);
    drop(journal);
    assert_reopened_initialization_removed(&scratch, &provisioned).await?;
    Ok(())
}

async fn read_initialization_state(
    scope: &crate::journal::connection::ConnectionJournal,
    reservation_id: i64,
) -> Result<InitializationState, HostError> {
    Box::pin(read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection.query(
            "SELECT create_state, start_state, result_state, remove_state, operation_key, image_runtime_id, image_source_sha, engine_id, docker_root_digest, home_volume, work_volume, helper_id, home_inode, work_inode FROM capacity_reservation_storage_initialization WHERE reservation_id = ?1",
            [reservation_id],
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
    .await
}

fn assert_initialization_state(
    state: &InitializationState,
    provisioned: &super::super::helpers::ProvisionedRunner,
) {
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
    assert_eq!(state.1.3, super::super::helpers::TEST_ENGINE_ID);
    assert_eq!(state.1.4, provisioned.docker_root_digest);
    assert!(state.1.5.ends_with("-home"));
    assert!(state.1.6.ends_with("-work"));
    assert_eq!(state.1.7.len(), 64);
}

async fn read_storage_handoff(
    scope: &crate::journal::connection::ConnectionJournal,
    reservation_id: i64,
) -> Result<StorageHandoff, HostError> {
    Box::pin(read_transaction(scope.journal_ref(), async |connection| {
        let mut rows = connection.query(
            "SELECT state, layout_digest, ready_digest, mount_evidence_digest, runner_container_id, dind_container_id FROM capacity_reservation_storage_handoffs WHERE reservation_id = ?1",
            [reservation_id],
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
    }))
    .await
}

fn assert_storage_handoff(
    handoff: &StorageHandoff,
    provisioned: &super::super::helpers::ProvisionedRunner,
) {
    assert_eq!(handoff.0, "attached");
    assert_eq!(handoff.1.len(), 64);
    assert_eq!(handoff.2.len(), 64);
    assert_ne!(handoff.1, handoff.2);
    assert_eq!(handoff.3.len(), 64);
    assert_eq!(handoff.4, provisioned.runner_container_id);
    assert_eq!(handoff.5, provisioned.dind_container_id);
}

async fn assert_reopened_initialization_removed(
    scratch: &Scratch,
    provisioned: &super::super::helpers::ProvisionedRunner,
) -> Result<(), HostError> {
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
