//! Regressions for the same start path used by `Turn::start`.

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail, create_session,
};

use super::Ready;

const INITIAL_SESSION: &[u8] = br#"{"sessionId":"session","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token","statistics":{"totalAvailableJobs":0,"totalAcquiredJobs":0,"totalAssignedJobs":0,"totalRunningJobs":0,"totalRegisteredRunners":0,"totalBusyRunners":0,"totalIdleRunners":0}}"#;

struct InitialSession;

impl Transport for InitialSession {
    fn exchange(&mut self, _request: &SessionRequest) -> Result<Exchange, TransportFail> {
        Ok(Exchange {
            status: 200,
            body: INITIAL_SESSION.to_vec(),
        })
    }
}

pub(super) fn zero_assignment_session() -> Result<QueueSession, String> {
    create_session(&mut InitialSession, 1, "owner", "admin-token")
        .map_err(|error| error.to_string())
}

pub(super) fn ready<'a>(session: &'a QueueSession, polled: &'a Poll) -> Ready<'a> {
    Ready {
        set_id: 1,
        session,
        admin_token: "admin-token",
        path: "messages".to_owned(),
        polled,
    }
}

#[cfg(all(test, unix))]
mod legacy_failed;
#[cfg(all(test, unix))]
mod progress_tests;
mod pump_tests;
#[cfg(all(test, unix))]
mod start_tests;
