use std::fs;
use std::path::Path;

use super::super::ActionArchiveSeedError;
use super::super::ActionArchiveStore;
use super::{has_retired_lease, retired_path, validate_live_lease};

impl ActionArchiveStore {
    /// Release a projection after the scheduler proves terminal runner state and full cleanup.
    ///
    /// The atomic tombstone makes interrupted removal repeatable after a daemon restart. This
    /// removes only the lease; immutable archive generations stay in the store.
    ///
    /// # Errors
    ///
    /// Returns an error when the generation conflicts or the live projection is corrupt.
    pub(crate) fn release_after_confirmed_cleanup(
        &self,
        launch_id: &str,
        generation_id: &str,
    ) -> Result<(), ActionArchiveSeedError> {
        self.release_with_parent_sync(launch_id, generation_id, sync_parent_directory)
    }

    #[cfg(test)]
    pub(crate) fn release_with_sync_fault(
        &self,
        launch_id: &str,
        generation_id: &str,
        fail_on_sync: usize,
    ) -> Result<(), ActionArchiveSeedError> {
        let mut calls = 0_usize;
        self.release_with_parent_sync(launch_id, generation_id, |directory| {
            calls += 1;
            if calls == fail_on_sync {
                Err(ActionArchiveSeedError::Io)
            } else {
                sync_parent_directory(directory)
            }
        })
    }

    fn release_with_parent_sync(
        &self,
        launch_id: &str,
        generation_id: &str,
        mut sync_parent: impl FnMut(&Path) -> Result<(), ActionArchiveSeedError>,
    ) -> Result<(), ActionArchiveSeedError> {
        super::validate_component(launch_id).map_err(|_| ActionArchiveSeedError::InvalidLease)?;
        super::validate_generation_id(generation_id)?;
        let active = self.leases.join(launch_id);
        let retired = retired_path(&self.leases, launch_id, generation_id);
        if retired.exists() {
            if active.exists() {
                return Err(ActionArchiveSeedError::LeaseConflict);
            }
            sync_parent(&self.leases)?;
            return remove_retired(&self.leases, &retired, &mut sync_parent);
        }
        if !active.exists() {
            return if has_retired_lease(&self.leases, launch_id)? {
                Err(ActionArchiveSeedError::LeaseConflict)
            } else {
                sync_parent(&self.leases)?;
                Ok(())
            };
        }
        validate_live_lease(&active, launch_id, generation_id)?;
        fs::rename(&active, &retired).map_err(|_| ActionArchiveSeedError::Io)?;
        sync_parent(&self.leases)?;
        remove_retired(&self.leases, &retired, &mut sync_parent)
    }
}

fn remove_retired(
    leases: &Path,
    retired: &Path,
    sync_parent: &mut impl FnMut(&Path) -> Result<(), ActionArchiveSeedError>,
) -> Result<(), ActionArchiveSeedError> {
    super::super::storage::cleanup_dir(retired)?;
    sync_parent(leases)
}

fn sync_parent_directory(path: &Path) -> Result<(), ActionArchiveSeedError> {
    super::super::storage::sync_directory(path)
}
