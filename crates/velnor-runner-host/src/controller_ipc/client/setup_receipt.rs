// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::ConnectionId;
use crate::error::HostError;

use super::super::wire::{
    ConnectionCensusReply, ConnectionSetupSnapshot, Request, Response, SessionObservationStatus,
    SetupReceiptReply, SetupReceiptStatus,
};
use super::session_observation::{self, ConnectionCensus};
use super::{CALL_TIMEOUT, RETRY, SOCKET_NAME, call_socket_response_with_timeout, read_connection};

const MAX_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerGroupReceipt {
    pub id: i64,

    pub name: String,

    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedSetReceipt {
    pub id: i64,

    pub name: String,

    pub runner_group_id: i64,

    pub disable_update: bool,

    pub labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSetupReceipt {
    pub revision: u64,

    pub captured_at_unix_ms: u64,

    pub runner_groups: Vec<RunnerGroupReceipt>,

    pub selected_set: SelectedSetReceipt,
}

pub fn wait_for_connection_setup_receipt(
    state: &Path,
    connection_id: &ConnectionId,
    timeout: Duration,
) -> Result<ConnectionSetupReceipt, HostError> {
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
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HostError::Docker);
        }
        let request = Request::ConnectionSetupStatus {
            connection_id: connection_id.clone(),
        };
        if !super::valid_request(&request) {
            return Err(HostError::Frame);
        }
        let path = state.join(SOCKET_NAME);
        let response =
            call_socket_response_with_timeout(&path, &request, remaining.min(CALL_TIMEOUT))?;
        let reply = setup_reply(response)?;
        match reply.status {
            SetupReceiptStatus::Ready => {
                return reply
                    .setup
                    .map(ConnectionSetupReceipt::from_snapshot)
                    .ok_or(HostError::Journal);
            }
            SetupReceiptStatus::Pending => {
                thread::sleep(RETRY.min(deadline.saturating_duration_since(Instant::now())));
            }
            SetupReceiptStatus::Stale => return Err(HostError::Identity),
        }
    }
}

fn setup_reply(response: Response) -> Result<SetupReceiptReply, HostError> {
    response.setup_receipt.ok_or(HostError::Journal)
}

pub(super) fn wait_for_census_replies(
    state: &Path,
    connection_id: &ConnectionId,
    nonce: &str,
    deadline: Instant,
) -> Result<ConnectionCensus, HostError> {
    let mut start = true;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HostError::Docker);
        }
        let request = census_request(connection_id, nonce, remaining, start)?;
        let reply = census_call(state, &request, remaining.min(CALL_TIMEOUT))?;
        start = reply_requires_start(&reply);
        if let Some(census) = completed_census(reply)? {
            return Ok(census);
        }
        thread::sleep(RETRY.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn census_request(
    connection_id: &ConnectionId,
    nonce: &str,
    remaining: Duration,
    start: bool,
) -> Result<Request, HostError> {
    if start {
        let timeout_ms = u64::try_from(remaining.as_millis())
            .map_err(|_| HostError::Frame)?
            .max(1);
        Ok(Request::ConnectionCensusStart {
            connection_id: connection_id.clone(),
            nonce: nonce.to_owned(),
            timeout_ms,
        })
    } else {
        Ok(Request::ConnectionCensusStatus {
            connection_id: connection_id.clone(),
            nonce: nonce.to_owned(),
        })
    }
}

fn census_call(
    state: &Path,
    request: &Request,
    timeout: Duration,
) -> Result<ConnectionCensusReply, HostError> {
    if !super::valid_request(request) {
        return Err(HostError::Frame);
    }
    let response = call_socket_response_with_timeout(&state.join(SOCKET_NAME), request, timeout)?;
    response.connection_census.ok_or(HostError::Journal)
}

fn completed_census(reply: ConnectionCensusReply) -> Result<Option<ConnectionCensus>, HostError> {
    if reply.setup_status == SetupReceiptStatus::Stale {
        return Err(HostError::Identity);
    }
    match reply.observation.status {
        SessionObservationStatus::Complete => {
            let setup = reply.setup.ok_or(HostError::Journal)?;
            let observation = session_observation::completed(&reply.observation)?;
            Ok(Some(ConnectionCensus {
                setup: ConnectionSetupReceipt::from_snapshot(setup),
                observation,
            }))
        }
        SessionObservationStatus::Pending | SessionObservationStatus::Missing => Ok(None),
        status => Err(session_observation::status_error(status)),
    }
}

fn reply_requires_start(reply: &ConnectionCensusReply) -> bool {
    reply.setup_status == SetupReceiptStatus::Pending
        || reply.observation.status == SessionObservationStatus::Missing
}

impl ConnectionSetupReceipt {
    pub(super) fn from_snapshot(snapshot: ConnectionSetupSnapshot) -> Self {
        Self {
            revision: snapshot.revision,
            captured_at_unix_ms: snapshot.captured_at_unix_ms,
            runner_groups: snapshot
                .runner_groups
                .into_iter()
                .map(|group| RunnerGroupReceipt {
                    id: group.id,
                    name: group.name,
                    is_default: group.is_default,
                })
                .collect(),
            selected_set: SelectedSetReceipt {
                id: snapshot.selected_set.id,
                name: snapshot.selected_set.name,
                runner_group_id: snapshot.selected_set.runner_group_id,
                disable_update: snapshot.selected_set.disable_update,
                labels: snapshot.selected_set.labels,
            },
        }
    }
}
