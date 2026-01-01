//! Message ack is `DELETE {queue}/{id}` (`deleteMessage`) and expects HTTP 204.

use crate::refresh::RefreshGate;
use crate::refresh::StatusClass;
use crate::{ParsedBatch, WireError, may_ack};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{attempt, bearer, json_content, user_agent};

/// Delete policy for one batch. Keeps [`ack`] under the argument limit.
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct AckScope<'a> {
    /// False suppresses the delete. A replay that is not safe must not ack.
    pub replay_safe: bool,
    /// True suppresses the delete so a sole unacquired offer stays queued.
    pub sole_unacquired_offer: bool,
    /// Queue bearer. Not the admin token.
    pub queue_token: &'a str,
}

/// Result of an acknowledgement attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Ack {
    /// HTTP 204. The message was deleted.
    Deleted,
    /// No request was sent.
    Suppressed,
}

/// Delete `batch.message_id` when the batch is safe to acknowledge.
///
/// The transport is not called when [`may_ack`] is false. That covers a message
/// id below zero, any [`crate::InnerKind::Unsupported`], and `replay_safe == false`.
/// `sole_unacquired_offer` also suppresses the call: an unacquired `JobAvailable`
/// that is the only retained copy must not be deleted. HTTP 401 refreshes once.
/// HTTP 403 does not loop. The pinned client expects HTTP 204.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] on timeout or reset,
/// [`SessionError::Conflict`] on HTTP 409, and [`SessionError::Wire`] for any
/// other non-204 outcome. A second HTTP 401 is [`WireError::RefreshExhausted`].
pub fn ack<T, R>(
    transport: &mut T,
    queue_path: &str,
    batch: &ParsedBatch,
    scope: &AckScope<'_>,
    gate: &RefreshGate,
    refresh: R,
) -> Result<Ack, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut() -> Result<(), WireError>,
{
    if scope.sole_unacquired_offer || !may_ack(batch, scope.replay_safe) {
        return Ok(Ack::Suppressed);
    }
    let request = SessionRequest {
        method: Method::Delete,
        path: message_path(queue_path, batch.message_id),
        query: None,
        headers: vec![json_content(), bearer(scope.queue_token)?, user_agent()],
        body: Vec::new(),
    };
    let answer = attempt(transport, &request, gate, refresh)?;
    match answer.class {
        StatusClass::Acked => Ok(Ack::Deleted),
        other => Err(reject(other)),
    }
}

fn message_path(queue_path: &str, message_id: i64) -> String {
    format!("{}/{}", queue_path.trim_end_matches('/'), message_id)
}
