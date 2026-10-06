use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::identity::{normalized_allowlist, object_generation, validate_identity};
use super::{
    ActionArchiveIdentity, ActionArchiveLease, ActionArchiveSeedError, ActionArchiveStore,
};

const MAX_ACTIONS_PER_MANIFEST: usize = 512;
const MAX_TOTAL_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;
const LOCK_SHARDS: usize = 64;

static OBJECT_LOCKS: OnceLock<[Mutex<()>; LOCK_SHARDS]> = OnceLock::new();

/// A streaming source for an exact, already-authorized action archive identity.
///
/// The implementation must use a fixed HTTPS endpoint policy, apply connection and body
/// deadlines, and stream no more than `maximum_bytes`. It must not run action code. The host
/// store checks the exact byte count, digest, archive format, and paths before publication.
pub(crate) trait ActionArchiveFetcher {
    /// Open the archive for the exact immutable identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the source cannot provide this identity within its bounds.
    fn open_archive(
        &mut self,
        identity: &ActionArchiveIdentity,
        maximum_bytes: u64,
    ) -> Result<Box<dyn Read + Send>, ActionArchiveSeedError>;
}

/// Exact action identities authorized for one consumer repository.
///
/// This type validates identity shape and payload budgets. It does not establish who
/// authorized the manifest. The caller must build it from a trusted workflow source and an
/// explicit repository/action allowlist. This repository has no production manifest provider
/// yet, so callers must not treat this constructor as an authorization check.
#[derive(Debug, Clone)]
pub(crate) struct ActionArchiveManifest {
    consumer_repository_id: u64,
    archives: Vec<ActionArchiveIdentity>,
}

impl ActionArchiveManifest {
    /// Validate an exact allowlist supplied by a trusted caller.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid repository, ref, digest, duplicate archive, or budget.
    pub(crate) fn new(
        consumer_repository_id: u64,
        archives: Vec<ActionArchiveIdentity>,
    ) -> Result<Self, ActionArchiveSeedError> {
        if consumer_repository_id == 0 || archives.len() > MAX_ACTIONS_PER_MANIFEST {
            return Err(ActionArchiveSeedError::InvalidIdentity);
        }
        for archive in &archives {
            validate_identity(archive)?;
        }
        let archives = normalized_allowlist(&archives)?;
        let total_size = archives.iter().try_fold(0_u64, |total, archive| {
            total
                .checked_add(archive.size)
                .filter(|size| *size <= MAX_TOTAL_ARCHIVE_BYTES)
                .ok_or(ActionArchiveSeedError::SizeLimit)
        })?;
        if total_size > MAX_TOTAL_ARCHIVE_BYTES {
            return Err(ActionArchiveSeedError::SizeLimit);
        }
        Ok(Self {
            consumer_repository_id,
            archives,
        })
    }

    /// Stable consumer repository ID bound into the lease generation.
    #[must_use]
    pub(crate) fn consumer_repository_id(&self) -> u64 {
        self.consumer_repository_id
    }

    /// Exact normalized action allowlist bound into the lease generation.
    #[must_use]
    pub(crate) fn archives(&self) -> &[ActionArchiveIdentity] {
        &self.archives
    }
}

impl ActionArchiveStore {
    /// Publish missing verified archives and create a durable projection for one launch.
    ///
    /// This method holds only a per-object in-process lock while it checks or fetches each
    /// archive. It does not hold a scheduler or launch lock. Persist the returned generation
    /// ID before worker creation. Keep the lease until worker and owned-resource cleanup is
    /// confirmed. The fetcher must be bounded and must use only the trusted manifest identity.
    ///
    /// # Errors
    ///
    /// Returns an error on a missing or invalid source, archive mismatch, store fault, or lease
    /// conflict. It never falls back to an unpinned ref.
    pub(crate) fn prepare_and_lease(
        &self,
        launch_id: &str,
        manifest: &ActionArchiveManifest,
        expected_generation_id: Option<&str>,
        fetcher: &mut impl ActionArchiveFetcher,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        self.prepare_and_lease_with_parent_sync(
            launch_id,
            manifest,
            expected_generation_id,
            fetcher,
            super::sync_directory,
        )
    }

    #[cfg(test)]
    pub(crate) fn prepare_and_lease_with_sync_fault(
        &self,
        launch_id: &str,
        manifest: &ActionArchiveManifest,
        expected_generation_id: Option<&str>,
        fetcher: &mut impl ActionArchiveFetcher,
        fail_on_sync: usize,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        let mut calls = 0_usize;
        self.prepare_and_lease_with_parent_sync(
            launch_id,
            manifest,
            expected_generation_id,
            fetcher,
            |directory| {
                calls += 1;
                if calls == fail_on_sync {
                    Err(ActionArchiveSeedError::Io)
                } else {
                    super::sync_directory(directory)
                }
            },
        )
    }

    fn prepare_and_lease_with_parent_sync(
        &self,
        launch_id: &str,
        manifest: &ActionArchiveManifest,
        expected_generation_id: Option<&str>,
        fetcher: &mut impl ActionArchiveFetcher,
        mut sync_objects_parent: impl FnMut(&Path) -> Result<(), ActionArchiveSeedError>,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        for identity in &manifest.archives {
            let generation = object_generation(identity)?;
            let _guard = object_lock(&generation)?;
            match self.object_path(identity) {
                Ok(_) => {
                    sync_objects_parent(&self.objects)?;
                    continue;
                }
                Err(ActionArchiveSeedError::MissingArchive) => {}
                Err(error) => return Err(error),
            }
            let maximum_bytes = identity
                .size
                .checked_add(1)
                .ok_or(ActionArchiveSeedError::SizeLimit)?;
            let source = fetcher.open_archive(identity, maximum_bytes)?;
            self.publish(identity, source.take(maximum_bytes))?;
        }
        self.lease(
            launch_id,
            manifest.consumer_repository_id,
            &manifest.archives,
            expected_generation_id,
        )
    }
}

fn object_lock(generation: &str) -> Result<MutexGuard<'static, ()>, ActionArchiveSeedError> {
    let prefix = generation
        .get(..2)
        .ok_or(ActionArchiveSeedError::InvalidIdentity)?;
    let bucket =
        u8::from_str_radix(prefix, 16).map_err(|_| ActionArchiveSeedError::InvalidIdentity)?;
    let index = usize::from(bucket) % LOCK_SHARDS;
    OBJECT_LOCKS.get_or_init(|| std::array::from_fn(|_| Mutex::new(())))[index]
        .lock()
        .map_err(|_| ActionArchiveSeedError::Io)
}
