// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::refresh::{RefreshGate, StatusClass};
use crate::{AcquireOutcome, WireError, acquire_path, classify_acquire};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{API_QUERY, Answer, attempt, bearer, json_content, user_agent};

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
    R: FnMut() -> Result<(), WireError>,
{
    let body = serde_json::to_vec(requested).map_err(|_| WireError::Encode)?;
    let request = SessionRequest {
        method: Method::Post,
        path: acquire_path(scale_set_id),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content(), bearer(queue_token)?, user_agent()],
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
