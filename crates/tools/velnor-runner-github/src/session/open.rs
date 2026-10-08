//! Create and refresh a message session. HTTP 409 does not delete the other one.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::paths::SCALE_SET_ENDPOINT;
use crate::refresh::{StatusClass, classify_status};
use crate::{Statistics, WireError};

use super::error::{SessionError, reject};
use super::request::{Exchange, Method, SessionRequest, Transport};
use super::retry::{API_QUERY, execute, fresh_gate, json_content};

/// Session the service created. The queue token is not in [`Debug`].
pub struct QueueSession {
    /// Service session id.
    pub session_id: String,
    /// Message queue path or URL. Poll uses this path.
    pub message_queue_url: String,
    statistics: Option<Statistics>,
    token: String,
}

impl QueueSession {
    /// Borrow the queue token for the `Authorization` header only.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Counts from the create or refresh body. Absent when the service omitted them.
    #[must_use]
    pub fn statistics(&self) -> Option<&Statistics> {
        self.statistics.as_ref()
    }

    /// Retire queue credentials and the returned URL after one-shot close.
    pub(crate) fn retire(&mut self) {
        self.token.zeroize();
        self.message_queue_url.zeroize();
    }
}

impl fmt::Debug for QueueSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QueueSession")
            .field("session_id", &self.session_id)
            .field("message_queue_url", &self.message_queue_url)
            .field("statistics", &self.statistics)
            .field("token", &"[redacted]")
            .finish()
    }
}

impl Drop for QueueSession {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

/// `POST .../sessions`. One attempt. HTTP 401 is not a refresh loop.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty owner,
/// [`SessionError::Conflict`] on HTTP 409, and [`SessionError::Wire`] when
/// the body is not a session.
pub fn create_session<T>(
    transport: &mut T,
    scale_set_id: i64,
    owner: &str,
    admin_token: &str,
) -> Result<QueueSession, SessionError>
where
    T: Transport + ?Sized,
{
    if owner.is_empty() || !safe_token(admin_token) {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    let body = serde_json::to_vec(&Owner { owner_name: owner }).map_err(|_| WireError::Encode)?;
    let request = session_request(
        Method::Post,
        format!("{SCALE_SET_ENDPOINT}/{scale_set_id}/sessions"),
        admin_token,
        body,
    );
    let exchange = execute(transport, &request)?;
    finish(&exchange)
}

/// `PATCH .../sessions/{id}`. Does not delete a conflicting session.
///
/// # Errors
///
/// Same failures as [`create_session`], plus an empty session id.
pub fn refresh_session<T>(
    transport: &mut T,
    scale_set_id: i64,
    session_id: &str,
    admin_token: &str,
) -> Result<QueueSession, SessionError>
where
    T: Transport + ?Sized,
{
    if session_id.is_empty() || !safe_token(admin_token) {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    let request = session_request(
        Method::Patch,
        format!("{SCALE_SET_ENDPOINT}/{scale_set_id}/sessions/{session_id}"),
        admin_token,
        Vec::new(),
    );
    let exchange = execute(transport, &request)?;
    finish(&exchange)
}

/// PATCH only while `held` is still the expired snapshot.
///
/// A different session id or queue token means another refresh already won.
/// The transport is not called.
///
/// # Errors
///
/// Same failures as [`refresh_session`] when the snapshot still matches.
pub fn refresh_if_current<T>(
    transport: &mut T,
    scale_set_id: i64,
    held: &QueueSession,
    expired_session_id: &str,
    expired_token: &str,
    admin_token: &str,
) -> Result<Option<QueueSession>, SessionError>
where
    T: Transport + ?Sized,
{
    if held.session_id != expired_session_id || held.token() != expired_token {
        return Ok(None);
    }
    refresh_session(transport, scale_set_id, &held.session_id, admin_token).map(Some)
}

/// Refresh the queue session used by an operation that received HTTP 401.
///
/// If `request` still carries the current queue token, this sends the pinned
/// `PATCH .../sessions/{id}` request and replaces the in-memory session with
/// the response. If another operation already refreshed `session`, it skips
/// the PATCH and uses that newer token. In both cases the request bearer is
/// replaced before the caller retries the operation.
///
/// The returned URL is the current `messageQueueUrl`. The caller must validate
/// it and route the transport and request path to that URL before returning
/// from its refresh callback. In particular, a changed queue origin must not
/// be replayed against the stale origin.
///
/// # Errors
///
/// Returns [`SessionError::Wire`] when the request has no unique bearer,
/// [`WireError::Malformed`] if the PATCH changes session identity, and the
/// errors from [`refresh_session`].
pub fn refresh_queue_request<'s, T>(
    transport: &mut T,
    scale_set_id: i64,
    session: &'s mut QueueSession,
    admin_token: &str,
    request: &mut SessionRequest,
) -> Result<&'s str, SessionError>
where
    T: Transport + ?Sized,
{
    if session.session_id.is_empty() || !safe_token(admin_token) {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    if request.bearer_token().is_none() {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    if request.uses_bearer(session.token()) {
        let session_id = session.session_id.clone();
        let refreshed = refresh_session(transport, scale_set_id, &session_id, admin_token)?;
        if refreshed.session_id != session_id {
            return Err(SessionError::Wire(WireError::Malformed));
        }
        *session = refreshed;
    }
    request.replace_bearer(session.token())?;
    Ok(session.message_queue_url.as_str())
}

fn session_request(
    method: Method,
    path: String,
    admin_token: &str,
    body: Vec<u8>,
) -> SessionRequest {
    SessionRequest {
        method,
        path,
        query: Some(API_QUERY.to_owned()),
        headers: vec![
            json_content(),
            ("Authorization".to_owned(), format!("Bearer {admin_token}")),
        ],
        body,
    }
}

fn safe_token(token: &str) -> bool {
    !token.is_empty()
        && !token
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

fn finish(exchange: &Exchange) -> Result<QueueSession, SessionError> {
    match classify_status(exchange.status, &fresh_gate()) {
        Ok(StatusClass::Ok) => decode(&exchange.body),
        Ok(class) => Err(reject(class)),
        Err(error) => Err(SessionError::Wire(error)),
    }
}

fn decode(body: &[u8]) -> Result<QueueSession, SessionError> {
    let parsed: Body = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    if parsed.session_id.is_empty()
        || parsed.message_queue_url.is_empty()
        || parsed.message_queue_access_token.is_empty()
        || parsed
            .message_queue_access_token
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(SessionError::Wire(WireError::Malformed));
    }
    Ok(QueueSession {
        session_id: parsed.session_id,
        message_queue_url: parsed.message_queue_url,
        statistics: parsed.statistics,
        token: parsed.message_queue_access_token,
    })
}

#[derive(Serialize)]
struct Owner<'a> {
    #[serde(rename = "ownerName")]
    owner_name: &'a str,
}

#[derive(Deserialize)]
struct Body {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "messageQueueUrl")]
    message_queue_url: String,
    #[serde(rename = "messageQueueAccessToken")]
    message_queue_access_token: String,
    #[serde(default)]
    statistics: Option<Statistics>,
}
