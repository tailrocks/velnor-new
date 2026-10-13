//! `POST .../acquirejobs` with a JSON array of int64 ids.

use serde::Deserialize;

use crate::refresh::RefreshGate;
use crate::refresh::StatusClass;
use crate::{AcquireOutcome, WireError, acquire_path, classify_acquire};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{API_QUERY, Answer, attempt, json_content};

/// Acquire `requested` ids. Partial success keeps only ids the service returned.
///
/// `already` is the previously acquired set. The same set is [`AcquireOutcome::Noop`].
/// Timeout and reset are [`SessionError::Uncertain`], not a definite failure.
/// HTTP 401 refreshes once. HTTP 403 does not loop.
///
/// # Errors
///
/// Returns [`WireError::OutsideRequest`] when a returned id was not requested,
/// [`WireError::Malformed`] when `{count, value}` does not match, and the same
/// refresh and transport errors as [`super::poll`].
pub fn acquire<T, R>(
    transport: &mut T,
    scale_set_id: i64,
    requested: &[i64],
    already: &[i64],
    gate: &RefreshGate,
    refresh: R,
) -> Result<AcquireOutcome, SessionError>
where
    T: Transport + ?Sized,
    R: FnMut() -> Result<(), WireError>,
{
    let body = serde_json::to_vec(requested).map_err(|_| WireError::Encode)?;
    let request = SessionRequest {
        method: Method::Post,
        path: acquire_path(scale_set_id),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content()],
        body,
    };
    let answer = attempt(transport, &request, gate, refresh)?;
    accepted(&answer, requested, already)
}

fn accepted(
    answer: &Answer,
    requested: &[i64],
    already: &[i64],
) -> Result<AcquireOutcome, SessionError> {
    match answer.class {
        StatusClass::Ok => {
            let returned = decode_ids(answer.body())?;
            Ok(classify_acquire(requested, &returned, already)?)
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
        return Err(WireError::Malformed.into());
    }
    Ok(parsed.value)
}
