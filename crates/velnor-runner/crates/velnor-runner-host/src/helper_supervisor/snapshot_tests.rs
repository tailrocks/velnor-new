use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::super::{path, run_helper, snapshot};
use super::TestScript;

const ACCEPTED: &str = "#!/bin/sh\nrequest=$(cat) || exit 1\n[ \"$request\" = '{}' ] || exit 1\nprintf '%s' '{\"schema\":2,\"result\":\"verified\"}'\n";
const REJECTED: &str = "#!/bin/sh\nprintf '%s' '{\"schema\":2,\"result\":\"replaced\"}'\n";

#[tokio::test]
async fn executes_a_snapshot_copied_from_the_open_verified_file_after_path_replacement()
-> Result<(), HostError> {
    let script = TestScript::new(ACCEPTED)?;
    let source = path::open_verified_from(&script.executable, &script.digest)?;
    let replacement = script.directory.join("replacement");
    write_executable(&replacement, REJECTED)?;
    fs::rename(&replacement, script.path()).map_err(|_| HostError::Path)?;
    let snapshot = snapshot::prepare(&script.directory, source, &script.digest).await?;
    assert_eq!(run_helper(snapshot, b"{}".to_vec()).await, Ok(()));
    Ok(())
}

#[tokio::test]
async fn rejects_source_inode_mutation_while_copying_into_snapshot() -> Result<(), HostError> {
    let script = TestScript::new(ACCEPTED)?;
    let source = path::open_verified_from(&script.executable, &script.digest)?;
    fs::write(script.path(), REJECTED).map_err(|_| HostError::Path)?;
    assert_eq!(
        snapshot::prepare(&script.directory, source, &script.digest)
            .await
            .err(),
        Some(HostError::Identity)
    );
    Ok(())
}

#[tokio::test]
async fn preserves_a_snapshot_held_by_another_supervisor_and_prunes_it_after_reap()
-> Result<(), HostError> {
    let script = TestScript::new(ACCEPTED)?;
    let active = script.snapshot().await?;
    let old_path = active.path().to_path_buf();
    replace_source(&script, REJECTED)?;
    let current_digest: [u8; 32] = Sha256::digest(REJECTED.as_bytes()).into();
    let source = path::open_verified_from(&script.executable, &current_digest)?;
    let current = snapshot::prepare(&script.directory, source, &current_digest).await?;
    assert!(old_path.exists());
    drop(active);
    let source = path::open_verified_from(&script.executable, &current_digest)?;
    let next = snapshot::prepare(&script.directory, source, &current_digest).await?;
    assert!(!old_path.exists());
    assert_eq!(current.path(), next.path());
    Ok(())
}

#[tokio::test]
async fn concurrent_preparations_share_one_verified_snapshot() -> Result<(), HostError> {
    let script = Arc::new(TestScript::new(ACCEPTED)?);
    let mut tasks = Vec::new();
    for _ in 0..6 {
        let script = Arc::clone(&script);
        tasks.push(tokio::spawn(async move { script.snapshot().await }));
    }
    let mut leases = Vec::new();
    for task in tasks {
        leases.push(task.await.map_err(|_| HostError::Identity)??);
    }
    let first = leases
        .first()
        .ok_or(HostError::Identity)?
        .path()
        .to_path_buf();
    assert!(leases.iter().all(|lease| lease.path() == first));
    Ok(())
}

#[tokio::test]
async fn rejects_unrecognized_store_entries_without_deleting_them() -> Result<(), HostError> {
    let script = TestScript::new(ACCEPTED)?;
    let store = script.directory.join("attestation-helper-executables");
    fs::create_dir(&store).map_err(|_| HostError::Path)?;
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Path)?;
    let unknown = store.join("unrecognized");
    fs::write(&unknown, b"keep").map_err(|_| HostError::Path)?;
    assert_eq!(script.snapshot().await.err(), Some(HostError::Path));
    assert!(unknown.exists());
    Ok(())
}

#[tokio::test]
async fn removes_interrupted_partial_copy_and_recovers() -> Result<(), HostError> {
    let script = TestScript::new(ACCEPTED)?;
    let store = script.directory.join("attestation-helper-executables");
    fs::create_dir(&store).map_err(|_| HostError::Path)?;
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Path)?;
    let partial = store.join(format!(".partial-{}", uuid::Uuid::new_v4()));
    let prepared = store.join(format!(".partial-{}", uuid::Uuid::new_v4()));
    let orphan_lock = store.join(format!("{}.lock", hex_digest(&[0x11; 32])));
    fs::write(&partial, b"interrupted").map_err(|_| HostError::Path)?;
    fs::write(&prepared, b"ready but not renamed").map_err(|_| HostError::Path)?;
    fs::write(&orphan_lock, b"").map_err(|_| HostError::Path)?;
    fs::set_permissions(&partial, fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Path)?;
    fs::set_permissions(&prepared, fs::Permissions::from_mode(0o500))
        .map_err(|_| HostError::Path)?;
    fs::set_permissions(&orphan_lock, fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Path)?;
    let lease = script.snapshot().await?;
    assert!(!partial.exists());
    assert!(!prepared.exists());
    assert!(!orphan_lock.exists());
    assert!(lease.path().exists());
    Ok(())
}

#[tokio::test]
async fn fails_closed_on_a_corrupt_published_snapshot() -> Result<(), HostError> {
    let script = TestScript::new(ACCEPTED)?;
    let lease = script.snapshot().await?;
    let path = lease.path().to_path_buf();
    drop(lease);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Path)?;
    fs::write(&path, REJECTED).map_err(|_| HostError::Path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).map_err(|_| HostError::Path)?;
    assert_eq!(script.snapshot().await.err(), Some(HostError::Identity));
    assert!(path.exists());
    Ok(())
}

fn replace_source(script: &TestScript, replacement: &str) -> Result<(), HostError> {
    write_executable(script.path(), replacement)
}

fn write_executable(path: &std::path::Path, contents: &str) -> Result<(), HostError> {
    fs::write(path, contents).map_err(|_| HostError::Path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Path)
}

fn hex_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in digest {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}
