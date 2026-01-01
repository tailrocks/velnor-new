//! `POST` a runner registration token. The PAT is not refreshed.

use std::fmt;

use serde::Deserialize;
use zeroize::Zeroize;

use crate::session::{bearer, execute, user_agent};
use crate::{
    BearerRole, Method, RequestPurpose, SessionError, SessionRequest, Transport, WireError,
};

use super::other_status;

/// Where the registration token is minted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum RegistrationScope<'a> {
    /// `/repos/{owner}/{repo}/actions/runners/registration-token`.
    Repository {
        /// Repository owner.
        owner: &'a str,
        /// Repository name.
        repo: &'a str,
    },
    /// `/orgs/{org}/actions/runners/registration-token`.
    Organization {
        /// Organization login.
        org: &'a str,
    },
    /// `/enterprises/{enterprise}/actions/runners/registration-token`.
    Enterprise {
        /// Enterprise slug.
        enterprise: &'a str,
    },
}

/// Inputs for [`registration_token`](crate::registration_token). `Debug` hides `pat`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct RegistrationTokenCall<'a> {
    /// Scope path. An empty component is rejected before transport.
    pub scope: RegistrationScope<'a>,
    /// Caller credential. Not a registration token.
    pub pat: &'a str,
}

impl fmt::Debug for RegistrationTokenCall<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegistrationTokenCall")
            .field("scope", &self.scope)
            .field("pat", &"[redacted]")
            .finish()
    }
}

/// Registration token. [`Debug`] is redacted. There is no [`Display`].
///
/// [`Debug`]: std::fmt::Debug
/// [`Display`]: std::fmt::Display
#[must_use]
pub struct RegistrationToken {
    raw: String,
}

impl RegistrationToken {
    const fn new(raw: String) -> Self {
        Self { raw }
    }

    /// Borrow the token for the admin-connection header only.
    #[must_use]
    pub const fn expose(&self) -> &str {
        self.raw.as_str()
    }
}

impl fmt::Debug for RegistrationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RegistrationToken([redacted])")
    }
}

impl Drop for RegistrationToken {
    fn drop(&mut self) {
        self.raw.zeroize();
    }
}

/// Repository registration-token path.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] when `owner` or `repo` is empty.
pub fn repository_registration_token_path(owner: &str, repo: &str) -> Result<String, WireError> {
    if owner.is_empty() || repo.is_empty() {
        return Err(WireError::RegistrationRejected);
    }
    Ok(format!(
        "/repos/{owner}/{repo}/actions/runners/registration-token"
    ))
}

/// Organization registration-token path.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] when `org` is empty.
pub fn organization_registration_token_path(org: &str) -> Result<String, WireError> {
    if org.is_empty() {
        return Err(WireError::RegistrationRejected);
    }
    Ok(format!("/orgs/{org}/actions/runners/registration-token"))
}

/// Enterprise registration-token path.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] when `enterprise` is empty.
pub fn enterprise_registration_token_path(enterprise: &str) -> Result<String, WireError> {
    if enterprise.is_empty() {
        return Err(WireError::RegistrationRejected);
    }
    Ok(format!(
        "/enterprises/{enterprise}/actions/runners/registration-token"
    ))
}

fn scope_path(scope: RegistrationScope<'_>) -> Result<String, WireError> {
    match scope {
        RegistrationScope::Repository { owner, repo } => {
            repository_registration_token_path(owner, repo)
        }
        RegistrationScope::Organization { org } => organization_registration_token_path(org),
        RegistrationScope::Enterprise { enterprise } => {
            enterprise_registration_token_path(enterprise)
        }
    }
}

/// `POST` a registration token. Empty scope or PAT never reaches [`Transport`].
///
/// Success is HTTP 201 and a non-empty `token`. `expires_at` is ignored.
/// HTTP 401 is not a refresh loop. HTTP 403 is [`WireError::Forbidden`].
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty scope, an empty
/// PAT, or a null, missing, or empty token. A non-201 status is
/// [`WireError::UnexpectedStatus`] or the pinned status class.
/// [`SessionError::Uncertain`] covers timeout and reset.
pub fn registration_token<T>(
    transport: &mut T,
    call: &RegistrationTokenCall<'_>,
) -> Result<RegistrationToken, SessionError>
where
    T: Transport + ?Sized,
{
    let request = registration_token_request(call)?;
    let exchange = execute(transport, &request)?;
    if exchange.status != 201 {
        return Err(other_status(exchange.status));
    }
    decode_token(&exchange.body)
}

pub(crate) fn registration_token_request(
    call: &RegistrationTokenCall<'_>,
) -> Result<SessionRequest, SessionError> {
    let path = scope_path(call.scope)?;
    Ok(SessionRequest {
        purpose: RequestPurpose::RegistrationTokenIssue,
        bearer_role: BearerRole::GithubRestCredential,
        method: Method::Post,
        path,
        query: None,
        headers: vec![v3_content(), bearer(call.pat)?, user_agent()],
        body: Vec::new(),
    })
}

fn v3_content() -> (String, String) {
    (
        "Content-Type".to_owned(),
        "application/vnd.github.v3+json".to_owned(),
    )
}

pub(crate) fn decode_token(body: &[u8]) -> Result<RegistrationToken, SessionError> {
    let parsed: TokenJson = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    let Some(token) = parsed.token else {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    };
    if token.is_empty() {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    Ok(RegistrationToken::new(token))
}

#[derive(Deserialize)]
struct TokenJson {
    #[serde(default)]
    token: Option<String>,
}
