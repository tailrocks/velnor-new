use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::identity::{
    lease_generation, normalized_allowlist, runner_relative_path, validate_component,
    validate_generation_id,
};
use super::projection::verify_projection;
use super::storage::{
    cleanup_dir, read_json, set_mode, sync_directory, unique_directory, write_json,
};
use super::{
    ActionArchiveIdentity, ActionArchiveLease, ActionArchiveSeedError, ActionArchiveStore,
    FORMAT_VERSION, RUNNER_ARCHIVE_LAYOUT, TRUST_SCOPE,
};

#[path = "lease/retirement.rs"]
mod retirement;

const MAX_ACTIONS_PER_LEASE: usize = 512;
const RUNNER_CACHE_DIR: &str = "cache";

#[derive(Clone, Copy)]
struct LeasePublication<'a> {
    staging: &'a Path,
    destination: &'a Path,
    launch_id: &'a str,
    consumer_repository_id: u64,
    generation_id: &'a str,
    archives: &'a [ActionArchiveIdentity],
    objects: &'a [PathBuf],
}

#[derive(Debug, Serialize, Deserialize)]
struct LeaseManifest {
    format_version: u32,
    trust_scope: String,
    runner_archive_layout: String,
    launch_id: String,
    consumer_repository_id: u64,
    generation_id: String,
    archives: Vec<ActionArchiveIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublicationStage {
    AfterFirstLink,
    BeforeDirectorySync,
    BeforeRename,
}

impl ActionArchiveStore {
    /// Reopen one durable lease from its persisted launch and generation identities.
    ///
    /// This method only verifies existing state. It does not fetch, publish, or recreate a
    /// projection. The scheduler must serialize recovery with confirmed cleanup and keep the
    /// lease until every consumer has stopped.
    ///
    /// # Errors
    ///
    /// Returns an error when the active lease, manifest, archive objects, or projection is
    /// missing, retired, corrupt, or bound to another generation.
    pub(crate) fn open_existing_lease(
        &self,
        launch_id: &str,
        expected_generation_id: &str,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        validate_component(launch_id).map_err(|_| ActionArchiveSeedError::InvalidLease)?;
        validate_generation_id(expected_generation_id)?;
        if has_retired_lease(&self.leases, launch_id)? {
            return Err(ActionArchiveSeedError::LeaseConflict);
        }
        let active = self.leases.join(launch_id);
        if !active.exists() {
            return Err(ActionArchiveSeedError::MissingArchive);
        }
        let manifest = validate_live_lease(&active, launch_id, expected_generation_id)?;
        for identity in &manifest.archives {
            self.object_path(identity)?;
        }
        Ok(ActionArchiveLease {
            launch_id: launch_id.to_owned(),
            generation_id: expected_generation_id.to_owned(),
            cache_path: active.join(RUNNER_CACHE_DIR),
        })
    }

    /// Create or replay a durable, allowlisted projection for one scheduler launch.
    ///
    /// Pass the persisted full `LaunchIdentity.launch_id` without rewriting it. Pass `None`
    /// before the first launch. On replay, pass the generation ID stored in the scheduler
    /// journal. The caller retains this lease across uncertain startup and releases it only
    /// after verified cleanup.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing exact generation, a conflicting launch ID, or bad storage.
    pub(crate) fn lease(
        &self,
        launch_id: &str,
        consumer_repository_id: u64,
        allowlist: &[ActionArchiveIdentity],
        expected_generation_id: Option<&str>,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        self.lease_with_hook(
            launch_id,
            consumer_repository_id,
            allowlist,
            expected_generation_id,
            |_| Ok(()),
            sync_directory,
        )
    }

    /// Test hook for failures between hard-link, sync, and atomic rename boundaries.
    #[cfg(test)]
    pub(crate) fn lease_with_fault(
        &self,
        launch_id: &str,
        consumer_repository_id: u64,
        allowlist: &[ActionArchiveIdentity],
        failed_stage: PublicationStage,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        self.lease_with_hook(
            launch_id,
            consumer_repository_id,
            allowlist,
            None,
            |stage| {
                if stage == failed_stage {
                    Err(ActionArchiveSeedError::Io)
                } else {
                    Ok(())
                }
            },
            sync_directory,
        )
    }

    #[cfg(test)]
    pub(crate) fn lease_with_sync_fault(
        &self,
        launch_id: &str,
        consumer_repository_id: u64,
        allowlist: &[ActionArchiveIdentity],
        expected_generation_id: Option<&str>,
        fail_on_sync: usize,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        let mut calls = 0_usize;
        self.lease_with_hook(
            launch_id,
            consumer_repository_id,
            allowlist,
            expected_generation_id,
            |_| Ok(()),
            |directory| {
                calls += 1;
                if calls == fail_on_sync {
                    Err(ActionArchiveSeedError::Io)
                } else {
                    sync_directory(directory)
                }
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn lease_with_test_hooks(
        &self,
        launch_id: &str,
        consumer_repository_id: u64,
        allowlist: &[ActionArchiveIdentity],
        expected_generation_id: Option<&str>,
        hook: impl FnMut(PublicationStage) -> Result<(), ActionArchiveSeedError>,
        sync_parent: impl FnMut(&Path) -> Result<(), ActionArchiveSeedError>,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        self.lease_with_hook(
            launch_id,
            consumer_repository_id,
            allowlist,
            expected_generation_id,
            hook,
            sync_parent,
        )
    }

    fn lease_with_hook(
        &self,
        launch_id: &str,
        consumer_repository_id: u64,
        allowlist: &[ActionArchiveIdentity],
        expected_generation_id: Option<&str>,
        mut hook: impl FnMut(PublicationStage) -> Result<(), ActionArchiveSeedError>,
        mut sync_parent: impl FnMut(&Path) -> Result<(), ActionArchiveSeedError>,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        validate_component(launch_id).map_err(|_| ActionArchiveSeedError::InvalidLease)?;
        if consumer_repository_id == 0 || allowlist.len() > MAX_ACTIONS_PER_LEASE {
            return Err(ActionArchiveSeedError::InvalidIdentity);
        }
        if has_retired_lease(&self.leases, launch_id)? {
            return Err(ActionArchiveSeedError::LeaseConflict);
        }
        let archives = normalized_allowlist(allowlist)?;
        let generation_id = lease_generation(consumer_repository_id, &archives)?;
        if let Some(expected) = expected_generation_id {
            validate_generation_id(expected)?;
            if expected != generation_id {
                return Err(ActionArchiveSeedError::LeaseConflict);
            }
        }
        let destination = self.leases.join(launch_id);
        if destination.exists() {
            let lease = replay_lease(
                &destination,
                launch_id,
                consumer_repository_id,
                &generation_id,
                &archives,
            )?;
            sync_parent(&self.leases)?;
            return Ok(lease);
        }
        let objects = archives
            .iter()
            .map(|identity| self.object_path(identity))
            .collect::<Result<Vec<_>, _>>()?;
        let staging = unique_directory(&self.leases, "lease")?;
        let publication = LeasePublication {
            staging: &staging,
            destination: &destination,
            launch_id,
            consumer_repository_id,
            generation_id: &generation_id,
            archives: &archives,
            objects: &objects,
        };
        let result = publish_lease(publication, &mut hook);
        if result.is_err() {
            cleanup_dir(&staging)?;
            if destination.exists() {
                let lease = replay_lease(
                    &destination,
                    launch_id,
                    consumer_repository_id,
                    &generation_id,
                    &archives,
                )?;
                sync_parent(&self.leases)?;
                return Ok(lease);
            }
        }
        result?;
        sync_parent(&self.leases)?;
        verify_projection(&destination.join(RUNNER_CACHE_DIR), &archives)?;
        Ok(ActionArchiveLease {
            launch_id: launch_id.to_owned(),
            generation_id,
            cache_path: destination.join(RUNNER_CACHE_DIR),
        })
    }
}

fn publish_lease(
    publication: LeasePublication<'_>,
    hook: &mut impl FnMut(PublicationStage) -> Result<(), ActionArchiveSeedError>,
) -> Result<(), ActionArchiveSeedError> {
    let LeasePublication {
        staging,
        destination,
        launch_id,
        consumer_repository_id,
        generation_id,
        archives,
        objects,
    } = publication;
    let cache = staging.join(RUNNER_CACHE_DIR);
    fs::create_dir(&cache).map_err(|_| ActionArchiveSeedError::Io)?;
    let mut owner_dirs = std::collections::BTreeSet::new();
    for (index, (identity, object)) in archives.iter().zip(objects).enumerate() {
        let (owner_repo, sha) = runner_relative_path(identity)?;
        let directory = cache.join(owner_repo);
        fs::create_dir_all(&directory).map_err(|_| ActionArchiveSeedError::Io)?;
        fs::hard_link(object, directory.join(sha)).map_err(|_| ActionArchiveSeedError::Io)?;
        if index == 0 {
            hook(PublicationStage::AfterFirstLink)?;
        }
        owner_dirs.insert(directory);
    }
    for directory in owner_dirs {
        set_mode(&directory, 0o555)?;
        hook(PublicationStage::BeforeDirectorySync)?;
        sync_directory(&directory)?;
    }
    write_json(
        &staging.join("manifest.json"),
        &LeaseManifest {
            format_version: FORMAT_VERSION,
            trust_scope: TRUST_SCOPE.to_owned(),
            runner_archive_layout: RUNNER_ARCHIVE_LAYOUT.to_owned(),
            launch_id: launch_id.to_owned(),
            consumer_repository_id,
            generation_id: generation_id.to_owned(),
            archives: archives.to_vec(),
        },
    )?;
    set_mode(&cache, 0o555)?;
    set_mode(staging, 0o555)?;
    sync_directory(&cache)?;
    sync_directory(staging)?;
    hook(PublicationStage::BeforeRename)?;
    fs::rename(staging, destination).map_err(|_| ActionArchiveSeedError::Io)
}

fn replay_lease(
    path: &Path,
    launch_id: &str,
    consumer_repository_id: u64,
    generation_id: &str,
    archives: &[ActionArchiveIdentity],
) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
    let manifest = read_lease_manifest(path)?;
    if manifest.format_version != FORMAT_VERSION
        || manifest.trust_scope != TRUST_SCOPE
        || manifest.runner_archive_layout != RUNNER_ARCHIVE_LAYOUT
        || manifest.launch_id != launch_id
        || manifest.consumer_repository_id != consumer_repository_id
        || manifest.generation_id != generation_id
        || manifest.archives != archives
    {
        return Err(ActionArchiveSeedError::LeaseConflict);
    }
    verify_projection(&path.join(RUNNER_CACHE_DIR), archives)?;
    Ok(ActionArchiveLease {
        launch_id: launch_id.to_owned(),
        generation_id: generation_id.to_owned(),
        cache_path: path.join(RUNNER_CACHE_DIR),
    })
}

fn validate_live_lease(
    path: &Path,
    launch_id: &str,
    generation_id: &str,
) -> Result<LeaseManifest, ActionArchiveSeedError> {
    let manifest = read_lease_manifest(path)?;
    let expected = normalized_allowlist(&manifest.archives)?;
    if manifest.format_version != FORMAT_VERSION
        || manifest.trust_scope != TRUST_SCOPE
        || manifest.runner_archive_layout != RUNNER_ARCHIVE_LAYOUT
        || manifest.launch_id != launch_id
        || manifest.consumer_repository_id == 0
        || manifest.generation_id != generation_id
        || expected != manifest.archives
        || lease_generation(manifest.consumer_repository_id, &expected)? != generation_id
    {
        return Err(ActionArchiveSeedError::LeaseConflict);
    }
    verify_projection(&path.join(RUNNER_CACHE_DIR), &expected)?;
    Ok(manifest)
}

fn read_lease_manifest(path: &Path) -> Result<LeaseManifest, ActionArchiveSeedError> {
    super::storage::verify_real_directory(path)?;
    read_json(&path.join("manifest.json"))
}

fn retired_path(leases: &Path, launch_id: &str, generation_id: &str) -> PathBuf {
    leases.join(format!(".retired-{launch_id}-{generation_id}"))
}

fn has_retired_lease(leases: &Path, launch_id: &str) -> Result<bool, ActionArchiveSeedError> {
    let prefix = format!(".retired-{launch_id}-");
    for entry in fs::read_dir(leases).map_err(|_| ActionArchiveSeedError::Io)? {
        let name = entry.map_err(|_| ActionArchiveSeedError::Io)?.file_name();
        if name.to_string_lossy().starts_with(&prefix) {
            return Ok(true);
        }
    }
    Ok(false)
}
