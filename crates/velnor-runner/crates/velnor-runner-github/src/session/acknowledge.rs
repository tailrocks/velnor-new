use crate::refresh::RefreshGate;
use crate::refresh::StatusClass;
use crate::{ParsedBatch, WireError, may_ack};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{attempt, bearer, json_content, user_agent};

#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct AckScope<'a> {
    pub replay_safe: bool,

    pub sole_unacquired_offer: bool,

    pub queue_token: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Ack {
    Deleted,

    Suppressed,
}

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
