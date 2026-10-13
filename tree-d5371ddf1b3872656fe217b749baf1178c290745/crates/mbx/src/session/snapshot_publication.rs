//! Cooperative no-replacement publication, using rename without hard links.
use eyre::{Result, bail};
use sha2::{Digest, Sha256};
use std::fs::{File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(in crate::session) fn owner_uid() -> Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(std::fs::metadata(tempfile::tempdir()?.path())?.uid())
}

pub(in crate::session) fn directory(path: &Path, private: bool) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    verify_directory(path, private)
}

pub(in crate::session) fn verify_directory(path: &Path, private: bool) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)?;
    let forbidden = if private { 0o077 } else { 0o022 };
    if !metadata.is_dir()
        || metadata.uid() != owner_uid()?
        || metadata.mode() & forbidden != 0
        || private && metadata.mode() & 0o7777 != 0o700
        || path.canonicalize()? != path
    {
        bail!("owned snapshot directory must be canonical, owned and protected");
    }
    Ok(())
}

pub(in crate::session) fn canonical_parent(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre::eyre!("snapshot lacks parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| eyre::eyre!("snapshot lacks filename"))?;
    let parent = parent.canonicalize()?;
    directory(&parent, false)?;
    Ok(parent.join(name))
}

pub(in crate::session) fn lock(parent: &Path) -> Result<File> {
    lock_with(parent, |_| Ok(()))
}

fn lock_with(parent: &Path, before_acquire: impl FnOnce(&File) -> Result<()>) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = parent.join(".mbx-publish.lock");
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(file) => drop(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let before = verify_lock(&path)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)?;
    if !same_file(&before, &verify_lock(&path)?)
        || !same_file(&before, &verify_lock_metadata(lock.metadata()?)?)
    {
        bail!("publication lock changed while opening");
    }
    before_acquire(&lock)?;
    // File owns the lock lifetime. Closing releases without mutating its bytes
    // or timestamps; publication callers always acquire independent handles.
    lock.lock()?;
    if !same_file(&before, &verify_lock(&path)?)
        || !same_file(&before, &verify_lock_metadata(lock.metadata()?)?)
    {
        bail!("publication lock changed while acquiring");
    }
    Ok(lock)
}

fn verify_lock(path: &Path) -> Result<Metadata> {
    let metadata = verify_lock_metadata(std::fs::symlink_metadata(path)?)?;
    if path.canonicalize()? != path {
        bail!("invalid owned snapshot publication lock");
    }
    Ok(metadata)
}

fn verify_lock_metadata(metadata: Metadata) -> Result<Metadata> {
    use std::os::unix::fs::MetadataExt;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != owner_uid()?
        || metadata.mode() & 0o7777 != 0o600
    {
        bail!("invalid owned snapshot publication lock");
    }
    Ok(metadata)
}

pub(in crate::session) fn verify(path: &Path, expected: &str, mode: u32) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = if mode & 0o111 != 0 {
        plain_executable(path)?
    } else {
        std::fs::symlink_metadata(path)?
    };
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != owner_uid()?
        || metadata.mode() & 0o7777 != mode
        || path.canonicalize()? != path
    {
        bail!("owned snapshot is not an independent immutable regular file");
    }
    let mut file = open_plain(path)?;
    if !same_file(&metadata, &file.metadata()?)
        || digest(&mut file)? != expected
        || !same_file(&metadata, &file.metadata()?)
        || !same_file(&metadata, &std::fs::symlink_metadata(path)?)
    {
        bail!("owned snapshot differs from its fixed source bytes");
    }
    Ok(())
}

pub(in crate::session) fn open_plain(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?)
}

pub(in crate::session) fn exists(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(in crate::session) fn publish(
    staged: tempfile::NamedTempFile,
    destination: &Path,
    expected: &str,
    mode: u32,
) -> Result<()> {
    let destination = canonical_parent(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| eyre::eyre!("snapshot lacks parent"))?;
    let _lock = lock(parent)?;
    if staged
        .path()
        .parent()
        .map(Path::canonicalize)
        .transpose()?
        .as_deref()
        != Some(parent)
    {
        bail!("snapshot staging file is outside its publication directory");
    }
    verify(staged.path(), expected, mode)?;
    if exists(&destination)? {
        return verify(&destination, expected, mode);
    }
    // Every owned publisher uses this lock. Hostile same-UID lock bypass is
    // outside this cooperative guarantee; no operating-system exec seal exists.
    let file = staged.persist(&destination).map_err(|error| error.error)?;
    drop(file);
    verify(&destination, expected, mode)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub(in crate::session) fn publish_bytes(destination: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    if bytes.len() > 65536 {
        bail!("snapshot script exceeds bound");
    }
    let destination = canonical_parent(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| eyre::eyre!("script lacks parent"))?;
    let mut staged = tempfile::Builder::new()
        .prefix(".mbx-script-")
        .tempfile_in(parent)?;
    staged.write_all(bytes)?;
    staged
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(mode))?;
    staged.as_file().sync_all()?;
    let expected = hex::encode(Sha256::digest(bytes));
    publish(staged, &destination, &expected, mode)
}

#[cfg(feature = "owned-cache-transport")]
pub(in crate::session) fn source_metadata(path: &Path) -> Result<Metadata> {
    let metadata = plain_executable(path)?;
    use std::os::unix::fs::MetadataExt;
    if metadata.mode() & 0o022 != 0 {
        bail!("snapshot source permits uncontrolled writes");
    }
    Ok(metadata)
}

pub(in crate::session) fn digest(file: &mut File) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        hasher.update(&bytes[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn plain_executable(path: &Path) -> Result<std::fs::Metadata> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        bail!("shim executable must be a plain regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            bail!("shim source is not executable");
        }
    }
    Ok(metadata)
}

pub(in crate::session) fn same_file(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.mode() == after.mode()
            && before.len() == after.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        before.len() == after.len() && before.modified().ok() == after.modified().ok()
    }
}

#[cfg(test)]
#[path = "snapshot_publication_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "snapshot_lock_lifecycle_tests.rs"]
mod lifecycle_tests;
