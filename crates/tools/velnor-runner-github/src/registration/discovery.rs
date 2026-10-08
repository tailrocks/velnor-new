//! One-shot credential bootstrap for read-only Scale Set discovery.
//!
//! These calls establish repository administration and obtain short-lived
//! Actions-service credentials. They do not prove runner-group policy or
//! Scale Set admission, and they must not be used to acquire jobs or create a
//! Scale Set. The host must persist intent before each `POST` and use a
//! bounded, fixed-origin transport that rejects redirects.

use std::num::NonZeroU64;

mod types;

pub use types::{
    OrganizationAdminEvidence, OrganizationDiscoveryToken, RepositoryAdminEvidence,
    RepositoryDiscoveryToken,
};

use super::discovery_admin::RepositoryDiscoveryAdmin;

use crate::{
    AdminConnectionCall, MessageQueueRoute, RegistrationScope, RegistrationTokenCall, SessionError,
    Transport, WireError, admin_connection_once, get_actions_repository, registration_token,
};

/// Transport required by repository discovery bootstrap.
///
/// Implementations must bind only the fixed api.github.com HTTPS origin for
/// bootstrap calls, validate the returned Actions-service HTTPS origin before
/// admin-token GETs, reject redirects, perform no automatic retries, enforce
/// a whole-request deadline and response-byte cap while streaming, and never
/// log request/response bodies or authorization headers. This API builds the
/// only paths and queries used by the discovery calls.
pub trait DiscoveryTransport: Transport {
    /// Bind the fixed GitHub API origin without sending a request.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe failure if the fixed API origin cannot be bound.
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError>;

    /// Validate and bind the returned Actions-service HTTPS origin without
    /// sending a request. Reject userinfo, query, fragment, wrong ports,
    /// unrecognized hosts, and any base path not allowed by the host.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe failure when the service origin is not allowed.
    fn bind_actions_service_origin(&mut self, url: &str) -> Result<(), SessionError>;

    /// Validate and bind the complete returned `MessageQueueURL` without sending
    /// a request, then return its relative path and raw query. Implementations
    /// must treat this as a distinct route role from the Actions Service origin,
    /// even when the two URLs share a host. The route query is secret-bearing;
    /// do not log it. The host must enforce origin/provenance policy before
    /// returning the route.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe failure when the queue URL or its path/query is invalid.
    fn bind_message_queue_origin(&mut self, url: &str) -> Result<MessageQueueRoute, SessionError>;
}

/// Credential-issuance operation that the host must durably journal before
/// sending its corresponding POST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryCredentialStep {
    /// Repository-scoped registration-token issuance.
    RepositoryRegistrationToken,
    /// Organization-scoped registration-token issuance for a bounded metadata read.
    OrganizationRegistrationToken,
    /// One-shot Actions-service admin credential exchange.
    ActionsAdminExchange,
}

/// Stable host-journal identifier for one credential issuance attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[must_use]
pub struct DiscoveryIntentId(NonZeroU64);

impl DiscoveryIntentId {
    /// Construct from a positive ID allocated and persisted by the host store.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Return the stable host-journal identifier.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Terminal accounting state for one credential-issuance call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum DiscoveryCredentialOutcome {
    /// The expected secret response was received and consumed in memory.
    Succeeded,
    /// The remote service explicitly rejected the request.
    Rejected,
    /// The outcome or completion journal write is not known.
    Uncertain,
}

/// Durable host intent required before either credential-issuance POST.
///
/// Implementations must atomically persist the step and reject replay while a
/// prior attempt is unresolved. This record contains repository scope only;
/// it must never contain a PAT, registration token, admin token, or response
/// body. If persistence fails, the corresponding transport request is not
/// sent.
pub trait DiscoveryIntentStore {
    /// Persist one credential-issuance intent before the matching POST.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe error when durable intent cannot be recorded.
    fn persist_before(
        &mut self,
        step: DiscoveryCredentialStep,
        repository_id: i64,
        full_name: &str,
    ) -> Result<DiscoveryIntentId, SessionError>;

    /// Persist the observed result for the exact one-shot operation.
    ///
    /// If this fails after the POST, the caller must keep the operation
    /// unresolved and must not issue a replacement request automatically.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe error when the outcome cannot be durably recorded.
    fn record_outcome(
        &mut self,
        id: DiscoveryIntentId,
        outcome: DiscoveryCredentialOutcome,
    ) -> Result<(), SessionError>;
}

/// Read exact repository identity, visibility, and caller-admin facts for a
/// bounded discovery bootstrap.
///
/// This performs one repository REST `GET`. It rejects a public repository or
/// missing/false admin permission before any credential-issuance `POST`.
///
/// # Errors
///
/// Returns a terminal API/transport error or
/// [`WireError::RegistrationRejected`] when the repository is not private or
/// administrator permission is not explicitly true.
pub fn read_repository_admin_evidence<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    host_credential: &str,
) -> Result<RepositoryAdminEvidence, SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    transport.bind_github_api_origin()?;
    let metadata = get_actions_repository(transport, owner, repository, host_credential)?;
    if !metadata.private || metadata.admin != Some(true) {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok(RepositoryAdminEvidence {
        id: metadata.id,
        owner: owner.to_owned(),
        repository: repository.to_owned(),
        full_name: metadata.full_name,
    })
}

/// Issue exactly one repository registration token for discovery.
///
/// The opaque evidence is consumed so the same successful preflight cannot be
/// reused in memory to issue another token. A timeout/reset or a malformed
/// successful response is uncertain; the caller must stop rather than retry.
/// The host remains responsible for a durable intent record before this call.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] for an unknown POST outcome, a
/// malformed successful response, or an unusable token response. No retry is
/// made.
pub fn issue_repository_discovery_token<T>(
    transport: &mut T,
    evidence: RepositoryAdminEvidence,
    host_credential: &str,
    intent: &mut impl DiscoveryIntentStore,
) -> Result<RepositoryDiscoveryToken, SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    if !safe_header_credential(host_credential) {
        return Err(WireError::RegistrationRejected.into());
    }
    transport.bind_github_api_origin()?;
    let intent_id = intent.persist_before(
        DiscoveryCredentialStep::RepositoryRegistrationToken,
        evidence.id,
        &evidence.full_name,
    )?;
    let registration_token = registration_token(
        transport,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository {
                owner: &evidence.owner,
                repo: &evidence.repository,
            },
            pat: host_credential,
        },
    );
    let registration_token = registration_token
        .and_then(|token| {
            if safe_header_credential(token.expose()) {
                Ok(token)
            } else {
                Err(SessionError::Uncertain)
            }
        })
        .map_err(uncertain_issued_credential);
    let registration_token = record_discovery_outcome(intent, intent_id, registration_token)?;
    let config_url = format!("https://github.com/{}", evidence.full_name);
    Ok(RepositoryDiscoveryToken {
        repository_id: evidence.id,
        repository_full_name: evidence.full_name,
        config_url,
        registration_token,
    })
}

/// Exchange the consumed repository registration token exactly once for an
/// Actions-service credential used only by bounded metadata `GET`s.
///
/// HTTP 401/403 and transport failures are never replayed. The host must
/// validate the returned service URL against its fixed HTTPS origin policy
/// before sending any request with the returned admin token.
///
/// # Errors
///
/// Returns an API/transport error. An ambiguous or malformed successful
/// response is not safe to retry.
pub fn exchange_repository_discovery_admin_once<T>(
    transport: &mut T,
    token: RepositoryDiscoveryToken,
    intent: &mut impl DiscoveryIntentStore,
) -> Result<RepositoryDiscoveryAdmin, SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    transport.bind_github_api_origin()?;
    let repository_id = token.repository_id;
    let repository_full_name = token.repository_full_name.clone();
    let intent_id = intent.persist_before(
        DiscoveryCredentialStep::ActionsAdminExchange,
        repository_id,
        &repository_full_name,
    )?;
    let result = admin_connection_once(
        transport,
        &AdminConnectionCall {
            config_url: &token.config_url,
            registration_token: token.registration_token.expose(),
        },
    )
    .and_then(|connection| {
        if safe_header_credential(connection.expose_token()) {
            Ok(connection)
        } else {
            Err(SessionError::Uncertain)
        }
    })
    .map_err(uncertain_issued_credential);
    drop(token);
    let connection = record_discovery_outcome(intent, intent_id, result)?;
    RepositoryDiscoveryAdmin::new(connection, repository_id, repository_full_name)
}

fn record_discovery_outcome<T>(
    intent: &mut impl DiscoveryIntentStore,
    id: DiscoveryIntentId,
    result: Result<T, SessionError>,
) -> Result<T, SessionError> {
    match result {
        Ok(value) => {
            if intent
                .record_outcome(id, DiscoveryCredentialOutcome::Succeeded)
                .is_err()
            {
                return Err(SessionError::Uncertain);
            }
            Ok(value)
        }
        Err(error) => {
            let outcome = discovery_outcome_for_error(error);
            if intent.record_outcome(id, outcome).is_err() {
                return Err(SessionError::Uncertain);
            }
            Err(error)
        }
    }
}

pub(super) fn discovery_outcome_for_error(error: SessionError) -> DiscoveryCredentialOutcome {
    if error.certainty() == crate::Certainty::Definite {
        DiscoveryCredentialOutcome::Rejected
    } else {
        DiscoveryCredentialOutcome::Uncertain
    }
}

pub(super) fn uncertain_issued_credential(error: SessionError) -> SessionError {
    match error {
        SessionError::Wire(WireError::Malformed | WireError::RegistrationRejected) => {
            SessionError::Uncertain
        }
        other => other,
    }
}

pub(super) fn safe_header_credential(value: &str) -> bool {
    !value.is_empty()
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}
