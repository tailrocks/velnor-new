//! Owned Docker identities and cleanup proof.

/// Why an object was not deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipFailure {
    /// Observed immutable id does not match the recorded id.
    IdMismatch,
    /// The service or daemon returned no body.
    MissingResponse,
    /// The name matched a foreign object.
    ForeignObject,
}

/// Immutable ids recorded before mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedIds {
    /// Docker container id, not a name prefix.
    pub container_id: String,
    /// Unique volume name. Volumes have no immutable id.
    pub volume: String,
}

/// Proof that those exact objects are gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupProof {
    /// Container id that was deleted.
    pub container_id: String,
    /// Volume name that was deleted.
    pub volume: String,
}

impl CleanupProof {
    /// True only when both owned ids match and are non-empty.
    #[must_use]
    pub fn covers(&self, owned: &OwnedIds) -> bool {
        !self.container_id.is_empty()
            && self.container_id == owned.container_id
            && self.volume == owned.volume
    }
}
