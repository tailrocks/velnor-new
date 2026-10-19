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
    database: turso::Database,
    process_state: Arc<crate::journal_identity::JournalProcessState>,
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
        let canonical = fs::canonicalize(&path).map_err(|_| HostError::Path)?;
        if canonical != path {
            return Err(HostError::Path);
        }
        let process_state = crate::journal_identity::process_state(&canonical)?;
        let file = Self {
            path: canonical,
            identity,
            anchor,
            database,
            process_state,
        };
        file.verify()?;
        Ok(file)
    }

    pub(super) async fn connection(&self) -> Result<turso::Connection, HostError> {
        self.verify()?;
        let connection = self.database.connect().map_err(|_| HostError::Journal)?;
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

impl Drop for JournalFile {
    fn drop(&mut self) {
        crate::journal_identity::release_process_state(&self.path, &self.process_state);
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

#[cfg(test)]
mod tests {
    use super::JournalFile;
    use crate::error::HostError;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "velnor-journal-file-identity-{}-{serial}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("scratch directory");
            Self(path)
        }

        fn file(&self) -> PathBuf {
            self.0.join("journal.db")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _removed = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn cloned_file_opens_fresh_connections_and_cold_reopens_populated_schema() {
        let scratch = Scratch::new();
        let path = scratch.file();
        drop(
            crate::journal::Journal::open(&path)
                .await
                .expect("create journal schema"),
        );

        let file = JournalFile::open(&path)
            .await
            .expect("open populated journal");
        let clone = file.clone();
        let first = file.connection().await.expect("first connection");
        first
            .execute("CREATE TABLE identity_test (value INTEGER)", ())
            .await
            .expect("create test table");
        first
            .execute("BEGIN IMMEDIATE", ())
            .await
            .expect("begin transaction");
        first
            .execute("INSERT INTO identity_test VALUES (1)", ())
            .await
            .expect("write inside transaction");
        let second = clone.connection().await.expect("fresh cloned connection");
        let mut rows = second
            .query("SELECT count(*) FROM identity_test", ())
            .await
            .expect("query from separate connection");
        let row = rows.next().await.expect("read row").expect("count row");
        assert_eq!(row.get::<i64>(0).expect("count value"), 0);
        drop(rows);
        drop(second);
        first
            .execute("ROLLBACK", ())
            .await
            .expect("rollback transaction");
        drop(first);

        let second = clone.connection().await.expect("connection after rollback");
        let mut rows = second
            .query("SELECT count(*) FROM identity_test", ())
            .await
            .expect("query after rollback");
        let row = rows.next().await.expect("read row").expect("count row");
        assert_eq!(row.get::<i64>(0).expect("count value"), 0);
        drop(rows);
        drop(second);
        drop(clone);
        drop(file);

        let reopened = JournalFile::open(&path)
            .await
            .expect("cold reopen parses populated schema at default stack size");
        let connection = reopened.connection().await.expect("reopened connection");
        drop(connection);
        drop(reopened);
    }

    #[tokio::test]
    async fn changed_file_identity_rejects_connections() {
        let scratch = Scratch::new();
        let path = scratch.file();
        let file = JournalFile::open(&path).await.expect("open journal file");
        let moved = scratch.0.join("old-journal.db");
        std::fs::rename(&path, &moved).expect("move original journal");
        std::fs::write(&path, b"replacement").expect("write replacement file");

        assert!(matches!(file.connection().await, Err(HostError::Path)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinked_journal_path_is_rejected() {
        let scratch = Scratch::new();
        let target = scratch.0.join("target.db");
        std::fs::write(&target, b"journal").expect("write target file");
        let link = scratch.file();
        std::os::unix::fs::symlink(target, &link).expect("create journal symlink");

        assert!(matches!(
            JournalFile::open(&link).await,
            Err(HostError::Path)
        ));
    }
}
