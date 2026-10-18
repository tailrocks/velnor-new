use std::path::Path;

use super::plist::render_plist;
use super::quarantine::{
    QuarantineOutcome, existing_matches, open_directory, parent_and_name, path_matches_file,
    quarantine_remove,
};
use super::staging::PrivateDirectory;

pub(super) fn write_plist(binary: &Path, state: &Path, path: &Path, logs: &Path) -> bool {
    if std::fs::create_dir_all(state).is_err() {
        return false;
    }
    let Some(body) = render_plist(binary, state, logs) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err()
        || std::fs::create_dir_all(logs).is_err()
        || !regular_file_or_absent(path)
    {
        return false;
    }
    atomic_write(path, body.as_bytes()).is_ok()
}

pub(super) fn regular_file_or_absent(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

pub(super) fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if existing_matches(path, contents)? {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let staging = PrivateDirectory::create(parent, "velnor-plist-stage")?;
    let file = staging.create_payload(contents)?;
    let published = publish_staged(&staging, &file, path, contents);
    let cleaned = staging.remove_payload_if_owned(&file, contents);
    drop(file);
    cleaned?;
    staging.cleanup()?;
    published?;
    std::fs::File::open(parent)?.sync_all()
}

pub(super) fn publish_staged(
    staging: &PrivateDirectory,
    payload: &std::fs::File,
    path: &Path,
    contents: &[u8],
) -> std::io::Result<()> {
    if !staging.path_matches_directory()? || !staging.payload_matches(payload, contents)? {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
    }
    let (parent, name) = parent_and_name(path)?;
    let parent_directory = open_directory(parent)?;
    match rustix::fs::linkat(
        &staging.directory,
        "payload",
        &parent_directory,
        name,
        rustix::fs::AtFlags::empty(),
    ) {
        Ok(()) => {
            let published_matches = path_matches_file(path, payload, contents)?;
            let staging_matches =
                staging.path_matches_directory()? && staging.payload_matches(payload, contents)?;
            if published_matches && staging_matches {
                Ok(())
            } else {
                match quarantine_remove(path, payload, contents)? {
                    QuarantineOutcome::Removed
                    | QuarantineOutcome::Missing
                    | QuarantineOutcome::Restored
                    | QuarantineOutcome::Recovery(_) => {}
                }
                Err(std::io::Error::from(std::io::ErrorKind::InvalidData))
            }
        }
        Err(error) if error == rustix::io::Errno::EXIST => {
            if existing_matches(path, contents)? {
                Ok(())
            } else {
                Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
            }
        }
        Err(error) => Err(error.into()),
    }
}
