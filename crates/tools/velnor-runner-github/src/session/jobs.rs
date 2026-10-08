//! `POST .../acquirejobs` with a `JSON` array of int64 ids.

use serde::Deserialize;

use crate::refresh::{RefreshGate, StatusClass};
use crate::{AcquireOutcome, WireError, acquire_path, classify_acquire};

use super::error::{SessionError, reject};
use super::request::{BearerRole, Method, RequestPurpose, SessionRequest, Transport};
use super::retry::{API_QUERY, Answer, attempt, bearer, json_content, user_agent};

/// Acquire `requested` ids. Partial success keeps only ids the service returned.
///
/// `already` is the previously acquired set. The same set is [`AcquireOutcome::Noop`].
/// `queue_token` overwrites the admin bearer. Capacity is not sent.
/// Timeout and reset are [`SessionError::Uncertain`], not a definite failure.
/// HTTP 401 refreshes once. HTTP 403 does not loop.
///
/// # Errors
///
/// Returns [`SessionError::Uncertain`] when an HTTP 200 response cannot be
/// decoded or admitted, because the service may already have acquired a job,
/// and the same refresh and transport errors as [`crate::session::poll`].
pub fn acquire<T, R>(
    transport: &mut T,
    scale_set_id: i64,
    requested: &[i64],
    already: &[i64],
    queue_token: &str,
    gate: &RefreshGate,
    refresh: R,
) -> Result<AcquireOutcome, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut(&mut T, &mut SessionRequest) -> Result<(), SessionError>,
{
    let body = serde_json::to_vec(requested).map_err(|_| WireError::Encode)?;
    let mut request = SessionRequest {
        purpose: RequestPurpose::AcquireJobs,
        bearer_role: BearerRole::SessionQueue,
        method: Method::Post,
        path: acquire_path(scale_set_id),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content(), bearer(queue_token)?, user_agent()],
        body,
    };
    let answer = attempt(transport, &mut request, gate, refresh)?;
    accepted(&answer, requested, already)
}

fn accepted(
    answer: &Answer,
    requested: &[i64],
    already: &[i64],
) -> Result<AcquireOutcome, SessionError> {
    if (200..300).contains(&answer.status) && answer.class != StatusClass::Ok {
        return Err(SessionError::Uncertain);
    }
    match answer.class {
        StatusClass::Ok => {
            let returned = decode_ids(answer.body()).map_err(|_| SessionError::Uncertain)?;
            classify_acquire(requested, &returned, already).map_err(|_| SessionError::Uncertain)
        }
        other => Err(reject(other)),
    }
}

#[derive(Deserialize)]
struct AcquirePage {
    count: i64,
    value: Vec<i64>,
}

fn decode_ids(body: &[u8]) -> Result<Vec<i64>, SessionError> {
    let parsed: AcquirePage = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    let len = i64::try_from(parsed.value.len()).map_err(|_| WireError::Malformed)?;
    if parsed.count != len {
        return Err(SessionError::from(WireError::Malformed));
    }
    Ok(parsed.value)
}
