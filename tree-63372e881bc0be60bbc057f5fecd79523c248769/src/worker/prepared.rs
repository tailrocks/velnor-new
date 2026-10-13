//! Durable prepared-DinD handles and split worker operations.

use bollard::Docker;

use crate::action_archive_seed::ActionArchiveLease;
use crate::error::HostError;
use crate::journal::LaunchIdentity;

use super::Started;

/// One verified, identity-owned DinD that passed the inner API and storage checks.
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

    /// DinD container ID for durable journal binding.
    #[must_use]
    pub(crate) fn dind_id(&self) -> &str {
        &self.dind_id
    }

    pub(crate) fn identity(&self) -> &LaunchIdentity {
        &self.identity
    }
}

/// Create DinD, then wait for its API and private VFS root before requesting JIT.
///
/// # Errors
///
/// Returns [`HostError::PreparationFailedClean`] only when preparation failed before
/// JIT and exact local cleanup confirmed the runner, DinD, and volumes absent. All
/// ambiguous Docker operations remain uncertain.
pub(crate) async fn prepare_dind_until(
    docker: &Docker,
    identity: &LaunchIdentity,
) -> Result<PreparedDind, HostError> {
    crate::stage::prepare_dind_until(docker, identity).await
}

/// Start the runner on one prepared DinD and deliver its JIT payload.
///
/// # Errors
///
/// Returns an uncertainty error when runner creation, start, or JIT delivery may
/// have completed. The caller must retain the durable launch reservation.
pub(crate) async fn start_runner_until(
    docker: &Docker,
    prepared: &PreparedDind,
    jit: &[u8],
    archive_lease: Option<&ActionArchiveLease>,
) -> Result<Started, HostError> {
    crate::stage::start_runner_until(docker, prepared, jit, archive_lease).await
}

/// Remove a prepared DinD only when no runner exists for the durable launch.
///
/// # Errors
///
/// Returns [`HostError::Cleanup`] or an ownership error when exact local absence
/// cannot be confirmed. The caller must retain the reservation on any error.
pub(crate) async fn cleanup_prepared_dind(
    docker: &Docker,
    prepared: &PreparedDind,
) -> Result<(), HostError> {
    crate::stage::cleanup_prepared_dind(docker, prepared).await
}

fn container_id(id: &str) -> bool {
    (12..=64).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}
