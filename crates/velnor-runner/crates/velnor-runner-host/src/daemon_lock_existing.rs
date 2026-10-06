//! Existing-lineage verification must not recreate a missing external anchor.

use std::path::Path;

use crate::error::HostError;

use super::EngineLineageGuard;

impl EngineLineageGuard {
    /// Verify a pinned lineage. A missing anchor is a rollback or state loss.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the anchor is missing or mismatched.
    pub(crate) fn verify_existing_lineage(
        &self,
        path: &Path,
        instance_id: &str,
        revision: u64,
    ) -> Result<(), HostError> {
        if super::read_anchor(&self.inner.anchor_path)?.is_none() {
            return Err(HostError::Journal);
        }
        self.verify_lineage(path, instance_id, revision)
    }
}
