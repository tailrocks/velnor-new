use crate::WireError;
use crate::paths::SCALE_SET_ENDPOINT;
use crate::refresh::{StatusClass, classify_status};

use super::error::SessionError;
use super::open::{QueueSession, create_session};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{API_QUERY, bearer, execute, fresh_gate, json_content};

pub fn reopen_session<T>(
    transport: &mut T,
    scale_set_id: i64,
    owner: &str,
    admin_token: &str,
    leaked: &[&str],
) -> Result<QueueSession, SessionError>
where
    T: Transport + ?Sized,
{
    for session_id in leaked {
        release_owned(transport, scale_set_id, session_id, admin_token)?;
    }
    create_session(transport, scale_set_id, owner, admin_token)
}

fn release_owned<T>(
    transport: &mut T,
    scale_set_id: i64,
    session_id: &str,
    admin_token: &str,
) -> Result<(), SessionError>
where
    T: Transport + ?Sized,
{
    if session_id.is_empty() || admin_token.is_empty() {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    let request = SessionRequest {
        method: Method::Delete,
        path: format!("{SCALE_SET_ENDPOINT}/{scale_set_id}/sessions/{session_id}"),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content(), bearer(admin_token)?],
        body: Vec::new(),
    };
    let exchange = execute(transport, &request)?;
    if session_gone(exchange.status, &exchange.body) {
        return Ok(());
    }
    Err(status_error(exchange.status))
}

const SESSION_EXPIRED: &[u8] = b"RunnerScaleSetSessionExpiredException";

fn session_gone(status: u16, body: &[u8]) -> bool {
    match status {
        204 | 404 => true,
        400 => contains_slice(body, SESSION_EXPIRED),
        _ => false,
    }
}

fn contains_slice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn status_error(status: u16) -> SessionError {
    match classify_status(status, &fresh_gate()) {
        Ok(StatusClass::SessionConflict) => SessionError::Conflict,
        Ok(_) => SessionError::Wire(WireError::UnexpectedStatus),
        Err(error) => SessionError::Wire(error),
    }
}
