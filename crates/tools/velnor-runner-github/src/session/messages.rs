//! `GET` the message queue. HTTP 202 is empty and is not deleted.

use crate::policy::{PollWithTrust, parse_poll_with_trust};
use crate::refresh::{RefreshGate, StatusClass};
use crate::{Poll, WireError, parse_poll};

use super::error::{SessionError, reject};
use super::request::{BearerRole, Method, RequestPurpose, SessionRequest, Transport};
use super::retry::{accept_preview, attempt, bearer, capacity_pair, poll_query, user_agent};
use super::route::MessageQueueRoute;

/// Poll the message queue at `queue_path` (no host).
///
/// `total_capacity` is sent as [`crate::CAPACITY_HEADER`]. It is not free slots.
/// `queue_token` is the queue bearer, not the admin token. `lastMessageId` is
/// omitted when `cursor` is not positive. `api-version=6.0-preview` is always
/// on the query. HTTP 401 refreshes once and repeats this same call.
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
    queue_token: &str,
    gate: &RefreshGate,
    refresh: R,
) -> Result<Poll, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut(&mut T, &mut SessionRequest) -> Result<(), SessionError>,
{
    let mut request = SessionRequest {
        purpose: RequestPurpose::MessageQueuePoll,
        bearer_role: BearerRole::SessionQueue,
        method: Method::Get,
        path: queue_path.to_owned(),
        query: Some(poll_query(cursor)),
        headers: vec![
            accept_preview(),
            bearer(queue_token)?,
            user_agent(),
            capacity_pair(total_capacity),
        ],
        body: Vec::new(),
    };
    let answer = attempt(transport, &mut request, gate, refresh)?;
    match answer.class {
        StatusClass::EmptyPoll => Ok(Poll::Empty),
        StatusClass::Ok => parse_body(answer.status, answer.body()),
        other => Err(reject(other)),
    }
}

/// Poll the same queue endpoint while preserving each event's raw
/// `jobWorkflowRef` beside that event.
///
/// Request construction, the cursor/capacity headers, and one-shot 401 refresh
/// are identical to [`poll`]. The returned trust batch does not authorize any
/// job effect; callers must evaluate each event against the Actions run and
/// configured trust policy before Acquire/JIT.
///
/// # Errors
///
/// Uses the same transport/status errors as [`poll`], plus the pinned trust
/// envelope decoder rejects malformed or misaligned event metadata.
pub fn poll_with_trust<T, R>(
    transport: &mut T,
    queue_path: &str,
    cursor: i64,
    total_capacity: u32,
    queue_token: &str,
    gate: &RefreshGate,
    refresh: R,
) -> Result<PollWithTrust, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut(&mut T, &mut SessionRequest) -> Result<(), SessionError>,
{
    let request = poll_request(
        queue_path,
        Some(poll_query(cursor)),
        total_capacity,
        queue_token,
    )?;
    poll_with_trust_request(transport, request, gate, refresh)
}

/// Poll a host-validated queue route while preserving its query and applying
/// the pinned cursor merge semantics.
pub(crate) fn poll_with_trust_route<T, R>(
    transport: &mut T,
    route: &MessageQueueRoute,
    cursor: i64,
    total_capacity: u32,
    queue_token: &str,
    gate: &RefreshGate,
    refresh: R,
) -> Result<PollWithTrust, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut(&mut T, &mut SessionRequest) -> Result<(), SessionError>,
{
    let request = poll_request(
        route.path(),
        route.poll_query(cursor),
        total_capacity,
        queue_token,
    )?;
    poll_with_trust_request(transport, request, gate, refresh)
}

fn poll_with_trust_request<T, R>(
    transport: &mut T,
    mut request: SessionRequest,
    gate: &RefreshGate,
    refresh: R,
) -> Result<PollWithTrust, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut(&mut T, &mut SessionRequest) -> Result<(), SessionError>,
{
    let answer = attempt(transport, &mut request, gate, refresh)?;
    match answer.class {
        StatusClass::EmptyPoll => Ok(PollWithTrust::Empty),
        StatusClass::Ok => {
            let text = std::str::from_utf8(answer.body()).map_err(|_| WireError::Malformed)?;
            parse_poll_with_trust(answer.status, text).map_err(SessionError::from)
        }
        other => Err(reject(other)),
    }
}

fn poll_request(
    queue_path: &str,
    query: Option<String>,
    total_capacity: u32,
    queue_token: &str,
) -> Result<SessionRequest, SessionError> {
    Ok(SessionRequest {
        purpose: RequestPurpose::MessageQueuePoll,
        bearer_role: BearerRole::SessionQueue,
        method: Method::Get,
        path: queue_path.to_owned(),
        query,
        headers: vec![
            accept_preview(),
            bearer(queue_token)?,
            user_agent(),
            capacity_pair(total_capacity),
        ],
        body: Vec::new(),
    })
}

fn parse_body(status: u16, body: &[u8]) -> Result<Poll, SessionError> {
    let text = std::str::from_utf8(body).map_err(|_| WireError::Malformed)?;
    parse_poll(status, text).map_err(SessionError::from)
}
