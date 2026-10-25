use crate::{OrchestratorError, internal::internal};
use std::{fs, io::Read as _, os::unix::fs::MetadataExt as _, path::Path};

pub(in crate::package_release_publish) fn read(path: &Path) -> Result<Vec<u8>, OrchestratorError> {
    read_after_open(path, || {})
}

fn identity(metadata: &fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

fn read_after_open(path: &Path, after_open: impl FnOnce()) -> Result<Vec<u8>, OrchestratorError> {
    const LIMIT: u64 = 1024 * 1024;
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("release_metadata_open"))?;
    let mut file = fs::File::from(descriptor);
    let before = file
        .metadata()
        .map_err(|_| internal("release_metadata_stat"))?;
    if !before.is_file() || before.len() == 0 || before.len() > LIMIT {
        return Err(internal("release_metadata_bound"));
    }
    after_open();
    let mut bytes = Vec::new();
    file.by_ref()
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| internal("release_metadata_read"))?;
    let held_after = file
        .metadata()
        .map_err(|_| internal("release_metadata_stat"))?;
    let path_after = fs::symlink_metadata(path).map_err(|_| internal("release_metadata_stat"))?;
    if bytes.len() as u64 != before.len()
        || !path_after.is_file()
        || identity(&before) != identity(&held_after)
        || identity(&before) != identity(&path_after)
    {
        return Err(internal("release_metadata_changed"));
    }
    Ok(bytes)
}
