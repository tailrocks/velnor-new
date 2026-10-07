use crate::{
    AdminConnectionCall, RegistrationScope, RegistrationTokenCall, SessionError, WireError,
    actions::{decode_repository, repository_request, status_error},
    registration::{
        RepositoryAdminEvidence, RepositoryDiscoveryAdmin, RepositoryDiscoveryToken,
        admin::{admin_request, decode_admin},
        discovery::{
            discovery_outcome_for_error, safe_header_credential, uncertain_issued_credential,
        },
        token::{decode_token, registration_token_request},
    },
};

use super::super::{DiscoveryCredentialOutcome, DiscoveryCredentialStep};
use super::{
    intent::AsyncDiscoveryIntentStore,
    transport::{AsyncDiscoveryTransport, execute_discovery},
};

/// Read exact repository identity, privacy, and caller-admin facts using one
/// GitHub REST `GET` on the fixed API origin.
///
/// # Errors
///
/// Returns an API, transport, decoding, or policy error without exposing the
/// response body or credential.
pub async fn read_repository_admin_evidence_async<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    host_credential: &str,
) -> Result<RepositoryAdminEvidence, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    transport.bind_github_api_origin()?;
    let request = repository_request(owner, repository, host_credential)?;
    let exchange = execute_discovery(transport, request).await?;
    if exchange.status != 200 {
        return Err(status_error(exchange.status));
    }
    let metadata = decode_repository(&exchange.body, owner, repository)?;
    if !metadata.private || metadata.admin != Some(true) {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok(RepositoryAdminEvidence::from_repository_metadata(
        metadata.id,
        owner.to_owned(),
        repository.to_owned(),
        metadata.full_name,
    ))
}

/// Issue exactly one repository-scoped runner registration token.
///
/// A Pending journal row is durable before the POST. There is no retry. If
/// this future is cancelled after the request starts, the row remains Pending
/// and the exchange future's drop guard signals the host worker to stop and
/// reap its child.
///
/// # Errors
///
/// Returns a secret-safe intent, API, or transport error. An ambiguous POST
/// result is [`SessionError::Uncertain`] and is never retried.
pub async fn issue_repository_discovery_token_async<T, I>(
    transport: &mut T,
    evidence: RepositoryAdminEvidence,
    host_credential: &str,
    intent: &mut I,
) -> Result<RepositoryDiscoveryToken, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
    I: AsyncDiscoveryIntentStore + ?Sized,
{
    if !safe_header_credential(host_credential) {
        return Err(WireError::RegistrationRejected.into());
    }
    transport.bind_github_api_origin()?;
    let request = registration_token_request(&RegistrationTokenCall {
        scope: RegistrationScope::Repository {
            owner: evidence.owner(),
            repo: evidence.repository(),
        },
        pat: host_credential,
    })?;
    let intent_id = intent
        .persist_before(
            DiscoveryCredentialStep::RepositoryRegistrationToken,
            evidence.repository_id(),
            evidence.full_name(),
        )
        .await?;
    let result = execute_discovery(transport, request)
        .await
        .and_then(|exchange| {
            if exchange.status != 201 {
                return Err(crate::registration::other_status(exchange.status));
            }
            decode_token(&exchange.body)
        })
        .and_then(|token| {
            if safe_header_credential(token.expose()) {
                Ok(token)
            } else {
                Err(SessionError::Uncertain)
            }
        })
        .map_err(uncertain_issued_credential);
    let registration_token = record_discovery_outcome_async(intent, intent_id, result).await?;
    let config_url = format!("https://github.com/{}", evidence.full_name());
    Ok(RepositoryDiscoveryToken::new(
        evidence.repository_id(),
        evidence.full_name().to_owned(),
        config_url,
        registration_token,
    ))
}

/// Exchange the single-use repository token exactly once for a read-only
/// Actions metadata capability.
///
/// # Errors
///
/// Returns a secret-safe intent, API, or transport error. An ambiguous POST
/// result is [`SessionError::Uncertain`] and is never retried.
pub async fn exchange_repository_discovery_admin_once_async<T, I>(
    transport: &mut T,
    token: RepositoryDiscoveryToken,
    intent: &mut I,
) -> Result<RepositoryDiscoveryAdmin, SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
    I: AsyncDiscoveryIntentStore + ?Sized,
{
    transport.bind_github_api_origin()?;
    let request = admin_request(&AdminConnectionCall {
        config_url: token.config_url(),
        registration_token: token.registration_token(),
    })?;
    let repository_id = token.repository_id();
    let repository_full_name = token.repository_full_name().to_owned();
    drop(token);
    let intent_id = intent
        .persist_before(
            DiscoveryCredentialStep::ActionsAdminExchange,
            repository_id,
            &repository_full_name,
        )
        .await?;
    let result = execute_discovery(transport, request)
        .await
        .and_then(|exchange| {
            if !(200..=299).contains(&exchange.status) {
                return Err(crate::registration::other_status(exchange.status));
            }
            decode_admin(&exchange.body)
        })
        .and_then(|connection| {
            if safe_header_credential(connection.expose_token()) {
                Ok(connection)
            } else {
                Err(SessionError::Uncertain)
            }
        })
        .map_err(uncertain_issued_credential);
    let connection = record_discovery_outcome_async(intent, intent_id, result).await?;
    Ok(RepositoryDiscoveryAdmin::new(connection))
}

async fn record_discovery_outcome_async<T, I>(
    intent: &mut I,
    id: crate::DiscoveryIntentId,
    result: Result<T, SessionError>,
) -> Result<T, SessionError>
where
    I: AsyncDiscoveryIntentStore + ?Sized,
{
    match result {
        Ok(value) => {
            if intent
                .record_outcome(id, DiscoveryCredentialOutcome::Succeeded)
                .await
                .is_err()
            {
                return Err(SessionError::Uncertain);
            }
            Ok(value)
        }
        Err(error) => {
            let outcome = discovery_outcome_for_error(error);
            if intent.record_outcome(id, outcome).await.is_err() {
                return Err(SessionError::Uncertain);
            }
            Err(error)
        }
    }
}
