// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::paths::SCALE_SET_ENDPOINT;
use crate::refresh::{StatusClass, classify_status};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{API_QUERY, bearer, execute, fresh_gate, json_content};

/// What the service said about a session delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionClose {
    /// HTTP 204: the session was closed by this request.
    Closed,
    /// HTTP 404: the session, or the scale set that holds it, no longer exists.
    Gone,
}

pub fn delete_session<T>(
    transport: &mut T,
    scale_set_id: i64,
    session_id: &str,
    admin_token: &str,
) -> Result<SessionClose, SessionError>
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
    if exchange.status == 404 {
        return Ok(SessionClose::Gone);
    }
    match classify_status(exchange.status, &fresh_gate()) {
        Ok(StatusClass::Acked) => Ok(SessionClose::Closed),
        Ok(class) => Err(reject(class)),
        Err(error) => Err(SessionError::Wire(error)),
    }
}
