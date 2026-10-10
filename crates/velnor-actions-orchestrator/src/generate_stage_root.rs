//! Private per-worktree staging owner for in-place `.github` generation.

use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};

use crate::OrchestratorError;

#[path = "generate_stage_cleanup.rs"]
mod cleanup;

#[path = "generate_stage_root_fs.rs"]
mod fs_ops;

use fs_ops::create_container;
use fs_ops::validate_directory_owners;

#[cfg(test)]
#[path = "generate_stage_root_tests.rs"]
mod tests;

const CONTAINER: &str = ".github.velnor-stage";
#[cfg(test)]
const IGNORE_FILE: &str = ".gitignore";
const OWNER_FILE: &str = "owner";
const SPARE_DIR: &str = "spare";
const OWNER_VERSION: &str = "velnor-generate-stage-v1";
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;

/// Identity of one filesystem object, captured without following its leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FsIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl FsIdentity {
    fn capture(path: &Path) -> Result<(Self, Metadata), OrchestratorError> {
        let metadata = fs::symlink_metadata(path).map_err(|error| io(path, &error))?;
        if metadata.file_type().is_symlink() {
            return Err(unsafe_path(path, "symlink_refused"));
        }
        if !metadata.is_dir() {
            return Err(unsafe_path(path, "not_a_directory"));
        }
        Ok((Self::from_metadata(&metadata), metadata))
    }

    fn from_metadata(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = metadata;
            Self {}
        }
    }
}

/// Canonical worktree identity persisted in the private owner record.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RootIdentity {
    path: PathBuf,
    path_text: String,
    object: FsIdentity,
}

impl RootIdentity {
    fn capture(root: &Path) -> Result<Self, OrchestratorError> {
        let path = root.canonicalize().map_err(|error| io(root, &error))?;
        let (object, metadata) = FsIdentity::capture(&path)?;
        if !metadata.is_dir() {
            return Err(unsafe_path(&path, "repository_root_not_directory"));
        }
        let path_text = path
            .to_str()
            .ok_or_else(|| unsafe_path(&path, "repository_root_not_utf8"))?
            .to_owned();
        Ok(Self {
            path,
            path_text,
            object,
        })
    }

    fn owner_bytes(&self) -> Vec<u8> {
        #[cfg(unix)]
        let identity = format!("{}\n{}", self.object.device, self.object.inode);
        #[cfg(not(unix))]
        let identity = "filesystem-identity-unavailable".to_owned();
        format!("{OWNER_VERSION}\n{}\n{identity}\n", self.path_text).into_bytes()
    }
}

/// Persistent sibling slot; the spare path survives every publish and cleanup.
#[derive(Debug)]
pub(super) struct StageRoot {
    root: RootIdentity,
    container: PathBuf,
    container_identity: FsIdentity,
    spare: PathBuf,
    spare_identity: FsIdentity,
    owner: Option<u32>,
}

impl StageRoot {
    /// Open an owned staging container or create it once at this worktree root.
    pub(super) fn open(root: &Path) -> Result<Self, OrchestratorError> {
        let identity = RootIdentity::capture(root)?;
        let container = identity.path.join(CONTAINER);
        match fs::symlink_metadata(&container) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_container(&identity, &container)?;
            }
            Err(error) => return Err(io(&container, &error)),
        }
        Self::validate(identity, container)
    }

    /// Path of the persistent spare root, supplied to the in-place writer.
    pub(super) fn spare(&self) -> &Path {
        &self.spare
    }

    /// Validate the current `.github` root and bind its inode before staging.
    pub(super) fn validate_target(
        &self,
        target: &Path,
    ) -> Result<Option<FsIdentity>, OrchestratorError> {
        self.validate_container_and_spare(false)?;
        let metadata = match fs::symlink_metadata(target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io(target, &error)),
        };
        if metadata.file_type().is_symlink() {
            return Err(unsafe_path(target, "symlink_refused"));
        }
        if !metadata.is_dir() {
            return Err(unsafe_path(target, "not_a_directory"));
        }
        self.require_same_filesystem(target)?;
        self.require_owner(target, &metadata)?;
        Ok(Some(FsIdentity::from_metadata(&metadata)))
    }

    /// Fail before publication unless every real output directory is ours.
    pub(super) fn validate_target_directories(
        &self,
        target: &Path,
        expected: Option<FsIdentity>,
    ) -> Result<(), OrchestratorError> {
        let Some(expected) = expected else {
            return Ok(());
        };
        self.require_identity(target, expected)?;
        validate_directory_owners(target, self.owner)?;
        self.require_identity(target, expected)
    }

    /// Remove stale or retired children while retaining the spare root inode.
    pub(super) fn clear_spare_before_staging(&self) -> Result<(), OrchestratorError> {
        self.validate_container_and_spare(false)?;
        self.clear_children(&self.spare, self.spare_identity)
    }

    /// Remove children from the retired root only after a successful exchange.
    pub(super) fn clean_retired_root(&self, expected: FsIdentity) -> Result<(), OrchestratorError> {
        self.validate_container()?;
        let published = self.root.path.join(".github");
        self.require_identity(&published, self.spare_identity)
            .map_err(|_| unsafe_path(&published, "published_root_identity_changed"))?;
        let (actual, metadata) = FsIdentity::capture(&self.spare)?;
        if actual != expected {
            return Err(unsafe_path(&self.spare, "retired_root_identity_changed"));
        }
        self.require_same_filesystem(&self.spare)?;
        self.require_owner(&self.spare, &metadata)?;
        self.clear_children(&self.spare, expected)
    }

    /// Check the staged tree and both roots again immediately before commit.
    pub(super) fn validate_staged(
        &self,
        target: &Path,
        original_target: Option<FsIdentity>,
        staged: &Path,
    ) -> Result<(), OrchestratorError> {
        self.validate_container()?;
        self.require_identity(&self.container, self.container_identity)?;
        self.require_identity(&self.spare, self.spare_identity)?;
        self.require_same_filesystem(&self.spare)?;
        let (staged_identity, staged_metadata) = FsIdentity::capture(staged)?;
        self.require_same_filesystem(staged)?;
        self.require_owner(staged, &staged_metadata)?;
        let first_stage = self.spare.join(".github");
        let wanted = if original_target.is_some() {
            self.spare.as_path()
        } else {
            first_stage.as_path()
        };
        if staged != wanted {
            return Err(unsafe_path(staged, "unexpected_staging_path"));
        }
        if original_target.is_some() {
            self.require_identity(&self.spare, staged_identity)?;
        }
        let current_target = self.validate_target(target)?;
        if current_target != original_target {
            return Err(unsafe_path(target, "target_identity_changed_before_commit"));
        }
        Ok(())
    }

    /// Remove only staged children after a failed precommit operation.
    pub(super) fn abort_before_commit(&self, cause: OrchestratorError) -> OrchestratorError {
        match self.clear_spare_before_staging() {
            Ok(()) => cause,
            Err(cleanup) => OrchestratorError::io(
                self.spare.display().to_string(),
                format!("generation_aborted:{cause}; staging_cleanup_failed:{cleanup}"),
            ),
        }
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
