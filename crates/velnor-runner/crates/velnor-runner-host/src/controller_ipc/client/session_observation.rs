// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::ConnectionId;
use crate::error::HostError;
use crate::launch::steady::{PollObservation, PollStatistics};

use super::super::wire::{Request, Response, SessionObservationReply, SessionObservationStatus};
use super::setup_receipt::ConnectionSetupReceipt;
use super::setup_receipt::wait_for_census_replies;
use super::{CALL_TIMEOUT, HEX, RETRY, call_socket_response_with_timeout, read_connection};

const MAX_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionObservation {
    pub available_events: u32,

    pub statistics: Option<SessionObservationStatistics>,

    pub observed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionObservationStatistics {
    pub available_jobs: i64,

    pub acquired_jobs: i64,

    pub assigned_jobs: i64,

    pub running_jobs: i64,

    pub registered_runners: i64,

    pub busy_runners: i64,

    pub idle_runners: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionCensus {
    pub setup: ConnectionSetupReceipt,

    pub observation: SessionObservation,
}

pub fn wait_for_session_observation(
    state: &Path,
    connection_id: &ConnectionId,
    timeout: Duration,
) -> Result<SessionObservation, HostError> {
    if timeout.is_zero() {
        return Err(HostError::Frame);
    }
    if read_connection(state, connection_id)?.is_none() {
        return Err(HostError::Config);
    }
    let timeout = timeout.min(MAX_WAIT);
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(HostError::Identity)?;
    let nonce = nonce()?;
    let timeout_ms = u64::try_from(timeout.as_millis()).map_err(|_| HostError::Frame)?;
    let start = Request::SessionObservationStart {
        connection_id: connection_id.clone(),
        nonce: nonce.clone(),
        timeout_ms,
    };
    let response = call(state, &start, timeout.min(CALL_TIMEOUT))?;
    match observation_reply(response)?.status {
        SessionObservationStatus::Pending | SessionObservationStatus::Complete => {}
        status => return Err(status_error(status)),
    }
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HostError::Docker);
        }
        let status = Request::SessionObservationStatus {
            connection_id: connection_id.clone(),
            nonce: nonce.clone(),
        };
        let response = call(state, &status, remaining.min(CALL_TIMEOUT))?;
        let reply = observation_reply(response)?;
        match reply.status {
            SessionObservationStatus::Complete => return completed(&reply),
            SessionObservationStatus::Pending => {
                thread::sleep(RETRY.min(deadline.saturating_duration_since(Instant::now())));
            }
            status => return Err(status_error(status)),
        }
    }
}

pub fn wait_for_connection_census(
    state: &Path,
    connection_id: &ConnectionId,
    timeout: Duration,
) -> Result<ConnectionCensus, HostError> {
    if timeout.is_zero() {
        return Err(HostError::Frame);
    }
    if read_connection(state, connection_id)?.is_none() {
        return Err(HostError::Config);
    }
    let timeout = timeout.min(MAX_WAIT);
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(HostError::Identity)?;
    let nonce = nonce()?;
    wait_for_census_replies(state, connection_id, &nonce, deadline)
}

fn call(state: &Path, request: &Request, timeout: Duration) -> Result<Response, HostError> {
    if !super::valid_request(request) {
        return Err(HostError::Frame);
    }
    call_socket_response_with_timeout(
        &state.join(super::SOCKET_NAME),
        request,
        timeout.max(Duration::from_millis(1)),
    )
}

fn observation_reply(response: Response) -> Result<SessionObservationReply, HostError> {
    response.session_observation.ok_or(HostError::Journal)
}

pub(super) fn completed(reply: &SessionObservationReply) -> Result<SessionObservation, HostError> {
    let Some(observed_at_unix_ms) = reply.observed_at_unix_ms else {
        return Err(HostError::Journal);
    };
    let Some(PollObservation::Complete {
        available_events,
        statistics,
    }) = reply.poll.as_ref()
    else {
        return Err(HostError::Identity);
    };
    Ok(SessionObservation {
        available_events: *available_events,
        statistics: statistics.as_ref().copied().map(public_statistics),
        observed_at_unix_ms,
    })
}

fn public_statistics(stats: PollStatistics) -> SessionObservationStatistics {
    SessionObservationStatistics {
        available_jobs: stats.available_jobs,
        acquired_jobs: stats.acquired_jobs,
        assigned_jobs: stats.assigned_jobs,
        running_jobs: stats.running_jobs,
        registered_runners: stats.registered_runners,
        busy_runners: stats.busy_runners,
        idle_runners: stats.idle_runners,
    }
}

pub(super) fn status_error(status: SessionObservationStatus) -> HostError {
    match status {
        SessionObservationStatus::Busy => HostError::Ownership,
        SessionObservationStatus::TimedOut => HostError::Docker,
        SessionObservationStatus::Stale | SessionObservationStatus::Unavailable => {
            HostError::Identity
        }
        SessionObservationStatus::Missing => HostError::Journal,
        SessionObservationStatus::Pending | SessionObservationStatus::Complete => {
            HostError::Journal
        }
    }
}

pub(super) fn nonce() -> Result<String, HostError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| HostError::Identity)?;
    Ok(bytes
        .iter()
        .flat_map(|byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]])
        .map(char::from)
        .collect())
}
