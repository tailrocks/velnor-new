//! Read counts from one fresh, normal controller poll.

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

/// Sanitized counts from one completed controller poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionObservation {
    /// Number of `JobAvailable` events in this response, not queue demand.
    pub available_events: u32,
    /// Scale Set counters, absent when the response omitted them.
    pub statistics: Option<SessionObservationStatistics>,
    /// Unix timestamp in milliseconds when the response completed.
    pub observed_at_unix_ms: u64,
}

/// Scalar counters from the Scale Set response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionObservationStatistics {
    /// Jobs reported available by the service.
    pub available_jobs: i64,
    /// Jobs acquired by the service.
    pub acquired_jobs: i64,
    /// Assigned job population reported by the service.
    pub assigned_jobs: i64,
    /// Running jobs reported by the service.
    pub running_jobs: i64,
    /// Registered runners reported by the service.
    pub registered_runners: i64,
    /// Busy runners reported by the service.
    pub busy_runners: i64,
    /// Idle runners reported by the service.
    pub idle_runners: i64,
}

/// Setup identity and the fresh poll observation captured for one connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionCensus {
    /// Setup groups and selected set from one controller-owned setup read.
    pub setup: ConnectionSetupReceipt,
    /// Counts from the next settled ordinary poll after this request.
    pub observation: SessionObservation,
}

/// Wait for the next ordinary poll for one configured connection.
///
/// The controller does not create a session or perform another poll. The
/// returned event count describes that one response only.
///
/// # Errors
///
/// Returns an error when the connection, controller response, observation,
/// or bounded wait is invalid or unavailable.
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

/// Wait for the current setup snapshot and the next settled ordinary poll.
///
/// The controller reuses its existing setup and poll work; this call does not
/// create a session, force a poll, or issue another remote setup request.
///
/// # Errors
///
/// Returns an error when the connection, setup, poll, response, or bounded
/// wait is invalid, stale, or unavailable.
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use crate::config::ConnectionId;
    use crate::controller_ipc::wire::{SessionObservationReply, SessionObservationStatus};
    use crate::error::HostError;
    use crate::launch::steady::{PollObservation, PollStatistics};

    use super::{completed, nonce, wait_for_connection_census, wait_for_session_observation};

    #[test]
    fn generated_request_nonce_is_bounded_lower_hex() -> Result<(), String> {
        let nonce = nonce().map_err(|error| error.to_string())?;
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("client nonce was not 32 lowercase hexadecimal bytes".to_owned());
        }
        Ok(())
    }

    #[test]
    fn zero_timeout_is_rejected_before_socket_or_config_access() {
        let connection = ConnectionId::parse("example").expect("valid test connection");
        assert_eq!(
            wait_for_session_observation(
                Path::new("/state-must-not-be-read"),
                &connection,
                Duration::ZERO,
            ),
            Err(HostError::Frame)
        );
    }

    #[test]
    fn zero_census_timeout_is_rejected_before_config_or_socket_access() {
        let connection = ConnectionId::parse("example").expect("valid test connection");
        assert_eq!(
            wait_for_connection_census(
                Path::new("/state-must-not-be-read"),
                &connection,
                Duration::ZERO,
            ),
            Err(HostError::Frame)
        );
    }

    #[test]
    fn complete_batch_preserves_every_service_counter() {
        let observation = completed(&SessionObservationReply {
            status: SessionObservationStatus::Complete,
            poll: Some(PollObservation::Complete {
                available_events: 2,
                statistics: Some(PollStatistics {
                    available_jobs: 11,
                    acquired_jobs: 12,
                    assigned_jobs: 13,
                    running_jobs: 14,
                    registered_runners: 15,
                    busy_runners: 16,
                    idle_runners: 17,
                }),
            }),
            observed_at_unix_ms: Some(18),
        })
        .expect("complete response decodes");
        assert_eq!(observation.available_events, 2);
        assert_eq!(observation.observed_at_unix_ms, 18);
        assert_eq!(
            observation.statistics,
            Some(super::super::SessionObservationStatistics {
                available_jobs: 11,
                acquired_jobs: 12,
                assigned_jobs: 13,
                running_jobs: 14,
                registered_runners: 15,
                busy_runners: 16,
                idle_runners: 17,
            })
        );
    }

    #[test]
    fn empty_poll_remains_zero_events_with_absent_statistics() {
        let observation = completed(&SessionObservationReply {
            status: SessionObservationStatus::Complete,
            poll: Some(PollObservation::Complete {
                available_events: 0,
                statistics: None,
            }),
            observed_at_unix_ms: Some(18),
        })
        .expect("empty poll is a completed response");
        assert_eq!(observation.available_events, 0);
        assert_eq!(observation.statistics, None);
    }
}
