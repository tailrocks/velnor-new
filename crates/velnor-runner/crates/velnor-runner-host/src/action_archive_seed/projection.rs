use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::ActionArchiveSeedError;
use super::identity::{ActionArchiveIdentity, runner_relative_path};
use super::storage::{verify_bytes, verify_real_directory};

pub(super) fn verify_projection(
    cache: &Path,
    archives: &[ActionArchiveIdentity],
) -> Result<(), ActionArchiveSeedError> {
    verify_real_directory(cache)?;
    let mut expected = BTreeMap::new();
    for identity in archives {
        let (owner_repo, sha) = runner_relative_path(identity)?;
        expected.insert(PathBuf::from(owner_repo).join(sha), identity);
    }
    let mut observed = BTreeSet::new();
    verify_projection_dir(cache, cache, &expected, &mut observed)?;
    if observed.len() != expected.len() {
        return Err(ActionArchiveSeedError::StoreIntegrity);
    }
    Ok(())
}

fn verify_projection_dir(
    root: &Path,
    directory: &Path,
    expected: &BTreeMap<PathBuf, &ActionArchiveIdentity>,
    observed: &mut BTreeSet<PathBuf>,
) -> Result<(), ActionArchiveSeedError> {
    for entry in fs::read_dir(directory).map_err(|_| ActionArchiveSeedError::StoreIntegrity)? {
        let path = entry
            .map_err(|_| ActionArchiveSeedError::StoreIntegrity)?
            .path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| ActionArchiveSeedError::StoreIntegrity)?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ActionArchiveSeedError::StoreIntegrity)?
            .to_path_buf();
        if metadata.file_type().is_symlink() {
            return Err(ActionArchiveSeedError::StoreIntegrity);
        }
        if metadata.is_dir() {
            if !expected.keys().any(|file| file.starts_with(&relative)) {
                return Err(ActionArchiveSeedError::StoreIntegrity);
            }
            verify_projection_dir(root, &path, expected, observed)?;
        } else if metadata.is_file() {
            let identity = expected
                .get(&relative)
                .ok_or(ActionArchiveSeedError::StoreIntegrity)?;
            verify_bytes(&path, identity)?;
            if !observed.insert(relative) {
                return Err(ActionArchiveSeedError::StoreIntegrity);
            }
        } else {
            return Err(ActionArchiveSeedError::StoreIntegrity);
        }
    }
    Ok(())
}
