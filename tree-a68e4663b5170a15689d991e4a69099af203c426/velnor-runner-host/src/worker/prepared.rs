//! Durable prepared-DinD handles and split worker operations.

use crate::error::HostError;
use crate::launch_identity::LaunchIdentity;

/// One verified, identity-owned `DinD` that passed the inner API and storage checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedDind {
    identity: LaunchIdentity,
    dind_id: String,
}

impl PreparedDind {
    /// Rebuild the handle from the ID persisted by the journal.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Ownership`] when the ID is not a Docker container ID.
    pub(crate) fn from_journal(
        identity: &LaunchIdentity,
        dind_id: &str,
    ) -> Result<Self, HostError> {
        if !container_id(dind_id) {
            return Err(HostError::Ownership);
        }
        Ok(Self {
            identity: identity.clone(),
            dind_id: dind_id.to_owned(),
        })
    }

    /// `DinD` container ID for durable journal binding.
    #[must_use]
    pub(crate) fn dind_id(&self) -> &str {
        &self.dind_id
    }

    pub(crate) fn identity(&self) -> &LaunchIdentity {
        &self.identity
    }
}

fn container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}
