//! `DELETE .../sessions/{id}`. The pinned client expects HTTP 204 and does not refresh.

use crate::paths::SCALE_SET_ENDPOINT;
use crate::refresh::{StatusClass, classify_status};

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
