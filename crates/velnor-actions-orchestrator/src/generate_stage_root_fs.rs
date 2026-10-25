use std::fs::{self, Metadata, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::OrchestratorError;

use super::mount::{MountIdentity, require_mount};
use super::{
    FsIdentity, OWNER_FILE, OwnerId, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE, RootIdentity, SPARE_DIR,
    StageRoot,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn create_container(
    root: &RootIdentity,
    container: &Path,
) -> Result<(), OrchestratorError> {
    fs::create_dir(container).map_err(|error| io(container, &error))?;
    set_mode(container, PRIVATE_DIR_MODE)?;
    write_private_file(&container.join(".gitignore"), b"*\n")?;
    write_private_file(&container.join(OWNER_FILE), &root.owner_bytes())?;
    let spare = container.join(SPARE_DIR);
    fs::create_dir(&spare).map_err(|error| io(&spare, &error))?;
    set_mode(&spare, PRIVATE_DIR_MODE)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), OrchestratorError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| io(path, &error))?;
    file.write_all(bytes).map_err(|error| io(path, &error))?;
    set_mode(path, PRIVATE_FILE_MODE)
}

impl StageRoot {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn validate(
        root: RootIdentity,
        container: PathBuf,
    ) -> Result<Self, OrchestratorError> {
        let owner = crate::filesystem_owner::effective_uid();
        let (container_identity, container_metadata) = FsIdentity::capture(&container)?;
        if !same_device(root.object, container_identity) {
            return Err(unsafe_path(&container, "cross_filesystem_staging"));
        }
        require_mount(&container, &root.mount)?;
        require_mode(&container, &container_metadata, PRIVATE_DIR_MODE)?;
        require_owner_id(&container, &container_metadata, owner)?;
        validate_container_files(&container, &root, owner)?;
        let spare = container.join(SPARE_DIR);
        let (spare_identity, spare_metadata) = FsIdentity::capture(&spare)?;
        if !same_device(container_identity, spare_identity) {
            return Err(unsafe_path(&spare, "cross_filesystem_staging"));
        }
        require_mount(&spare, &root.mount)?;
        require_owner_id(&spare, &spare_metadata, owner)?;
        let stage = Self {
            root,
            container,
            container_identity,
            spare,
            spare_identity,
            owner,
        };
        stage.validate_container_and_spare(true)?;
        Ok(stage)
    }

    pub(super) fn validate_container_and_spare(
        &self,
        require_spare_identity: bool,
    ) -> Result<(), OrchestratorError> {
        self.validate_container()?;
        let (actual, metadata) = FsIdentity::capture(&self.spare)?;
        if require_spare_identity && actual != self.spare_identity {
            return Err(unsafe_path(&self.spare, "staging_root_identity_changed"));
        }
        if !same_device(self.container_identity, actual) {
            return Err(unsafe_path(&self.spare, "cross_filesystem_staging"));
        }
        require_mount(&self.spare, &self.root.mount)?;
        self.require_owner(&self.spare, &metadata)
    }

    pub(super) fn validate_container(&self) -> Result<(), OrchestratorError> {
        let (actual, metadata) = FsIdentity::capture(&self.container)?;
        if actual != self.container_identity {
            return Err(unsafe_path(
                &self.container,
                "staging_container_identity_changed",
            ));
        }
        if !same_device(self.root.object, actual) {
            return Err(unsafe_path(&self.container, "cross_filesystem_staging"));
        }
        require_mount(&self.container, &self.root.mount)?;
        require_mode(&self.container, &metadata, PRIVATE_DIR_MODE)?;
        self.require_owner(&self.container, &metadata)?;
        validate_container_files(&self.container, &self.root, self.owner)
    }

    pub(super) fn require_identity(
        path: &Path,
        expected: FsIdentity,
    ) -> Result<(), OrchestratorError> {
        let (actual, _) = FsIdentity::capture(path)?;
        if actual != expected {
            return Err(unsafe_path(path, "staging_root_identity_changed"));
        }
        Ok(())
    }

    pub(super) fn require_same_filesystem(&self, path: &Path) -> Result<(), OrchestratorError> {
        let (actual, _) = FsIdentity::capture(path)?;
        if !same_device(self.root.object, actual) {
            return Err(unsafe_path(path, "cross_filesystem_staging"));
        }
        Ok(())
    }

    pub(super) fn require_same_mount(&self, path: &Path) -> Result<(), OrchestratorError> {
        require_mount(path, &self.root.mount)
    }

    pub(super) fn require_owner(
        &self,
        path: &Path,
        metadata: &Metadata,
    ) -> Result<(), OrchestratorError> {
        require_owner_id(path, metadata, self.owner)
    }

    pub(super) fn clear_children(
        path: &Path,
        expected: FsIdentity,
    ) -> Result<(), OrchestratorError> {
        Self::require_identity(path, expected)?;
        super::cleanup::clear_children(path)?;
        Self::require_identity(path, expected)
    }
}

pub(super) fn validate_container_files(
    container: &Path,
    root: &RootIdentity,
    owner: OwnerId,
) -> Result<(), OrchestratorError> {
    let ignore = container.join(".gitignore");
    validate_private_file(&ignore, b"*\n", owner, "staging_ignore_mismatch")?;
    validate_private_file(
        &container.join(OWNER_FILE),
        &root.owner_bytes(),
        owner,
        "owner_record_mismatch",
    )?;
    let spare = container.join(SPARE_DIR);
    let (_, metadata) = FsIdentity::capture(&spare)?;
    require_owner_id(&spare, &metadata, owner)
}

pub(super) fn validate_directory_owners(
    root: &Path,
    expected_owner: OwnerId,
    expected_mount: &MountIdentity,
) -> Result<(), OrchestratorError> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|error| io(&path, &error))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        if metadata_owner(&metadata) != expected_owner {
            return Err(unsafe_path(&path, "foreign_directory_owner"));
        }
        require_mount(&path, expected_mount)?;
        pending.extend(
            fs::read_dir(&path)
                .map_err(|error| io(&path, &error))?
                .map(|entry| {
                    entry
                        .map(|entry| entry.path())
                        .map_err(|error| io(&path, &error))
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    Ok(())
}

fn validate_private_file(
    path: &Path,
    expected: &[u8],
    owner: OwnerId,
    mismatch_reason: &str,
) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io(path, &error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(unsafe_path(path, "staging_metadata_not_regular_file"));
    }
    require_mode(path, &metadata, PRIVATE_FILE_MODE)?;
    require_owner_id(path, &metadata, owner)?;
    let actual = fs::read(path).map_err(|error| io(path, &error))?;
    if actual != expected {
        return Err(unsafe_path(path, mismatch_reason));
    }
    Ok(())
}

pub(super) fn same_device(first: FsIdentity, second: FsIdentity) -> bool {
    #[cfg(unix)]
    {
        first.device == second.device
    }
    #[cfg(not(unix))]
    {
        let _ = (first, second);
        true
    }
}

pub(super) fn require_mode(
    path: &Path,
    metadata: &Metadata,
    expected: u32,
) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o7777 != expected {
            return Err(unsafe_path(path, "staging_mode_mismatch"));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, metadata, expected);
    }
    Ok(())
}

pub(super) fn require_owner_id(
    path: &Path,
    metadata: &Metadata,
    expected: OwnerId,
) -> Result<(), OrchestratorError> {
    if metadata_owner(metadata) != expected {
        return Err(unsafe_path(path, "staging_owner_mismatch"));
    }
    Ok(())
}

pub(super) fn metadata_owner(metadata: &Metadata) -> OwnerId {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.uid()
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        ()
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<(), OrchestratorError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| io(path, &error))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn io(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
