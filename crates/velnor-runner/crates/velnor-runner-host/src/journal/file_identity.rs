//! Canonical journal file identity checks.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::HostError;

#[derive(Debug, Clone)]
pub(super) struct JournalFile {
    path: PathBuf,
    identity: FileIdentity,
    anchor: Arc<File>,
}

impl PartialEq for JournalFile {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.identity == other.identity
    }
}

impl Eq for JournalFile {}

impl JournalFile {
    pub(super) async fn open(path: &Path) -> Result<Self, HostError> {
        let path = canonical_candidate(path)?;
        let (anchor, identity) = FileIdentity::open(&path)?;
        let text = path.to_str().ok_or(HostError::Path)?;
        let database = turso::Builder::new_local(text)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        let connection = database.connect().map_err(|_| HostError::Journal)?;
        drop(connection);
        drop(database);
        let canonical = fs::canonicalize(&path).map_err(|_| HostError::Path)?;
        if canonical != path {
            return Err(HostError::Path);
        }
        let file = Self {
            path: canonical,
            identity,
            anchor,
        };
        file.verify()?;
        Ok(file)
    }

    pub(super) async fn connection(&self) -> Result<turso::Connection, HostError> {
        self.verify()?;
        let text = self.path.to_str().ok_or(HostError::Path)?;
        let database = turso::Builder::new_local(text)
            .build()
            .await
            .map_err(|_| HostError::Journal)?;
        let connection = database.connect().map_err(|_| HostError::Journal)?;
        self.verify()?;
        Ok(connection)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    fn verify(&self) -> Result<(), HostError> {
        if fs::canonicalize(&self.path).map_err(|_| HostError::Path)? != self.path
            || FileIdentity::read(&self.path)? != self.identity
            || FileIdentity::from_file(&self.anchor)? != self.identity
        {
            return Err(HostError::Path);
        }
        Ok(())
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(unix)]
impl FileIdentity {
    fn open(path: &Path) -> Result<(Arc<File>, Self), HostError> {
        let file = open_database_file(path).map_err(|_| HostError::Path)?;
        let identity = Self::from_file(&file)?;
        if Self::read(path)? != identity {
            return Err(HostError::Path);
        }
        Ok((Arc::new(file), identity))
    }

    fn from_file(file: &File) -> Result<Self, HostError> {
        use std::os::unix::fs::MetadataExt;

        let metadata = file.metadata().map_err(|_| HostError::Path)?;
        if !metadata.file_type().is_file() {
            return Err(HostError::Path);
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn read(path: &Path) -> Result<Self, HostError> {
        use std::os::unix::fs::MetadataExt;

        let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
        if !metadata.file_type().is_file() {
            return Err(HostError::Path);
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

#[cfg(not(unix))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileIdentity {
    length: u64,
    modified: std::time::SystemTime,
}

#[cfg(not(unix))]
impl FileIdentity {
    fn open(path: &Path) -> Result<(Arc<File>, Self), HostError> {
        let file = open_database_file(path).map_err(|_| HostError::Path)?;
        let identity = Self::from_file(&file)?;
        if Self::read(path)? != identity {
            return Err(HostError::Path);
        }
        Ok((Arc::new(file), identity))
    }

    fn from_file(file: &File) -> Result<Self, HostError> {
        let metadata = file.metadata().map_err(|_| HostError::Path)?;
        if !metadata.file_type().is_file() {
            return Err(HostError::Path);
        }
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified().map_err(|_| HostError::Path)?,
        })
    }

    fn read(path: &Path) -> Result<Self, HostError> {
        let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
        if !metadata.file_type().is_file() {
            return Err(HostError::Path);
        }
        Ok(Self {
            length: metadata.len(),
            modified: metadata.modified().map_err(|_| HostError::Path)?,
        })
    }
}

#[cfg(unix)]
fn open_database_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    match options.create_new(true).mode(0o600).open(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path),
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn open_database_file(path: &Path) -> std::io::Result<File> {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            OpenOptions::new().read(true).write(true).open(path)
        }
        Err(error) => Err(error),
    }
}

fn canonical_candidate(path: &Path) -> Result<PathBuf, HostError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or(HostError::Path)?;
    let candidate = fs::canonicalize(parent)
        .map_err(|_| HostError::Path)?
        .join(name);
    match fs::symlink_metadata(&candidate) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(candidate),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(candidate),
        _ => Err(HostError::Path),
    }
}
