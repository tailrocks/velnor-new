// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::refresh::{RefreshGate, StatusClass};
use crate::{Poll, WireError, parse_poll};

use super::error::{SessionError, reject};
use super::request::{Method, SessionRequest, Transport};
use super::retry::{accept_preview, attempt, bearer, capacity_pair, poll_query, user_agent};

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
    R: FnMut() -> Result<(), WireError>,
{
    let request = SessionRequest {
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
    let answer = attempt(transport, &request, gate, refresh)?;
    match answer.class {
        StatusClass::EmptyPoll => Ok(Poll::Empty),
        StatusClass::Ok => parse_body(answer.status, answer.body()),
        other => Err(reject(other)),
    }
}

fn parse_body(status: u16, body: &[u8]) -> Result<Poll, SessionError> {
    let text = std::str::from_utf8(body).map_err(|_| WireError::Malformed)?;
    parse_poll(status, text).map_err(SessionError::from)
}
