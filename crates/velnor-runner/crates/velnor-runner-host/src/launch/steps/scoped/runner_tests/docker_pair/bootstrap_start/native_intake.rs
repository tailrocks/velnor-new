//! Save the poll messages before the scoped `AcquireJobs` call.

use super::*;

use crate::journal::HeaderAllocation;
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

pub(in crate::launch::steps::scoped::runner::tests::docker_pair) const NATIVE_COMPOSITION_JOB_ID:
    &str = "native-intake-composition-job";

pub(in crate::launch::steps::scoped::runner::tests::docker_pair) struct PollSession {
    pub(super) intent_id: i64,
    pub(super) allocation: HeaderAllocation,
    acquire_calls: std::rc::Rc<std::cell::Cell<usize>>,
    acknowledgement_calls: std::rc::Rc<std::cell::Cell<usize>>,
}

impl PollSession {
    pub(in crate::launch::steps::scoped::runner::tests::docker_pair) fn acquire_calls(
        &self,
    ) -> usize {
        self.acquire_calls.get()
    }

    pub(in crate::launch::steps::scoped::runner::tests::docker_pair) fn acknowledgement_calls(
        &self,
    ) -> usize {
        self.acknowledgement_calls.get()
    }
}

#[path = "native_intake/acquisition.rs"]
mod acquisition;
pub(in crate::launch::steps::scoped::runner::tests::docker_pair) use acquisition::{
    authorize_actual_started_request, open_composition_with_poll,
};

#[path = "native_intake/replay.rs"]
mod replay;
pub(in crate::launch::steps::scoped::runner::tests::docker_pair) use replay::replay_owned_terminal_offer;

struct SessionResponse;

impl Transport for SessionResponse {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Post);
        Ok(Exchange {
            status: 200,
            body: br#"{"sessionId":"native-intake-session","messageQueueUrl":"https://queue.example.invalid/","messageQueueAccessToken":"queue-token"}"#.to_vec(),
        })
    }
}
