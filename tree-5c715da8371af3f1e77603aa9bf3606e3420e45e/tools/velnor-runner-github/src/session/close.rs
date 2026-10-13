//! `DELETE .../sessions/{id}`. The pinned client expects HTTP 204 and does not refresh.

use crate::paths::SCALE_SET_ENDPOINT;
use crate::refresh::{StatusClass, classify_status};
use crate::registration::{AsyncDiscoveryTransport, execute_discovery};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{API_QUERY, bearer, execute, fresh_gate, json_content};

/// Delete the owned session with the admin bearer. Any status other than 204 is not success.
///
/// HTTP 401 is not retried. HTTP 409 does not delete another session.
/// A missing response is [`SessionError::Uncertain`], which is not success.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] on timeout or reset,
/// [`SessionError::Conflict`] on HTTP 409, [`WireError`](crate::WireError)
/// [`Forbidden`](crate::WireError::Forbidden) on HTTP 403, and
/// [`UnexpectedStatus`](crate::WireError::UnexpectedStatus) for every other
/// non-204 status.
pub fn delete_session<T>(
    transport: &mut T,
    scale_set_id: i64,
    session_id: &str,
    admin_token: &str,
) -> Result<(), SessionError>
where
    T: Transport + ?Sized,
{
    let request = SessionRequest {
        method: Method::Delete,
        path: format!("{SCALE_SET_ENDPOINT}/{scale_set_id}/sessions/{session_id}"),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content(), bearer(admin_token)?],
        body: Vec::new(),
    };
    let exchange = execute(transport, &request)?;
    match classify_status(exchange.status, &fresh_gate()) {
        Ok(StatusClass::Acked) => Ok(()),
        Ok(class) => Err(reject(class)),
        Err(error) => Err(SessionError::Wire(error)),
    }
}

/// Async one-shot counterpart used by journal-owned cleanup after a restart.
/// It intentionally has no refresh or retry path; only an observed 204 is
/// success. The caller binds the exact service origin before this call.
pub(crate) async fn delete_session_async<T>(
    transport: &mut T,
    scale_set_id: i64,
    session_id: &str,
    admin_token: &str,
) -> Result<(), SessionError>
where
    T: AsyncDiscoveryTransport + ?Sized,
{
    if scale_set_id <= 0 || !safe_path_segment(session_id) {
        return Err(crate::WireError::RegistrationRejected.into());
    }
    let request = SessionRequest {
        method: Method::Delete,
        path: format!("{SCALE_SET_ENDPOINT}/{scale_set_id}/sessions/{session_id}"),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content(), bearer(admin_token)?],
        body: Vec::new(),
    };
    let exchange = execute_discovery(transport, request).await?;
    match classify_status(exchange.status, &fresh_gate()) {
        Ok(StatusClass::Acked) => Ok(()),
        Ok(class) => Err(reject(class)),
        Err(error) => Err(SessionError::Wire(error)),
    }
}

pub(crate) fn safe_path_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
}
