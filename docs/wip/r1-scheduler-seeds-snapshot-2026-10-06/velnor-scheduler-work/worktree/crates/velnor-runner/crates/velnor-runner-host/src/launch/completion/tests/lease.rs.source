use super::*;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use tar::{Builder, Header};

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
async fn restart_finishes_archive_retirement_after_worker_cleanup_proof() -> Result<(), String> {
    let (scratch, journal) = open("completion-lease-retirement-restart").await?;
    let LaunchReservation::New(id) = journal
        .reserve_assignment(7, 90, 190, 8)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("expected a new seeded launch".to_owned());
    };
    let identity = journal
        .launch_identity(id)
        .await
        .map_err(|error| error.to_string())?;
    let archive_root = journal
        .path()
        .parent()
        .ok_or_else(|| "journal has no parent".to_owned())?
        .join("action-archives");
    let store = crate::action_archive_seed::ActionArchiveStore::open(&archive_root)
        .map_err(|error| error.to_string())?;
    let bytes = archive_bytes()?;
    let archive = crate::action_archive_seed::ActionArchiveIdentity {
        repository_id: 101,
        name_with_owner: "ChainArgos/test-action".to_owned(),
        commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        sha256: Sha256::digest(&bytes).into(),
        size: u64::try_from(bytes.len()).map_err(|error| error.to_string())?,
    };
    store
        .publish(&archive, Cursor::new(&bytes))
        .map_err(|error| error.to_string())?;
    let lease = store
        .lease(
            identity.launch_id(),
            202,
            std::slice::from_ref(&archive),
            None,
        )
        .map_err(|error| error.to_string())?;
    journal
        .bind_seed_generation(id, lease.generation_id())
        .await
        .map_err(|error| error.to_string())?;
    if !journal
        .claim_acquire(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the acquire claim".to_owned());
    }
    journal
        .resolve_acquire(id, true)
        .await
        .map_err(|error| error.to_string())?;
    if !journal
        .claim_jit(id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("expected the JIT claim".to_owned());
    }
    let runner_name = format!("v{}", identity.launch_id());
    journal
        .bind_github_runner(id, "100")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .bind_pair(id, &format!("{:064x}", 90), &format!("{:064x}", 91))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .finish(id, crate::Outcome::Done)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_runner_completed(7, 90, 100, &runner_name)
        .await
        .map_err(|error| error.to_string())?;

    // Simulate a crash after exact GitHub and Docker cleanup, then after archive retirement.
    journal
        .mark_completion_worker_cleanup_proven(id)
        .await
        .map_err(|error| error.to_string())?;
    store
        .release_after_confirmed_cleanup(lease.launch_id(), lease.generation_id())
        .map_err(|error| error.to_string())?;
    drop(store);
    drop(journal);

    let journal = crate::Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
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
    Ok(())
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
