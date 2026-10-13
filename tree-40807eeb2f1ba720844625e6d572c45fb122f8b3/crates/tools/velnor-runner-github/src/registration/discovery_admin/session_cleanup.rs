//! One-shot cleanup of a known journaled session under its repository scope.

use std::time::SystemTime;

use crate::session::{SessionError, delete_session_async, safe_path_segment};
use crate::{AsyncDiscoveryTransport, ScaleSetFound, WireError};

use super::RepositoryDiscoveryAdmin;

/// Expected repository cleanup route from the durable row and exact current
/// config. This is descriptive metadata for GET-only preflight; it cannot
/// authorize session deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct RepositorySessionCleanupBinding {
    destination: String,
    target_repository_id: i64,
    target_repository_full_name: String,
    runner_group_id: i64,
    runner_group_name: String,
    scale_set_id: i64,
    scale_set_name: String,
}

/// Borrowed fields used only to build the GET-only expected route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct RepositorySessionCleanupExpectation<'a> {
    /// Exact durable controller destination; must match the current config.
    pub destination: &'a str,
    /// Literal journal scope kind; only `repository` is accepted here.
    pub registration_scope: &'a str,
    /// Exact registration scope name.
    pub scope_name: &'a str,
    /// Immutable target repository ID.
    pub target_repository_id: i64,
    /// Exact target repository full name.
    pub target_repository_full_name: &'a str,
    /// Actions Service internal group ID.
    pub runner_group_id: i64,
    /// Exact Actions Service group name.
    pub runner_group_name: &'a str,
    /// Exact existing Scale Set ID.
    pub scale_set_id: i64,
    /// Exact existing product Scale Set name.
    pub scale_set_name: &'a str,
}

/// A durable, one-shot Journal close permit required for session DELETE.
///
/// The State Journal crate implements this interface for its private
/// non-Clone permit. Do not implement it for config, request, event, or
/// caller-provided data. `begin_delete_attempt` must be one-shot; the durable
/// row must already be in its closing state before this call.
pub trait RepositorySessionCloseClaim {
    /// Positive durable close intent ID.
    fn intent_id(&self) -> i64;
    /// Stable controller destination associated with the close intent.
    fn destination(&self) -> &str;
    /// Registration scope kind; this helper accepts only `repository`.
    fn registration_scope(&self) -> &str;
    /// Exact registration scope name.
    fn scope_name(&self) -> &str;
    /// Immutable target repository ID.
    fn target_repository_id(&self) -> i64;
    /// Exact target repository full name.
    fn target_repository_full_name(&self) -> &str;
    /// Actions Service internal runner-group ID.
    fn runner_group_id(&self) -> i64;
    /// Exact Actions Service runner-group name.
    fn runner_group_name(&self) -> &str;
    /// Exact existing Scale Set ID.
    fn scale_set_id(&self) -> i64;
    /// Exact existing product Scale Set name.
    fn scale_set_name(&self) -> &str;
    /// Exact session ID from the persisted row, exposed only through this
    /// close-claim contract.
    fn session_id_for_cleanup(&self) -> &str;
    /// Atomically consume this in-memory permit's one DELETE attempt.
    fn begin_delete_attempt(&mut self) -> bool;
}

impl RepositorySessionCleanupBinding {
    /// Build an expected identity for the GET-only route check. This is not a
    /// close permit and cannot authorize DELETE. Organization or enterprise
    /// scopes are refused by this repository-only API.
    ///
    /// # Errors
    ///
    /// Returns `RegistrationRejected` for an unsupported scope, invalid or
    /// inconsistent identity, or unsupported product Scale Set selector.
    pub fn from_expected_fields(
        fields: RepositorySessionCleanupExpectation<'_>,
    ) -> Result<Self, SessionError> {
        let RepositorySessionCleanupExpectation {
            destination,
            registration_scope,
            scope_name,
            target_repository_id,
            target_repository_full_name,
            runner_group_id,
            runner_group_name,
            scale_set_id,
            scale_set_name,
        } = fields;
        let valid_full_name = |value: &str| {
            value.split_once('/').is_some_and(|(owner, repo)| {
                !owner.is_empty()
                    && !repo.is_empty()
                    && !repo.contains('/')
                    && !value.chars().any(char::is_control)
            })
        };
        if destination.is_empty()
            || destination.len() > 256
            || destination.chars().any(char::is_control)
            || registration_scope != "repository"
            || target_repository_id <= 0
            || !valid_full_name(scope_name)
            || !valid_full_name(target_repository_full_name)
            || scope_name != target_repository_full_name
            || runner_group_id <= 0
            || !valid_name(runner_group_name)
            || scale_set_id <= 0
            || !super::super::is_supported_product_selector(scale_set_name)
        {
            return Err(WireError::RegistrationRejected.into());
        }
        Ok(Self {
            destination: destination.to_owned(),
            target_repository_id,
            target_repository_full_name: target_repository_full_name.to_owned(),
            runner_group_id,
            runner_group_name: runner_group_name.to_owned(),
            scale_set_id,
            scale_set_name: scale_set_name.to_owned(),
        })
    }

    /// Stable controller destination retained from the exact config/journal
    /// identity. State must compare it with the current config.
    #[must_use]
    pub fn destination(&self) -> &str {
        &self.destination
    }

    /// Immutable repository identity from the journal claim.
    #[must_use]
    pub const fn target_repository_id(&self) -> i64 {
        self.target_repository_id
    }

    /// Repository name retained by the journal/config binding.
    #[must_use]
    pub fn target_repository_full_name(&self) -> &str {
        &self.target_repository_full_name
    }

    /// Actions Service internal group ID, not a REST group ID.
    #[must_use]
    pub const fn runner_group_id(&self) -> i64 {
        self.runner_group_id
    }

    /// Exact Actions Service group name.
    #[must_use]
    pub fn runner_group_name(&self) -> &str {
        &self.runner_group_name
    }

    /// Exact Scale Set ID.
    #[must_use]
    pub const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    /// Exact Scale Set name.
    #[must_use]
    pub fn scale_set_name(&self) -> &str {
        &self.scale_set_name
    }
}

/// Read-only confirmation that one exact repository group and Scale Set were
/// found under the same short-lived repository-scoped admin connection.
///
/// This route is not an admission permit and contains no credential. It is
/// consumed by the one-shot session close operation.
#[derive(Debug, PartialEq, Eq)]
#[must_use]
pub struct RepositorySessionCleanupRoute {
    binding: RepositorySessionCleanupBinding,
    observed_at: SystemTime,
}

impl RepositorySessionCleanupRoute {
    fn new(binding: RepositorySessionCleanupBinding) -> Self {
        Self {
            binding,
            observed_at: SystemTime::now(),
        }
    }

    /// Exact identity read from the current repository-scoped service route.
    #[must_use]
    pub const fn binding(&self) -> &RepositorySessionCleanupBinding {
        &self.binding
    }

    /// Completion time of the group and Scale Set GETs.
    #[must_use]
    pub const fn observed_at(&self) -> SystemTime {
        self.observed_at
    }
}

/// Result of the terminal known-session cleanup operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum RepositorySessionCleanupOutcome {
    /// The exact session DELETE received HTTP 204.
    Closed,
}

impl RepositoryDiscoveryAdmin {
    /// Read and bind the exact repository-scoped group and existing product
    /// Scale Set before the caller claims a durable session close intent.
    /// This performs only bounded GETs and never creates a missing object.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe transport/service error or `RegistrationRejected`
    /// if the current admin scope or discovered route does not match the
    /// journal/config identity.
    pub async fn read_existing_session_cleanup_route_async<T>(
        &self,
        transport: &mut T,
        binding: &RepositorySessionCleanupBinding,
    ) -> Result<RepositorySessionCleanupRoute, SessionError>
    where
        T: AsyncDiscoveryTransport + ?Sized,
    {
        if !admin_matches_binding(self, binding) {
            return Err(WireError::RegistrationRejected.into());
        }

        let groups = self.list_runner_groups_async(transport).await?;
        if !groups.iter().any(|group| {
            group.id == binding.runner_group_id && group.name == binding.runner_group_name
        }) {
            return Err(WireError::RegistrationRejected.into());
        }

        let found = self
            .get_existing_product_scale_set_async(
                transport,
                binding.runner_group_id,
                &binding.scale_set_name,
            )
            .await?;
        match found {
            ScaleSetFound::Found(view)
                if view.id == binding.scale_set_id && view.name == binding.scale_set_name =>
            {
                Ok(RepositorySessionCleanupRoute::new(binding.clone()))
            }
            ScaleSetFound::NotFound | ScaleSetFound::Found(_) => {
                Err(WireError::RegistrationRejected.into())
            }
        }
    }

    /// Delete exactly the caller's known journaled session once, after the
    /// current repository/group/Scale Set route has been re-read.
    ///
    /// The admin and route are consumed, so the call cannot be retried through
    /// either capability. The caller must transition the durable session row
    /// to `closing` before invoking this method and record `closed` only on the
    /// returned 204-backed `Closed` outcome. All transport failures and every
    /// non-204 status remain unresolved; there is no session GET, DELETE
    /// refresh, 404-as-gone handling, or replacement session creation.
    ///
    /// # Errors
    ///
    /// Returns `RegistrationRejected` before transport for missing/malformed
    /// IDs or any scope/route mismatch. Returns `Uncertain` for a missing
    /// response and a secret-safe status error for every non-204 response.
    pub async fn delete_claimed_session_once_async<T>(
        mut self,
        transport: &mut T,
        route: RepositorySessionCleanupRoute,
        claim: &mut impl RepositorySessionCloseClaim,
    ) -> Result<RepositorySessionCleanupOutcome, SessionError>
    where
        T: AsyncDiscoveryTransport + ?Sized,
    {
        let binding = binding_from_claim(claim)?;
        if !safe_path_segment(claim.session_id_for_cleanup())
            || route.binding != binding
            || !admin_matches_binding(&self, &binding)
        {
            return Err(WireError::RegistrationRejected.into());
        }

        transport.bind_actions_service_origin(self.connection.expose_url())?;
        if !claim.begin_delete_attempt() {
            return Err(WireError::RegistrationRejected.into());
        }

        let result = delete_session_async(
            transport,
            binding.scale_set_id,
            claim.session_id_for_cleanup(),
            self.connection.expose_token(),
        )
        .await;
        // DELETE is one-shot even when its result is uncertain. Retire the
        // short-lived admin bearer and service URL before returning.
        self.connection.retire();
        result.map(|()| RepositorySessionCleanupOutcome::Closed)
    }
}

fn binding_from_claim(
    claim: &impl RepositorySessionCloseClaim,
) -> Result<RepositorySessionCleanupBinding, SessionError> {
    if claim.intent_id() <= 0 {
        return Err(WireError::RegistrationRejected.into());
    }
    RepositorySessionCleanupBinding::from_expected_fields(RepositorySessionCleanupExpectation {
        destination: claim.destination(),
        registration_scope: claim.registration_scope(),
        scope_name: claim.scope_name(),
        target_repository_id: claim.target_repository_id(),
        target_repository_full_name: claim.target_repository_full_name(),
        runner_group_id: claim.runner_group_id(),
        runner_group_name: claim.runner_group_name(),
        scale_set_id: claim.scale_set_id(),
        scale_set_name: claim.scale_set_name(),
    })
}

fn admin_matches_binding(
    admin: &RepositoryDiscoveryAdmin,
    binding: &RepositorySessionCleanupBinding,
) -> bool {
    admin.repository_id == binding.target_repository_id
        && admin
            .repository_full_name
            .eq_ignore_ascii_case(&binding.target_repository_full_name)
}

fn valid_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
