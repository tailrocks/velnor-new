//! Read the latest setup snapshot produced by the active controller.

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

/// One ordered runner-group row from the controller's setup read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerGroupReceipt {
    /// Service identifier.
    pub id: i64,
    /// Display name returned by the service.
    pub name: String,
    /// Whether this is the repository's default group.
    pub is_default: bool,
}

/// The selected Scale Set returned by the same setup read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedSetReceipt {
    /// Service identifier.
    pub id: i64,
    /// Configured selector name.
    pub name: String,
    /// Runner group that owns this Scale Set.
    pub runner_group_id: i64,
    /// Whether registration updates are disabled.
    pub disable_update: bool,
    /// Labels in service order.
    pub labels: Vec<String>,
}

/// Cached setup facts captured together by the active controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSetupReceipt {
    /// Monotonic in-controller snapshot revision.
    pub revision: u64,
    /// Unix timestamp in milliseconds when the controller captured the snapshot.
    pub captured_at_unix_ms: u64,
    /// Runner groups in the order returned by the setup read.
    pub runner_groups: Vec<RunnerGroupReceipt>,
    /// The selected Scale Set from that same read.
    pub selected_set: SelectedSetReceipt,
}

/// Wait for the controller's existing setup read for one configured connection.
///
/// This request only reads the cached result. It does not create a session or
/// call the remote setup API.
///
/// # Errors
///
/// Returns an error when the connection is absent, the controller reports a
/// stale setup, the response is malformed, or the bounded wait expires.
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use crate::config::ConnectionId;
    use crate::error::HostError;

    use super::wait_for_connection_setup_receipt;

    #[test]
    fn zero_timeout_is_rejected_before_config_or_socket_access() {
        let connection = ConnectionId::parse("example").expect("valid test connection");
        assert_eq!(
            wait_for_connection_setup_receipt(
                Path::new("/state-must-not-be-read"),
                &connection,
                Duration::ZERO,
            ),
            Err(HostError::Frame)
        );
    }
}
