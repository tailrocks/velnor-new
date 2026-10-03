use eyre::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Bytes actually observed from one regular executable, never an immutability claim.
#[derive(Debug, Serialize)]
pub(crate) struct ExecutableObservation {
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
    pub(crate) file_identity: Option<mbx_cache_core::FileIdentity>,
}

pub(crate) fn executable(path: &Path) -> Result<ExecutableObservation> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        bail!("admission executable path is not a plain file");
    }
    let path = path.canonicalize()?;
    let before = std::fs::symlink_metadata(&path)?;
    if !before.is_file() {
        bail!("admission executable is not a plain file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if before.permissions().mode() & 0o111 == 0 {
            bail!("admission file is not executable");
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(&path)?;
    let opened = file.metadata()?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    let current = std::fs::symlink_metadata(&path)?;
    if !same(&before, &opened) || !same(&opened, &after) || !same(&after, &current) {
        bail!("admission executable changed during observation");
    }
    Ok(ExecutableObservation {
        file_identity: mbx_cache_core::FileIdentity::describe(&path, &opened),
        path,
        sha256: hex::encode(hash.finalize()),
    })
}

fn same(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.mode() == right.mode()
            && left.size() == right.size()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        left.len() == right.len() && left.modified().ok() == right.modified().ok()
    }
}

pub(super) fn plain_bytes(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let before = std::fs::symlink_metadata(path)?;
    if !before.is_file() || before.len() > limit {
        bail!("invalid bounded admission sidecar");
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(path)?;
    let opened = file.metadata()?;
    if !same(&before, &opened) {
        bail!("admission sidecar replaced before read");
    }
    let mut bytes = Vec::new();
    (&mut file).take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit
        || !same(&opened, &file.metadata()?)
        || !same(&opened, &std::fs::symlink_metadata(path)?)
    {
        bail!("admission sidecar changed during read");
    }
    Ok(bytes)
}
