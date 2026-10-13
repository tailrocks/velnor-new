//! Identity checks for a file-backed journal under a trusted service directory.

#[cfg(unix)]
mod unix {
    use std::fs::{self, File, OpenOptions};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};

    use crate::HostError;

    const SIDECARS: [&str; 3] = ["wal", "shm", "journal"];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FileIdentity {
        device: u64,
        inode: u64,
        owner: u32,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct ProtectedJournalPath {
        parent: PathBuf,
        parent_identity: FileIdentity,
        database_identity: FileIdentity,
        owner: u32,
    }

    impl ProtectedJournalPath {
        /// Validate the exact parent directory previously retained by the host.
        ///
        /// This prevents a path replacement between daemon-lock acquisition and
        /// journal bootstrap from redirecting State to a different state tree.
        pub(crate) fn prepare_for_parent(
            path: &Path,
            expected_parent: Option<(u64, u64)>,
        ) -> Result<Self, HostError> {
            let parent = path
                .parent()
                .filter(|path| path.is_absolute())
                .ok_or(HostError::Path)?;
            let parent_identity = inspect_parent(parent)?;
            if expected_parent.is_some_and(|(device, inode)| {
                parent_identity.device != device || parent_identity.inode != inode
            }) {
                return Err(HostError::Path);
            }
            let owner = parent_identity.owner;

            // Reject unsafe sidecars before creating the database or running migrations.
            validate_sidecars(path, owner)?;
            match fs::symlink_metadata(path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    create_database(path, parent)?;
                }
                Err(_) => return Err(HostError::Path),
            }
            let database_identity = inspect_regular_file(path, owner)?;
            let protected = Self {
                parent: parent.to_path_buf(),
                parent_identity,
                database_identity,
                owner,
            };
            protected.validate(path)?;
            Ok(protected)
        }

        /// Validate an existing database beneath the exact retained parent without
        /// creating the database or any SQLite sidecar.
        pub(crate) fn inspect_existing_for_parent(
            path: &Path,
            expected_parent: (u64, u64),
        ) -> Result<Self, HostError> {
            let parent = path
                .parent()
                .filter(|path| path.is_absolute())
                .ok_or(HostError::Path)?;
            let parent_identity = inspect_parent(parent)?;
            if parent_identity.device != expected_parent.0
                || parent_identity.inode != expected_parent.1
            {
                return Err(HostError::Path);
            }
            let owner = parent_identity.owner;
            validate_sidecars(path, owner)?;
            let database_identity = inspect_regular_file(path, owner)?;
            let protected = Self {
                parent: parent.to_path_buf(),
                parent_identity,
                database_identity,
                owner,
            };
            protected.validate(path)?;
            Ok(protected)
        }

        /// Revalidate path ownership and identities immediately before each Turso open.
        pub(crate) fn validate(&self, path: &Path) -> Result<(), HostError> {
            if path.parent() != Some(self.parent.as_path())
                || inspect_parent(&self.parent)? != self.parent_identity
                || inspect_regular_file(path, self.owner)? != self.database_identity
            {
                return Err(HostError::Path);
            }
            validate_sidecars(path, self.owner)
        }
    }

    fn inspect_parent(path: &Path) -> Result<FileIdentity, HostError> {
        if fs::canonicalize(path).map_err(|_| HostError::Path)? != path {
            return Err(HostError::Path);
        }
        let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
        let mode = metadata.mode() & 0o7777;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || mode & 0o700 != 0o700
            || mode & 0o022 != 0
            || mode & 0o007 != 0
            || mode & 0o7000 != 0
        {
            return Err(HostError::Path);
        }
        Ok(identity(&metadata))
    }

    fn inspect_regular_file(path: &Path, owner: u32) -> Result<FileIdentity, HostError> {
        let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
        let mode = metadata.mode() & 0o7777;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.uid() != owner
            || mode & 0o600 != 0o600
            || mode & 0o022 != 0
            || mode & 0o7000 != 0
        {
            return Err(HostError::Path);
        }
        Ok(identity(&metadata))
    }

    fn validate_sidecars(path: &Path, owner: u32) -> Result<(), HostError> {
        for suffix in SIDECARS {
            let sidecar = sidecar_path(path, suffix)?;
            match fs::symlink_metadata(&sidecar) {
                Ok(_) => {
                    inspect_regular_file(&sidecar, owner)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(HostError::Path),
            }
        }
        Ok(())
    }

    fn sidecar_path(path: &Path, suffix: &str) -> Result<PathBuf, HostError> {
        let path = path.to_str().ok_or(HostError::Path)?;
        Ok(PathBuf::from(format!("{path}-{suffix}")))
    }

    fn create_database(path: &Path, parent: &Path) -> Result<(), HostError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|_| HostError::Path)?;
        file.sync_all().map_err(|_| HostError::Path)?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| HostError::Path)
    }

    fn identity(metadata: &fs::Metadata) -> FileIdentity {
        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            owner: metadata.uid(),
        }
    }
}

#[cfg(unix)]
pub(super) use unix::ProtectedJournalPath;

#[cfg(not(unix))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProtectedJournalPath;

#[cfg(not(unix))]
impl ProtectedJournalPath {
    pub(super) fn prepare_for_parent(
        _path: &std::path::Path,
        _expected_parent: Option<(u64, u64)>,
    ) -> Result<Self, crate::HostError> {
        Err(crate::HostError::Path)
    }

    pub(super) fn inspect_existing_for_parent(
        _path: &std::path::Path,
        _expected_parent: (u64, u64),
    ) -> Result<Self, crate::HostError> {
        Err(crate::HostError::Path)
    }

    pub(super) fn validate(&self, _path: &std::path::Path) -> Result<(), crate::HostError> {
        Err(crate::HostError::Path)
    }
}
