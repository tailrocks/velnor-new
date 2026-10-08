//! Opaque session operations minted only from a matching pool proof.

use std::fmt;
use std::time::SystemTime;

use crate::policy::{PoolBinding, PoolRegistrationScope, VerifiedPoolPolicy};

use super::{ActionsServiceScaleSetRoute, OrganizationDiscoveryAdmin};

mod acknowledge;
mod acquire;
mod assigned_demand;
mod close;
mod operations;
mod origin;
mod types;

pub use types::{
    AcquireUnresolvedReason, PopulationObservationSource, SessionCloseOutcome,
    SessionPopulationObservation, VerifiedAcquireOutcome, VerifiedAcquiredJob,
    VerifiedAssignedDemand, VerifiedQueueSession,
};

/// Why a proof could not be promoted into session capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PoolSessionCapabilityError {
    /// The proof has expired or is not yet valid.
    #[error("pool proof is stale")]
    StaleProof,
    /// The credential, service route, or proof binding differs.
    #[error("pool binding mismatch")]
    BindingMismatch,
}

/// Admin capability bound to one verified pool and its same-scope bootstrap.
/// The credential is private and never exposed by this API.
#[must_use]
pub struct VerifiedPoolSessionAdmin {
    pub(super) connection: crate::AdminConnection,
    pub(super) binding: PoolBinding,
    pub(super) policy_digest: String,
    session_creation_attempted: bool,
    created_session_id: Option<String>,
    close_attempted: bool,
    expires_at: SystemTime,
}

impl VerifiedPoolSessionAdmin {
    /// Consume an organization-scoped GET-only admin after the source-specific
    /// route and pool proof have both been checked against the same binding.
    ///
    /// # Errors
    ///
    /// Returns `BindingMismatch` if the retained admin, route, and proof do not
    /// name the same organization, repository, group, and Scale Set; returns
    /// `StaleProof` when the proof has expired.
    pub(crate) fn from_organization_route(
        admin: OrganizationDiscoveryAdmin,
        route: &ActionsServiceScaleSetRoute,
        proof: &VerifiedPoolPolicy,
    ) -> Result<Self, PoolSessionCapabilityError> {
        let binding = proof.binding();
        let (owner, _) = split_repository(&binding.repository_full_name)?;
        if !matches!(
            &binding.registration_scope,
            PoolRegistrationScope::Organization { organization }
                if organization.eq_ignore_ascii_case(admin.organization())
        ) || !admin
            .target_repository_full_name()
            .eq_ignore_ascii_case(&binding.repository_full_name)
            || admin.target_repository_id() != binding.repository_id
            || !route
                .organization()
                .eq_ignore_ascii_case(admin.organization())
            || route.runner_group_id() != binding.actions_runner_group_id
            || route.runner_group_name() != binding.actions_runner_group_name
            || route.scale_set().id != binding.scale_set_id
            || route.scale_set().name != binding.scale_set_name
            || !owner.eq_ignore_ascii_case(admin.organization())
        {
            return Err(PoolSessionCapabilityError::BindingMismatch);
        }
        Self::from_connection(admin.connection, binding, proof)
    }

    fn from_connection(
        connection: crate::AdminConnection,
        binding: &PoolBinding,
        proof: &VerifiedPoolPolicy,
    ) -> Result<Self, PoolSessionCapabilityError> {
        if proof.expires_at() <= SystemTime::now() {
            return Err(PoolSessionCapabilityError::StaleProof);
        }
        if binding.repository_id <= 0
            || binding.scale_set_id <= 0
            || binding.actions_runner_group_id <= 0
            || binding.scale_set_name.is_empty()
            || binding.policy_digest.is_empty()
            || proof.policy_digest() != binding.policy_digest
        {
            return Err(PoolSessionCapabilityError::BindingMismatch);
        }
        Ok(Self {
            connection,
            binding: binding.clone(),
            policy_digest: proof.policy_digest().to_owned(),
            session_creation_attempted: false,
            created_session_id: None,
            close_attempted: false,
            expires_at: proof.expires_at(),
        })
    }

    /// Validated Actions Service URL for the host's strict origin binding.
    /// Never persist or log this URL.
    #[must_use]
    pub fn service_url(&self) -> &str {
        self.connection.expose_url()
    }

    /// Exact pool binding this capability may operate.
    #[must_use]
    pub const fn binding(&self) -> &PoolBinding {
        &self.binding
    }

    /// Digest of the exact trust policy bound to the pool proof.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    /// Expiration of the pool proof. Side-effect methods reject an expired proof.
    #[must_use]
    pub const fn expires_at(&self) -> SystemTime {
        self.expires_at
    }

    pub(super) fn require_fresh(&self) -> Result<(), crate::SessionError> {
        if self.expires_at <= SystemTime::now()
            || self.close_attempted
            || (self.session_creation_attempted && self.created_session_id.is_none())
        {
            Err(crate::SessionError::Wire(
                crate::WireError::RegistrationRejected,
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn require_session(
        &self,
        session: &types::VerifiedQueueSession,
    ) -> Result<(), crate::SessionError> {
        if self.close_attempted
            || self.created_session_id.as_deref() != Some(session.inner.session_id.as_str())
            || session.one_shot.close_attempted
            || session.one_shot.assigned_jit_uncertain
            || session.scale_set_id != self.binding.scale_set_id
            || session.policy_digest != self.policy_digest
            || session.inner.session_id.is_empty()
        {
            Err(crate::SessionError::Wire(
                crate::WireError::RegistrationRejected,
            ))
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for VerifiedPoolSessionAdmin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedPoolSessionAdmin")
            .field("binding", &self.binding)
            .field("policy_digest", &self.policy_digest)
            .field(
                "session_creation_attempted",
                &self.session_creation_attempted,
            )
            .field("created_session_id", &self.created_session_id)
            .field("close_attempted", &self.close_attempted)
            .field("expires_at", &self.expires_at)
            .field("admin", &"[redacted]")
            .finish_non_exhaustive()
    }
}

fn split_repository(full_name: &str) -> Result<(&str, &str), PoolSessionCapabilityError> {
    let (owner, repository) = full_name
        .split_once('/')
        .filter(|(owner, repository)| {
            !owner.is_empty() && !repository.is_empty() && !repository.contains('/')
        })
        .ok_or(PoolSessionCapabilityError::BindingMismatch)?;
    Ok((owner, repository))
}

fn valid_queue_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 4096
        && !path.starts_with("//")
        && !path
            .chars()
            .any(|character| matches!(character, '?' | '#' | '\\') || character.is_control())
}

#[cfg(test)]
mod tests;
