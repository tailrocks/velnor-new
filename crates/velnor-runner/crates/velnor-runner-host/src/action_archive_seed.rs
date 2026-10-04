//! Immutable action archives for the official Linux runner archive cache.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod archive_paths;
mod identity;
mod lease;
mod projection;
mod storage;
mod validation;

#[cfg(test)]
#[path = "action_archive_seed_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use lease::PublicationStage;
#[cfg(test)]
pub(crate) use validation::validate_archive_with_limit;

pub(crate) use identity::ActionArchiveIdentity;
use identity::object_generation;
use storage::{
    cleanup_dir, create_directory, read_json, set_mode, sync_directory, unique_directory,
    verify_bytes, write_json,
};
use validation::validate_archive;

const FORMAT_VERSION: u32 = 1;
const TRUST_SCOPE: &str = "github.com";
const RUNNER_ARCHIVE_LAYOUT: &str = "actions-runner-v2.337.0-linux-archive-v1";
const ARCHIVE_FILE: &str = "archive.tar.gz";

/// Durable per-launch view of an allowlisted archive generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionArchiveLease {
    launch_id: String,
    generation_id: String,
    cache_path: PathBuf,
}

impl ActionArchiveLease {
    /// Stable scheduler launch identity that owns this projection.
    #[must_use]
    pub(crate) fn launch_id(&self) -> &str {
        &self.launch_id
    }

    /// Stable identifier for the exact consumer and action set in this projection.
    #[must_use]
    pub(crate) fn generation_id(&self) -> &str {
        &self.generation_id
    }

    /// Read-only projection root for `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`.
    #[must_use]
    pub(crate) fn cache_path(&self) -> &Path {
        &self.cache_path
    }
}

/// Private host store for verified official-runner action archive entries.
#[derive(Debug, Clone)]
pub(crate) struct ActionArchiveStore {
    objects: PathBuf,
    leases: PathBuf,
}

/// Archive preparation or projection failed closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ActionArchiveSeedError {
    /// An action repository, commit, or payload bound is invalid.
    #[error("invalid action archive identity")]
    InvalidIdentity,
    /// The launch identity is not a safe single path component.
    #[error("invalid archive lease identity")]
    InvalidLease,
    /// The archive exceeds a compressed or expanded size bound.
    #[error("action archive exceeds its size bound")]
    SizeLimit,
    /// The archive bytes do not match their declared digest or size.
    #[error("action archive digest or size mismatch")]
    DigestMismatch,
    /// The compressed bytes are not a valid gzip tar archive.
    #[error("invalid action archive")]
    InvalidArchive,
    /// An entry has an unsafe path, link, or unsupported file type.
    #[error("unsafe action archive entry")]
    UnsafeEntry,
    /// The exact immutable archive generation has not been published.
    #[error("action archive is not seeded")]
    MissingArchive,
    /// A stored generation or projection failed integrity validation.
    #[error("action archive store integrity failure")]
    StoreIntegrity,
    /// A launch ID already names a different immutable projection.
    #[error("action archive lease conflicts with an existing generation")]
    LeaseConflict,
    /// A filesystem operation failed.
    #[error("action archive store filesystem operation failed")]
    Io,
    /// A bounded manifest could not be encoded or decoded.
    #[error("action archive store manifest failed")]
    Manifest,
}

#[derive(Debug, Serialize, Deserialize)]
struct ObjectManifest {
    format_version: u32,
    trust_scope: String,
    runner_archive_layout: String,
    identity: ActionArchiveIdentity,
}

impl ActionArchiveStore {
    /// Open a private store. The daemon owns this directory and its parent.
    ///
    /// # Errors
    ///
    /// Returns an error when the root cannot be created or is a symbolic link.
    pub(crate) fn open(root: impl AsRef<Path>) -> Result<Self, ActionArchiveSeedError> {
        let requested = root.as_ref();
        fs::create_dir_all(requested).map_err(|_| ActionArchiveSeedError::Io)?;
        let metadata = fs::symlink_metadata(requested).map_err(|_| ActionArchiveSeedError::Io)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ActionArchiveSeedError::StoreIntegrity);
        }
        let root = fs::canonicalize(requested).map_err(|_| ActionArchiveSeedError::Io)?;
        create_directory(&root, 0o700)?;
        let objects = root.join("objects");
        let leases = root.join("leases");
        create_directory(&objects, 0o700)?;
        create_directory(&leases, 0o700)?;
        Ok(Self { objects, leases })
    }

    /// Verify and atomically publish one immutable archive generation.
    ///
    /// The method inspects tar paths only. It never extracts or executes archive bytes.
    /// The caller must bind the byte stream to the trusted GitHub repository and commit
    /// resolution. The digest verifies stored bytes; it does not authenticate their origin.
    ///
    /// # Errors
    ///
    /// Returns an error for wrong identity or digest, unsafe archive entries, or I/O failure.
    pub(crate) fn publish<R: std::io::Read>(
        &self,
        identity: &ActionArchiveIdentity,
        source: R,
    ) -> Result<String, ActionArchiveSeedError> {
        identity::validate_identity(identity)?;
        let generation_id = object_generation(identity)?;
        let destination = self.objects.join(&generation_id);
        if destination.exists() {
            Self::verify_object(&destination, identity)?;
            return Ok(generation_id);
        }
        let staging = unique_directory(&self.objects, "object")?;
        let result = Self::publish_object(&staging, &destination, identity, source);
        if result.is_err() {
            cleanup_dir(&staging)?;
            if destination.exists() {
                Self::verify_object(&destination, identity)?;
                return Ok(generation_id);
            }
        }
        result?;
        sync_directory(&self.objects)?;
        Ok(generation_id)
    }

    fn publish_object<R: std::io::Read>(
        staging: &Path,
        destination: &Path,
        identity: &ActionArchiveIdentity,
        source: R,
    ) -> Result<(), ActionArchiveSeedError> {
        let archive = staging.join(ARCHIVE_FILE);
        storage::copy_verified(source, &archive, identity)?;
        validate_archive(&archive, identity.size)?;
        write_json(
            &staging.join("manifest.json"),
            &ObjectManifest {
                format_version: FORMAT_VERSION,
                trust_scope: TRUST_SCOPE.to_owned(),
                runner_archive_layout: RUNNER_ARCHIVE_LAYOUT.to_owned(),
                identity: identity.clone(),
            },
        )?;
        set_mode(&archive, 0o444)?;
        set_mode(staging, 0o555)?;
        sync_directory(staging)?;
        fs::rename(staging, destination).map_err(|_| ActionArchiveSeedError::Io)
    }

    fn object_path(
        &self,
        identity: &ActionArchiveIdentity,
    ) -> Result<PathBuf, ActionArchiveSeedError> {
        let path = self.objects.join(object_generation(identity)?);
        if !path.exists() {
            return Err(ActionArchiveSeedError::MissingArchive);
        }
        Self::verify_object(&path, identity)?;
        Ok(path.join(ARCHIVE_FILE))
    }

    fn verify_object(
        path: &Path,
        identity: &ActionArchiveIdentity,
    ) -> Result<(), ActionArchiveSeedError> {
        storage::verify_real_directory(path)?;
        let manifest: ObjectManifest = read_json(&path.join("manifest.json"))?;
        if manifest.format_version != FORMAT_VERSION
            || manifest.trust_scope != TRUST_SCOPE
            || manifest.runner_archive_layout != RUNNER_ARCHIVE_LAYOUT
            || manifest.identity != *identity
        {
            return Err(ActionArchiveSeedError::StoreIntegrity);
        }
        verify_bytes(&path.join(ARCHIVE_FILE), identity)?;
        validate_archive(&path.join(ARCHIVE_FILE), identity.size)
    }
}
