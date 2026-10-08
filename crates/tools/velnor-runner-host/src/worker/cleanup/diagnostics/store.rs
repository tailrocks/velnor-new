use std::path::Path;

use crate::HostError;

use super::{DiagnosticsStore, validate_protected_state_directory};

impl DiagnosticsStore {
    /// Open or create the private diagnostics child of an existing service-owned state directory.
    ///
    /// The path chain is opened component by component without following symlinks. Ancestors
    /// must be root or service-owned and non-writable by other users, except root-owned sticky
    /// directories such as `/tmp`. The supplied parent must be owned by the service UID.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Path`] for an absent, writable, foreign-owned, or unsafe path.
    pub fn under_protected_parent(parent: &Path) -> Result<Self, HostError> {
        validate_protected_state_directory(parent)?.open_diagnostics_store()
    }
}
