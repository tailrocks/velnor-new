use std::fmt;

use serde::Deserialize;
use zeroize::Zeroize;

use crate::session::{bearer, execute, user_agent};
use crate::{Method, SessionError, SessionRequest, Transport, WireError};

use super::other_status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum RegistrationScope<'a> {
    Repository { owner: &'a str, repo: &'a str },

    Organization { org: &'a str },

    Enterprise { enterprise: &'a str },
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct RegistrationTokenCall<'a> {
    pub scope: RegistrationScope<'a>,

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

#[must_use]
pub struct RegistrationToken {
    raw: String,
}

impl RegistrationToken {
    const fn new(raw: String) -> Self {
        Self { raw }
    }

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

pub fn repository_registration_token_path(owner: &str, repo: &str) -> Result<String, WireError> {
    if owner.is_empty() || repo.is_empty() {
        return Err(WireError::RegistrationRejected);
    }
    Ok(format!(
        "/repos/{owner}/{repo}/actions/runners/registration-token"
    ))
}

pub fn organization_registration_token_path(org: &str) -> Result<String, WireError> {
    if org.is_empty() {
        return Err(WireError::RegistrationRejected);
    }
    Ok(format!("/orgs/{org}/actions/runners/registration-token"))
}

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

pub fn registration_token<T>(
    transport: &mut T,
    call: &RegistrationTokenCall<'_>,
) -> Result<RegistrationToken, SessionError>
where
    T: Transport + ?Sized,
{
    let path = scope_path(call.scope)?;
    let request = SessionRequest {
        method: Method::Post,
        path,
        query: None,
        headers: vec![v3_content(), bearer(call.pat)?, user_agent()],
        body: Vec::new(),
    };
    let exchange = execute(transport, &request)?;
    if exchange.status != 201 {
        return Err(other_status(exchange.status));
    }
    decode_token(&exchange.body)
}

fn v3_content() -> (String, String) {
    (
        "Content-Type".to_owned(),
        "application/vnd.github.v3+json".to_owned(),
    )
}

fn decode_token(body: &[u8]) -> Result<RegistrationToken, SessionError> {
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
