//! Protected journal leaf, sidecar and reopen identity checks.

use std::path::Path;

use crate::{HostError, Journal};

use super::Scratch;

fn protected_directory(root: &Path) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;

    let path = root.join("state");
    std::fs::create_dir(&path).map_err(|error| error.to_string())?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    Ok(path)
}

#[cfg(unix)]
#[tokio::test]
async fn protected_open_rejects_symlink_database_before_bootstrap() -> Result<(), String> {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new("protected-symlink").map_err(|error| error.to_string())?;
    let directory = protected_directory(&scratch.path)?;
    let target = scratch.path.join("target.db");
    let path = directory.join("launch.db");
    std::fs::write(&target, b"keep this file intact").map_err(|error| error.to_string())?;
    symlink(&target, &path).map_err(|error| error.to_string())?;

    assert!(matches!(
        Journal::open_protected(&path).await,
        Err(HostError::Path)
    ));
    assert_eq!(
        std::fs::read(&target).map_err(|error| error.to_string())?,
        b"keep this file intact"
    );
    assert!(
        std::fs::symlink_metadata(&path)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
    );
    assert!(!directory.join("launch.db-wal").exists());
    assert!(!directory.join("launch.db-shm").exists());
    Ok(())
}

#[tokio::test]
async fn protected_journal_rejects_database_inode_replacement_on_reopen() -> Result<(), String> {
    let scratch = Scratch::new("protected-replaced").map_err(|error| error.to_string())?;
    let directory = protected_directory(&scratch.path)?;
    let path = directory.join("launch.db");
    let journal = Journal::open_protected(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(journal.draining().await, Ok(false));

    let replacement = directory.join("replacement.db");
    std::fs::write(&replacement, b"different database inode").map_err(|error| error.to_string())?;
    std::fs::rename(&replacement, &path).map_err(|error| error.to_string())?;
    assert!(matches!(journal.draining().await, Err(HostError::Path)));
    assert_eq!(
        std::fs::read(&path).map_err(|error| error.to_string())?,
        b"different database inode"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn protected_open_rejects_symlink_sidecar_before_database_creation() -> Result<(), String> {
    use std::os::unix::fs::symlink;

    let scratch = Scratch::new("protected-sidecar").map_err(|error| error.to_string())?;
    let directory = protected_directory(&scratch.path)?;
    let path = directory.join("launch.db");
    let target = scratch.path.join("wal-target");
    std::fs::write(&target, b"keep this sidecar target").map_err(|error| error.to_string())?;
    symlink(&target, directory.join("launch.db-wal")).map_err(|error| error.to_string())?;

    assert!(matches!(
        Journal::open_protected(&path).await,
        Err(HostError::Path)
    ));
    assert!(!path.exists());
    assert_eq!(
        std::fs::read(&target).map_err(|error| error.to_string())?,
        b"keep this sidecar target"
    );
    Ok(())
}
