//! `GET` the message queue. HTTP 202 is empty and is not deleted.

use crate::refresh::RefreshGate;
use crate::refresh::StatusClass;
use crate::{Poll, WireError, parse_poll};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{accept_preview, attempt, capacity_pair, poll_query};

/// Poll the message queue at `queue_path` (no host).
///
/// `total_capacity` is sent as [`crate::CAPACITY_HEADER`]. It is not free slots.
/// `lastMessageId` is omitted when `cursor` is not positive. `api-version=6.0-preview`
/// is always on the query. HTTP 401 refreshes once and repeats this same call.
/// HTTP 403 does not loop. This function does not acknowledge.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] on timeout or reset,
/// [`SessionError::Conflict`] on HTTP 409, and [`SessionError::Wire`] when the
/// status or body is not a pinned poll. A second HTTP 401 is
/// [`WireError::RefreshExhausted`]. HTTP 403 is [`WireError::Forbidden`].
pub fn poll<T, R>(
    transport: &mut T,
    queue_path: &str,
    cursor: i64,
    total_capacity: u32,
    gate: &RefreshGate,
    refresh: R,
) -> Result<Poll, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut() -> Result<(), WireError>,
{
    let request = SessionRequest {
        method: Method::Get,
        path: queue_path.to_owned(),
        query: Some(poll_query(cursor)),
        headers: vec![accept_preview(), capacity_pair(total_capacity)],
        body: Vec::new(),
    };
    let answer = attempt(transport, &request, gate, refresh)?;
    match answer.class {
        StatusClass::EmptyPoll => Ok(Poll::Empty),
        StatusClass::Ok => parse_body(answer.status, answer.body()),
        other => Err(reject(other)),
    }
}

fn parse_body(status: u16, body: &[u8]) -> Result<Poll, SessionError> {
    let text = std::str::from_utf8(body).map_err(|_| WireError::Malformed)?;
    Ok(parse_poll(status, text)?)
}
