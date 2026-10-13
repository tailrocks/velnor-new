use std::path::Path;

use super::super::identity::{validate_component, validate_generation_id};
use super::super::storage::sync_directory;
use super::super::{ActionArchiveLease, ActionArchiveSeedError, ActionArchiveStore};
use super::{RUNNER_CACHE_DIR, has_retired_lease, validate_live_lease};

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
        self.open_existing_lease_with_parent_sync(launch_id, expected_generation_id, sync_directory)
    }

    #[cfg(test)]
    pub(crate) fn open_existing_lease_with_sync_fault(
        &self,
        launch_id: &str,
        expected_generation_id: &str,
        fail_on_sync: usize,
    ) -> Result<ActionArchiveLease, ActionArchiveSeedError> {
        let mut calls = 0_usize;
        self.open_existing_lease_with_parent_sync(launch_id, expected_generation_id, |directory| {
            calls += 1;
            if calls == fail_on_sync {
                Err(ActionArchiveSeedError::Io)
            } else {
                sync_directory(directory)
            }
        })
    }

    fn open_existing_lease_with_parent_sync(
        &self,
        launch_id: &str,
        expected_generation_id: &str,
        mut sync_parent: impl FnMut(&Path) -> Result<(), ActionArchiveSeedError>,
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
        sync_parent(&self.objects)?;
        sync_parent(&self.leases)?;
        Ok(ActionArchiveLease {
            launch_id: launch_id.to_owned(),
            generation_id: expected_generation_id.to_owned(),
            cache_path: active.join(RUNNER_CACHE_DIR),
        })
    }
}
