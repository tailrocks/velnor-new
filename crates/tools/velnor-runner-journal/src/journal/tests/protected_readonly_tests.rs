//! Read-only protected opens never create, migrate, or follow replacements.

use std::num::NonZeroU32;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::journal::{ReplayRoute, ScopedLaunchIdentity};
use crate::{CapacityClaim, HostError, Journal};

use super::Scratch;

fn protected_directory(root: &Path) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("state");
    std::fs::create_dir(&path).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    Ok(path)
}

#[tokio::test]
async fn missing_database_is_not_created_by_readonly_protected_open() -> Result<(), String> {
    let scratch = Scratch::new("readonly-protected-missing").map_err(|error| error.to_string())?;
    let directory = protected_directory(&scratch.path)?;
    let path = directory.join("launch.db");
    let parent = std::fs::metadata(&directory).map_err(|error| error.to_string())?;
    assert!(matches!(
        Journal::open_readonly_protected_at(&path, parent.dev(), parent.ino()).await,
        Err(HostError::Path)
    ));
    assert!(matches!(
        Journal::open_existing_protected_at(&path, parent.dev(), parent.ino()).await,
        Err(HostError::Path)
    ));
    assert!(!path.exists());
    assert!(!directory.join("launch.db-wal").exists());
    assert!(!directory.join("launch.db-shm").exists());
    Ok(())
}

#[tokio::test]
async fn existing_readonly_open_checks_parent_and_leaf_identity_on_each_read() -> Result<(), String>
{
    let scratch = Scratch::new("readonly-protected-replaced").map_err(|error| error.to_string())?;
    let directory = protected_directory(&scratch.path)?;
    let path = directory.join("launch.db");
    let writable = Journal::open_protected(&path)
        .await
        .map_err(|error| error.to_string())?;
    writable
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    let parent = std::fs::metadata(&directory).map_err(|error| error.to_string())?;
    let existing = Journal::open_existing_protected_at(&path, parent.dev(), parent.ino())
        .await
        .map_err(|error| error.to_string())?;
    let readonly = Journal::open_readonly_protected_at(&path, parent.dev(), parent.ino())
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(readonly.draining().await, Ok(true));
    assert_eq!(
        readonly.drain_snapshot().await,
        Ok(crate::journal::DrainSnapshot {
            draining: true,
            occupied_launches: 0,
            unresolved_intents: 0,
        })
    );
    assert!(readonly.request_drain().await.is_err());

    let replacement = directory.join("replacement.db");
    std::fs::write(&replacement, b"different inode").map_err(|error| error.to_string())?;
    std::fs::rename(&replacement, &path).map_err(|error| error.to_string())?;
    assert!(matches!(readonly.draining().await, Err(HostError::Path)));
    assert!(matches!(
        existing.request_drain().await,
        Err(HostError::Path)
    ));
    assert_eq!(
        std::fs::read(&path).map_err(|error| error.to_string())?,
        b"different inode"
    );
    Ok(())
}

#[tokio::test]
async fn aggregate_drain_snapshot_tracks_durable_launch_occupancy() -> Result<(), String> {
    let scratch = Scratch::new("readonly-protected-counts").map_err(|error| error.to_string())?;
    let directory = protected_directory(&scratch.path)?;
    let path = directory.join("launch.db");
    let journal = Journal::open_protected(&path)
        .await
        .map_err(|error| error.to_string())?;
    let identity = ScopedLaunchIdentity::new(
        ReplayRoute {
            destination: "https://api.github.com",
            registration_scope: "repository",
            owner: "acme",
            repository: "widget",
            runner_group_id: 2,
            runner_group_name: "trusted",
            scale_set_id: 3,
            scale_set_name: "linux",
        },
        "session-1",
        5,
        9,
    )
    .map_err(|error| error.to_string())?;
    assert!(matches!(
        journal
            .reserve_launch_if_accepting(&identity, NonZeroU32::new(1).ok_or("capacity")?)
            .await
            .map_err(|error| error.to_string())?,
        CapacityClaim::New(_)
    ));
    let parent = std::fs::metadata(&directory).map_err(|error| error.to_string())?;
    let readonly = Journal::open_readonly_protected_at(&path, parent.dev(), parent.ino())
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        readonly.drain_snapshot().await,
        Ok(crate::journal::DrainSnapshot {
            draining: false,
            occupied_launches: 1,
            unresolved_intents: 0,
        })
    );
    Ok(())
}
