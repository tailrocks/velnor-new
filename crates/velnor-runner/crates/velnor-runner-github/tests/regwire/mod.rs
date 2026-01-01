//! Scripted transport shared by registration wire tests.

use std::collections::VecDeque;

use velnor_runner_github::{Exchange, SessionError, SessionRequest, Transport, TransportFail};

pub(super) struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    pub(super) seen: Vec<SessionRequest>,
}

impl Script {
    pub(super) fn once(status: u16, body: &str) -> Self {
        Self::replies(vec![Ok(exchange(status, body))])
    }

    pub(super) fn replies(replies: Vec<Result<Exchange, TransportFail>>) -> Self {
        Self {
            replies: VecDeque::from(replies),
            seen: Vec::new(),
        }
    }
}

pub(super) fn exchange(status: u16, body: &str) -> Exchange {
    Exchange {
        status,
        body: body.as_bytes().to_vec(),
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.replies
            .pop_front()
            .unwrap_or(Err(TransportFail::Reset))
    }
}

pub(super) fn header<'a>(request: &'a SessionRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

pub(super) fn show<T>(result: Result<T, SessionError>) -> Result<T, String> {
    result.map_err(|err| format!("{err:?}"))
}
