//! Report readiness refresh failures to the blocked poll test.

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use velnor_runner_github::{Exchange, SessionRequest, Transport, TransportFail};

use crate::{controller_ipc::ControlState, error::HostError};

pub(super) struct BlockingPoll {
    pub(super) ready: mpsc::Receiver<Result<u64, String>>,
    pub(super) poll_entered: Option<mpsc::Sender<Instant>>,
    pub(super) poll_started: Instant,
    pub(super) diagnostic: Option<String>,
    pub(super) control: ControlState,
    pub(super) config_digest: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) journal_instance: String,
    pub(super) expected_capacity: u32,
}

#[derive(Clone, Copy)]
pub(super) struct SampleTiming {
    pub(super) started_after_poll: Duration,
    pub(super) finished_after_poll: Duration,
}

pub(super) struct RefreshFailure {
    pub(super) stage: &'static str,
    pub(super) error: HostError,
    pub(super) detail: Option<String>,
    pub(super) sample_timing: SampleTiming,
}

pub(super) fn refresh_failure(
    stage: &'static str,
    error: HostError,
    detail: Option<String>,
    sample_timing: SampleTiming,
) -> RefreshFailure {
    RefreshFailure {
        stage,
        error,
        detail,
        sample_timing,
    }
}

impl RefreshFailure {
    pub(super) fn diagnostic(&self) -> String {
        format!(
            "Readiness stage {} failed; sample_started_after_poll={:?}, sample_finished_after_poll={:?}; error={}; detail={:?}.",
            self.stage,
            self.sample_timing.started_after_poll,
            self.sample_timing.finished_after_poll,
            self.error,
            self.detail
        )
    }
}

impl Transport for BlockingPoll {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        if !request.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case(velnor_runner_github::CAPACITY_HEADER)
                && value == &self.expected_capacity.to_string()
        }) {
            return Err(TransportFail::Reset);
        }
        self.poll_entered
            .take()
            .ok_or(TransportFail::Reset)?
            .send(Instant::now())
            .map_err(|_| TransportFail::Reset)?;
        let revision = match self.ready.recv_timeout(Duration::from_secs(12)) {
            Ok(Ok(revision)) => revision,
            Ok(Err(error)) => {
                self.diagnostic = Some(format!(
                    "The readiness refresh returned an error. Error: {error}."
                ));
                return Err(TransportFail::Reset);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.diagnostic = Some(format!(
                    "The readiness result consumer reached its 12-second timeout after {:?}.",
                    self.poll_started.elapsed()
                ));
                return Err(TransportFail::Reset);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.diagnostic = Some("The readiness result channel disconnected.".to_owned());
                return Err(TransportFail::Reset);
            }
        };
        let pending = match self.control.complete_session_poll_receipts() {
            Ok(receipts) => receipts.is_some(),
            Err(error) => {
                self.diagnostic = Some(format!(
                    "The test could not read the session poll receipt. Error: {error}."
                ));
                return Err(TransportFail::Reset);
            }
        };
        let ready = match self.control.readiness_matches(
            &self.config_digest,
            &self.engine_id,
            &self.docker_root_digest,
            &self.journal_instance,
            revision,
            std::time::Instant::now(),
        ) {
            Ok(ready) => ready,
            Err(error) => {
                self.diagnostic = Some(format!(
                    "The test could not check the readiness proof. Error: {error}."
                ));
                return Err(TransportFail::Reset);
            }
        };
        if !pending || !ready {
            self.diagnostic = Some(format!(
                "The test received the readiness result after {:?}. Its proof check returned pending_receipt={pending}, ready={ready}, revision={revision}.",
                self.poll_started.elapsed()
            ));
            return Err(TransportFail::Reset);
        }
        Ok(Exchange {
            status: 202,
            body: Vec::new(),
        })
    }
}
