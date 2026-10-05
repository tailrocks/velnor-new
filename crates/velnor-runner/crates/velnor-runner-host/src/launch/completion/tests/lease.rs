use super::*;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::PathBuf;
use tar::{Builder, Header};

use crate::action_archive_seed::{ActionArchiveIdentity, ActionArchiveLease, ActionArchiveStore};

#[tokio::test]
async fn missing_archive_lease_keeps_completed_worker_occupied() -> Result<(), String> {
    let (_scratch, journal) = open("completion-missing-lease").await?;
    let (id, identity, runner_id, dind_id) = launch(&journal, 7, 89).await?;
    journal
        .bind_seed_generation(id, &"a".repeat(64))
        .await
        .map_err(|error| error.to_string())?;
    let runner_name = format!("v{}", identity.launch_id());
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)?;
    let removed_volumes = engine.removed_volumes.clone();
    let api = BlockingRunnerApi::released(&runner_name, 99);

    completion::record_completion_events(&journal, 7, &completion_poll(89, 99, &runner_name))
        .await
        .map_err(|error| error.to_string())?;
    let tasks = completion::schedule_completed_isolated(
        api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine,
    )
    .await
    .map_err(|error| error.to_string())?;
    for task in tasks {
        task.await.map_err(|error| error.to_string())?;
    }

    assert_eq!(api.calls()?, Vec::new());
    assert!(!removed_volumes.load(Ordering::Acquire));
    assert_eq!(journal.occupied_launches().await, Ok(1));
    assert!(
        !journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    Ok(())
}

#[tokio::test]
async fn seeded_worker_cleanup_retires_existing_archive_lease() -> Result<(), String> {
    let (_scratch, journal) = open("completion-seeded-lease-cleanup").await?;
    let (id, identity, runner_id, dind_id) = launch(&journal, 7, 89).await?;
    let (_archive_root, store, lease) = seed_archive_lease(&journal, id, &identity).await?;
    let runner_name = format!("v{}", identity.launch_id());
    let engine = CompletionEngine::with_stopped_pair(&identity, &runner_id, &dind_id)?;
    let removed_volumes = engine.removed_volumes.clone();
    let api = BlockingRunnerApi::released(&runner_name, 99);

    completion::record_completion_events(&journal, 7, &completion_poll(89, 99, &runner_name))
        .await
        .map_err(|error| error.to_string())?;
    let tasks = completion::schedule_completed_isolated(
        api.clone(),
        7,
        "admin-token",
        journal.clone(),
        engine,
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(tasks.len(), 1);
    for task in tasks {
        task.await.map_err(|error| error.to_string())?;
    }

    let calls = api.calls()?;
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1].0, "DELETE");
    assert!(removed_volumes.load(Ordering::Acquire));
    assert!(
        journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    assert_eq!(journal.occupied_launches().await, Ok(0));
    assert!(
        store
            .open_existing_lease(identity.launch_id(), lease.generation_id())
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn restart_retires_archive_lease_after_durable_worker_cleanup_proof() -> Result<(), String> {
    let (scratch, journal) = open("completion-lease-retirement-restart").await?;
    let (id, identity, _runner_id, _dind_id) = launch(&journal, 7, 90).await?;
    let (archive_root, store, lease) = seed_archive_lease(&journal, id, &identity).await?;
    let runner_name = format!("v{}", identity.launch_id());
    completion::record_completion_events(&journal, 7, &completion_poll(90, 100, &runner_name))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .mark_completion_worker_cleanup_proven(id)
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .completion_worker_cleanup_proven(id)
            .await
            .map_err(|error| error.to_string())?
    );
    assert!(
        store
            .open_existing_lease(identity.launch_id(), lease.generation_id())
            .is_ok()
    );
    drop(store);
    drop(journal);

    let journal = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    assert!(
        journal
            .completion_worker_cleanup_proven(id)
            .await
            .map_err(|error| error.to_string())?
    );
    let store =
        ActionArchiveStore::open_existing(&archive_root).map_err(|error| error.to_string())?;
    assert!(
        store
            .open_existing_lease(identity.launch_id(), lease.generation_id())
            .is_ok()
    );
    let api = BlockingRunnerApi::released(&runner_name, 100);
    let tasks = completion::schedule_completed_isolated(
        api.clone(),
        7,
        "admin-token",
        journal.clone(),
        CompletionEngine::empty(),
    )
    .await
    .map_err(|error| error.to_string())?;
    assert_eq!(tasks.len(), 1);
    for task in tasks {
        task.await.map_err(|error| error.to_string())?;
    }
    assert!(
        api.calls()?.is_empty(),
        "restart must not repeat GitHub cleanup"
    );
    assert!(
        journal
            .intent(id)
            .await
            .map_err(|error| error.to_string())?
            .cleanup_proven
    );
    assert_eq!(journal.occupied_launches().await, Ok(0));
    assert!(
        store
            .open_existing_lease(identity.launch_id(), lease.generation_id())
            .is_err()
    );
    Ok(())
}

async fn seed_archive_lease(
    journal: &crate::journal::Journal,
    intent_id: i64,
    identity: &crate::journal::LaunchIdentity,
) -> Result<(PathBuf, ActionArchiveStore, ActionArchiveLease), String> {
    let archive_root = journal
        .path()
        .parent()
        .ok_or_else(|| "journal has no parent".to_owned())?
        .join("action-archives");
    let publisher = ActionArchiveStore::open(&archive_root).map_err(|error| error.to_string())?;
    let bytes = archive_bytes()?;
    let archive = ActionArchiveIdentity {
        repository_id: 101,
        name_with_owner: "ChainArgos/test-action".to_owned(),
        commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        sha256: Sha256::digest(&bytes).into(),
        size: u64::try_from(bytes.len()).map_err(|error| error.to_string())?,
    };
    publisher
        .publish(&archive, Cursor::new(&bytes))
        .map_err(|error| error.to_string())?;
    let published_lease = publisher
        .lease(
            identity.launch_id(),
            202,
            std::slice::from_ref(&archive),
            None,
        )
        .map_err(|error| error.to_string())?;
    journal
        .bind_seed_generation(intent_id, published_lease.generation_id())
        .await
        .map_err(|error| error.to_string())?;
    drop(publisher);
    let store =
        ActionArchiveStore::open_existing(&archive_root).map_err(|error| error.to_string())?;
    let lease = store
        .open_existing_lease(identity.launch_id(), published_lease.generation_id())
        .map_err(|error| error.to_string())?;
    Ok((archive_root, store, lease))
}

fn archive_bytes() -> Result<Vec<u8>, String> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = Builder::new(encoder);
    let body = b"name: test\n";
    let mut header = Header::new_gnu();
    header
        .set_path("package/action.yml")
        .map_err(|error| error.to_string())?;
    header.set_size(u64::try_from(body.len()).map_err(|error| error.to_string())?);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append(&header, body.as_slice())
        .map_err(|error| error.to_string())?;
    let encoder = archive.into_inner().map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}
