//! Private, digest-bound executable snapshots for the fixed verifier helper.
//!
//! Same-UID hostile writes to this service-owned directory are outside the model. If the
//! controller crashes, an orphaned helper that has not loaded its image may fail to start when a
//! successor prunes the released snapshot lock; it cannot be redirected to a different digest.

use std::fs::{self, File};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use crate::error::HostError;

use super::path::{MAX_HELPER_BYTES, VerifiedHelper};

mod image;
mod store;

const EXECUTABLE_DIRECTORY: &str = "attestation-helper-executables";

/// A verified helper path protected from the store's stale-entry sweeper.
#[derive(Debug)]
pub(super) struct SnapshotLease {
    path: PathBuf,
    image: File,
    _lock: File,
}

impl SnapshotLease {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn validate_path(&self) -> Result<(), HostError> {
        validate_open_file(&self.path, &self.image, owner(), 0o500, MAX_HELPER_BYTES).map(|_| ())
    }
}

pub(super) async fn prepare(
    state_directory: &Path,
    mut source: VerifiedHelper,
    expected_sha256: &[u8; 32],
) -> Result<SnapshotLease, HostError> {
    let owner = owner();
    validate_state_directory(state_directory, owner)?;
    let directory = ensure_executable_directory(state_directory, owner)?;
    let maintenance = store::acquire_maintenance_lock(&directory, owner).await?;
    let inventory = store::collect_inventory(&directory, owner)?;
    store::validate_inventory(&inventory, owner)?;
    store::prune_stale(&inventory, expected_sha256, owner)?;
    let inventory = store::collect_inventory(&directory, owner)?;
    store::validate_inventory(&inventory, owner)?;
    let digest = image::digest_name(expected_sha256);
    let lease = if inventory.snapshots.contains_key(&digest) {
        image::open_lease(&directory, &digest, expected_sha256, owner)?
    } else {
        store::ensure_room(&inventory, source.length)?;
        image::create_snapshot(&directory, &digest, &mut source, expected_sha256, owner)?;
        image::open_lease(&directory, &digest, expected_sha256, owner)?
    };
    drop(maintenance);
    Ok(lease)
}

fn validate_state_directory(path: &Path, owner: u32) -> Result<(), HostError> {
    if !path.is_absolute() || fs::canonicalize(path).map_err(|_| HostError::Path)? != path {
        return Err(HostError::Path);
    }
    let components = path.components().collect::<Vec<_>>();
    let mut current = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        match component {
            Component::RootDir => current.push("/"),
            Component::Normal(name) => current.push(name),
            _ => return Err(HostError::Path),
        }
        let metadata = fs::symlink_metadata(&current).map_err(|_| HostError::Path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(HostError::Path);
        }
        let final_component = index + 1 == components.len();
        if final_component {
            if metadata.uid() != owner || metadata.mode() & 0o077 != 0 {
                return Err(HostError::Path);
            }
        } else if (metadata.uid() != owner && metadata.uid() != 0)
            || (metadata.mode() & 0o022 != 0
                && !(metadata.uid() == 0 && metadata.mode() & 0o1000 != 0))
        {
            return Err(HostError::Path);
        }
    }
    Ok(())
}

fn ensure_executable_directory(state_directory: &Path, owner: u32) -> Result<PathBuf, HostError> {
    let directory = state_directory.join(EXECUTABLE_DIRECTORY);
    match fs::symlink_metadata(&directory) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            builder.create(&directory).map_err(|_| HostError::Path)?;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|_| HostError::Path)?;
        }
        Err(_) => return Err(HostError::Path),
    }
    let metadata = fs::symlink_metadata(&directory).map_err(|_| HostError::Path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != owner
        || metadata.mode() & 0o7777 != 0o700
        || fs::canonicalize(&directory).map_err(|_| HostError::Path)? != directory
    {
        return Err(HostError::Path);
    }
    Ok(directory)
}

pub(super) fn validate_path_file(
    path: &Path,
    owner: u32,
    mode: u32,
    max_size: u64,
) -> Result<fs::Metadata, HostError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    validate_metadata(path, &metadata, owner, mode, max_size)?;
    Ok(metadata)
}

pub(super) fn validate_open_file(
    path: &Path,
    file: &File,
    owner: u32,
    mode: u32,
    max_size: u64,
) -> Result<fs::Metadata, HostError> {
    let metadata = file.metadata().map_err(|_| HostError::Path)?;
    validate_metadata(path, &metadata, owner, mode, max_size)?;
    let linked = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if linked.file_type().is_symlink()
        || !linked.is_file()
        || linked.dev() != metadata.dev()
        || linked.ino() != metadata.ino()
        || linked.nlink() != metadata.nlink()
        || linked.len() != metadata.len()
        || linked.mode() & 0o7777 != mode
    {
        return Err(HostError::Path);
    }
    Ok(metadata)
}

fn validate_metadata(
    path: &Path,
    metadata: &fs::Metadata,
    owner: u32,
    mode: u32,
    max_size: u64,
) -> Result<(), HostError> {
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != owner
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != mode
        || metadata.len() > max_size
        || !same_path(path, metadata)
    {
        return Err(HostError::Path);
    }
    Ok(())
}

fn same_path(path: &Path, opened: &fs::Metadata) -> bool {
    fs::symlink_metadata(path).is_ok_and(|linked| {
        !linked.file_type().is_symlink()
            && linked.is_file()
            && linked.dev() == opened.dev()
            && linked.ino() == opened.ino()
            && linked.nlink() == opened.nlink()
    })
}

fn owner() -> u32 {
    rustix::process::geteuid().as_raw()
}
