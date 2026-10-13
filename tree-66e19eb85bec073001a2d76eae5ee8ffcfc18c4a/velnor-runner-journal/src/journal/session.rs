//! Durable singleton Scale Set session intent, separate from worker capacity.

use crate::error::HostError;
use velnor_runner_github::RepositorySessionCloseClaim;

use super::capacity::{ReplayRoute, append_component, validate_text};

mod operations;

pub use operations::{ScaleSetSessionClaim, ScaleSetSessionCloseClaim};

const KIND: &str = "scale-set-session";
const PREFIX: &str = "scale-session-v1:";

/// Stable route identity audited on a controller session reservation.
#[derive(Clone, PartialEq, Eq)]
pub struct ScaleSetSessionIdentity {
    subject: String,
    destination: String,
    registration_scope: String,
    scope_name: String,
    target_repository_id: i64,
    target_repository_full_name: String,
    runner_group_id: i64,
    runner_group_name: String,
    scale_set_id: i64,
    scale_set_name: String,
}

impl std::fmt::Debug for ScaleSetSessionIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScaleSetSessionIdentity")
            .field("subject", &"[redacted]")
            .finish()
    }
}

impl ScaleSetSessionIdentity {
    /// Validate one route using the immutable target repository and numeric pool IDs.
    ///
    /// The target full name is checked against the registration scope and stored
    /// separately so a restart cleanup claim can verify the exact repository.
    /// Mutable names remain audit metadata; the singleton session fence is
    /// controller-wide and prevents a second live route from being created.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for malformed route or identifiers.
    pub fn new(
        route: ReplayRoute<'_>,
        target_repository_id: i64,
        target_repository_full_name: &str,
    ) -> Result<Self, HostError> {
        validate_route(route, target_repository_id, target_repository_full_name)?;
        let target_repository = target_repository_id.to_string();
        let group_id = route.runner_group_id.to_string();
        let set_id = route.scale_set_id.to_string();
        let stable = [
            route.destination,
            route.registration_scope,
            target_repository.as_str(),
            group_id.as_str(),
            set_id.as_str(),
        ];
        let mut subject = PREFIX.to_owned();
        for part in stable {
            append_component(&mut subject, part);
        }
        for audit in [
            route.owner,
            route.repository,
            route.runner_group_name,
            route.scale_set_name,
        ] {
            append_component(&mut subject, audit);
        }
        let scope_name = if route.registration_scope == "repository" {
            format!("{}/{}", route.owner, route.repository)
        } else {
            route.owner.to_owned()
        };
        Ok(Self {
            subject,
            destination: route.destination.to_owned(),
            registration_scope: route.registration_scope.to_owned(),
            scope_name,
            target_repository_id,
            target_repository_full_name: target_repository_full_name.to_owned(),
            runner_group_id: route.runner_group_id,
            runner_group_name: route.runner_group_name.to_owned(),
            scale_set_id: route.scale_set_id,
            scale_set_name: route.scale_set_name.to_owned(),
        })
    }

    /// Exact numeric Scale Set ID in this immutable route.
    #[must_use]
    pub const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    pub(super) fn subject(&self) -> &str {
        &self.subject
    }

    fn legacy_target_matches(&self, stored: Option<&str>) -> bool {
        match stored {
            Some(stored) => stored == self.target_repository_full_name,
            None if self.registration_scope == "repository" => {
                self.scope_name == self.target_repository_full_name
            }
            None => false,
        }
    }
}

/// One durable, non-replayable permit to delete an exact known Scale Set session.
///
/// The row has already transitioned to a close-attempted state before this
/// value is created. Dropping it or receiving an uncertain response keeps the
/// row unresolved, including after restart.
#[derive(PartialEq, Eq)]
pub struct ScaleSetSessionClosePermit {
    intent_id: i64,
    identity: ScaleSetSessionIdentity,
    session_id: String,
    delete_dispatch_started: bool,
}

impl std::fmt::Debug for ScaleSetSessionClosePermit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScaleSetSessionClosePermit")
            .field("intent_id", &self.intent_id)
            .field("session_id", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl ScaleSetSessionClosePermit {
    /// Durable intent row ID for this one-shot close attempt.
    #[must_use]
    pub fn intent_id(&self) -> i64 {
        self.intent_id
    }

    /// Exact GitHub API destination bound to the reservation.
    #[must_use]
    pub fn destination(&self) -> &str {
        &self.identity.destination
    }

    /// `repository` or `organization` registration scope.
    #[must_use]
    pub fn registration_scope(&self) -> &str {
        &self.identity.registration_scope
    }

    /// Exact scope name (`owner/repository` for repository scope).
    #[must_use]
    pub fn scope_name(&self) -> &str {
        &self.identity.scope_name
    }

    /// Immutable GitHub target repository ID.
    #[must_use]
    pub fn target_repository_id(&self) -> i64 {
        self.identity.target_repository_id
    }

    /// Exact target repository full name authorized by the configuration.
    #[must_use]
    pub fn target_repository_full_name(&self) -> &str {
        &self.identity.target_repository_full_name
    }

    /// Configured internal runner-group ID.
    #[must_use]
    pub fn runner_group_id(&self) -> i64 {
        self.identity.runner_group_id
    }

    /// Configured internal runner-group name.
    #[must_use]
    pub fn runner_group_name(&self) -> &str {
        &self.identity.runner_group_name
    }

    /// Configured Scale Set ID.
    #[must_use]
    pub fn scale_set_id(&self) -> i64 {
        self.identity.scale_set_id
    }

    /// Configured Scale Set name.
    #[must_use]
    pub fn scale_set_name(&self) -> &str {
        &self.identity.scale_set_name
    }
}

impl RepositorySessionCloseClaim for ScaleSetSessionClosePermit {
    fn intent_id(&self) -> i64 {
        self.intent_id
    }

    fn destination(&self) -> &str {
        &self.identity.destination
    }

    fn registration_scope(&self) -> &str {
        &self.identity.registration_scope
    }

    fn scope_name(&self) -> &str {
        &self.identity.scope_name
    }

    fn target_repository_id(&self) -> i64 {
        self.identity.target_repository_id
    }

    fn target_repository_full_name(&self) -> &str {
        &self.identity.target_repository_full_name
    }

    fn runner_group_id(&self) -> i64 {
        self.identity.runner_group_id
    }

    fn runner_group_name(&self) -> &str {
        &self.identity.runner_group_name
    }

    fn scale_set_id(&self) -> i64 {
        self.identity.scale_set_id
    }

    fn scale_set_name(&self) -> &str {
        &self.identity.scale_set_name
    }

    fn session_id_for_cleanup(&self) -> &str {
        &self.session_id
    }

    fn begin_delete_attempt(&mut self) -> bool {
        if self.delete_dispatch_started {
            return false;
        }
        self.delete_dispatch_started = true;
        true
    }
}

fn validate_route(
    route: ReplayRoute<'_>,
    target_repository_id: i64,
    target_repository_full_name: &str,
) -> Result<(), HostError> {
    validate_text(route.destination, 512)?;
    validate_text(route.registration_scope, 16)?;
    validate_text(route.owner, 100)?;
    validate_text(route.runner_group_name, 128)?;
    validate_text(route.scale_set_name, 128)?;
    validate_text(target_repository_full_name, 201)?;
    let (target_owner, target_repository) = target_repository_full_name
        .split_once('/')
        .ok_or(HostError::Journal)?;
    validate_text(target_owner, 100)?;
    validate_text(target_repository, 100)?;
    if target_repository.contains('/')
        || !route.destination.starts_with("https://")
        || route.destination.contains(['?', '#'])
        || !matches!(route.registration_scope, "repository" | "organization")
        || target_repository_id <= 0
        || route.runner_group_id <= 0
        || route.scale_set_id <= 0
        || (route.registration_scope == "repository"
            && (route.repository.is_empty()
                || validate_text(route.repository, 100).is_err()
                || target_repository_full_name != format!("{}/{}", route.owner, route.repository)))
        || (route.registration_scope == "organization"
            && (!route.repository.is_empty() || target_owner != route.owner))
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

pub(super) fn validate_session_id(session_id: &str) -> Result<(), HostError> {
    validate_text(session_id, 256)?;
    if session_id.chars().any(char::is_whitespace) {
        return Err(HostError::Journal);
    }
    Ok(())
}
