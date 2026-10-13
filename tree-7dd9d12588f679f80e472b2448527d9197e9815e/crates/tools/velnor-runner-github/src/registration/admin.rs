//! `POST /actions/runner-registration`. One extra try on HTTP 401 or 403.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::session::{execute, json_content, user_agent};
use crate::{Method, SessionError, SessionRequest, Transport, WireError};

use super::other_status;

/// Inputs for [`admin_connection`](crate::admin_connection). `Debug` hides the token.
#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct AdminConnectionCall<'a> {
    /// GitHub config URL. Sent as JSON `url`.
    pub config_url: &'a str,
    /// Registration token. Empty never reaches [`Transport`](crate::Transport).
    pub registration_token: &'a str,
}

impl fmt::Debug for AdminConnectionCall<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdminConnectionCall")
            .field("config_url", &self.config_url)
            .field("registration_token", &"[redacted]")
            .finish()
    }
}

/// Actions service URL and admin token. Both are secrets.
///
/// [`Debug`] is redacted. There is no [`Display`].
///
/// [`Debug`]: std::fmt::Debug
/// [`Display`]: std::fmt::Display
#[must_use]
pub struct AdminConnection {
    url: String,
    token: String,
}

impl AdminConnection {
    const fn new(url: String, token: String) -> Self {
        Self { url, token }
    }

    /// Borrow the service URL. Do not log it.
    #[must_use]
    pub const fn expose_url(&self) -> &str {
        self.url.as_str()
    }

    /// Borrow the admin token for `Authorization` only.
    #[must_use]
    pub const fn expose_token(&self) -> &str {
        self.token.as_str()
    }
}

impl fmt::Debug for AdminConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdminConnection([redacted])")
    }
}

impl Drop for AdminConnection {
    fn drop(&mut self) {
        self.url.zeroize();
        self.token.zeroize();
    }
}

/// `POST /actions/runner-registration` with `RemoteAuth`, not `Bearer`.
///
/// Success is any HTTP 2xx. A 401 or 403 is sent once more, then the status
/// stands. A second 403 is [`WireError::Forbidden`]. A second 401 is
/// [`WireError::UnexpectedStatus`], not a session refresh.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty registration
/// token, or when `url` or `token` is null, missing, or empty.
/// [`SessionError::Uncertain`] covers timeout and reset.
pub fn admin_connection<T>(
    transport: &mut T,
    call: &AdminConnectionCall<'_>,
) -> Result<AdminConnection, SessionError>
where
    T: Transport + ?Sized,
{
    let request = admin_request(call)?;
    let exchange = post_with_one_auth_retry(transport, &request)?;
    decode_admin(&exchange.body)
}

/// Exchange one repository registration token for an Actions Service admin
/// connection with exactly one transport attempt.
///
/// This narrow variant is for bounded metadata-discovery flows. It does not
/// retry 401/403 or transport failures: a timeout/reset means the POST outcome
/// is uncertain and the caller must stop. It does not create a Scale Set,
/// session, acquired job, JIT configuration, or runner. The returned connection
/// is still a secret capability and must remain in the control plane.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty registration
/// token or malformed response. HTTP errors are returned without a retry;
/// timeout/reset is [`SessionError::Uncertain`].
pub fn admin_connection_once<T>(
    transport: &mut T,
    call: &AdminConnectionCall<'_>,
) -> Result<AdminConnection, SessionError>
where
    T: Transport + ?Sized,
{
    let request = admin_request(call)?;
    let exchange = execute(transport, &request)?;
    if !is_2xx(exchange.status) {
        return Err(other_status(exchange.status));
    }
    decode_admin(&exchange.body)
}

fn admin_request(call: &AdminConnectionCall<'_>) -> Result<SessionRequest, SessionError> {
    if call.registration_token.is_empty() {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    let body = admin_body(call.config_url)?;
    Ok(SessionRequest {
        method: Method::Post,
        path: "/actions/runner-registration".to_owned(),
        query: None,
        headers: vec![
            json_content(),
            remote_auth(call.registration_token)?,
            user_agent(),
        ],
        body,
    })
}

/// Fresh when `expires_at` is more than 60 seconds after `now`.
///
/// Unix `0` is the zero time and is stale. `now + 60 >= expires_at` is stale.
/// This is stricter than Go `Time.After` at the exact 60 second mark.
/// The caller supplies both instants. This function does not parse a JWT.
#[must_use]
pub const fn admin_token_is_fresh(now_unix: i64, expires_at_unix: i64) -> bool {
    if expires_at_unix == 0 {
        return false;
    }
    match now_unix.checked_add(60) {
        Some(limit) => expires_at_unix > limit,
        None => false,
    }
}

fn admin_body(config_url: &str) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(&AdminPost {
        url: config_url,
        runner_event: "register",
    })
    .map_err(|_| SessionError::from(WireError::Encode))
}

fn remote_auth(token: &str) -> Result<(String, String), SessionError> {
    if token.is_empty() {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    Ok(("Authorization".to_owned(), format!("RemoteAuth {token}")))
}

fn post_with_one_auth_retry<T>(
    transport: &mut T,
    request: &SessionRequest,
) -> Result<crate::Exchange, SessionError>
where
    T: Transport + ?Sized,
{
    let first = execute(transport, request)?;
    if is_2xx(first.status) {
        return Ok(first);
    }
    if !retries_auth(first.status) {
        return Err(other_status(first.status));
    }
    let second = execute(transport, request)?;
    if is_2xx(second.status) {
        return Ok(second);
    }
    Err(other_status(second.status))
}

const fn is_2xx(status: u16) -> bool {
    matches!(status, 200..=299)
}

const fn retries_auth(status: u16) -> bool {
    matches!(status, 401 | 403)
}

fn decode_admin(body: &[u8]) -> Result<AdminConnection, SessionError> {
    let parsed: AdminFields = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    match (secret_text(parsed.url), secret_text(parsed.token)) {
        (Some(url), Some(token)) => Ok(AdminConnection::new(url, token)),
        (url, token) => {
            wipe(url);
            wipe(token);
            Err(SessionError::Wire(WireError::RegistrationRejected))
        }
    }
}

fn secret_text(value: Option<String>) -> Option<String> {
    match value {
        Some(text) if !text.is_empty() => Some(text),
        Some(mut text) => {
            text.zeroize();
            None
        }
        None => None,
    }
}

fn wipe(value: Option<String>) {
    if let Some(mut text) = value {
        text.zeroize();
    }
}

#[derive(Serialize)]
struct AdminPost<'a> {
    url: &'a str,
    runner_event: &'a str,
}

#[derive(Deserialize)]
struct AdminFields {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    token: Option<String>,
}
