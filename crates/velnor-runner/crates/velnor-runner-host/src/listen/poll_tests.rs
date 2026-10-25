use std::collections::VecDeque;

use velnor_runner_github::{Exchange, Method, Poll, SessionRequest, Transport, TransportFail};

use super::{PollPathResult, poll_path_with};

struct Script {
    responses: VecDeque<Result<Exchange, TransportFail>>,
    requests: Vec<SessionRequest>,
}

impl Script {
    fn new(responses: impl IntoIterator<Item = Result<Exchange, TransportFail>>) -> Self {
        Self {
            responses: responses.into_iter().collect(),
            requests: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push(request.clone());
        self.responses
            .pop_front()
            .unwrap_or(Err(TransportFail::Reset))
    }
}

fn authorized(request: &SessionRequest, expected: &str) -> bool {
    request
        .headers
        .iter()
        .any(|(name, value)| name.eq_ignore_ascii_case("authorization") && value == expected)
}

#[test]
fn unauthorized_queue_request_is_not_replayed_with_old_bearer() {
    let mut script = Script::new([Ok(Exchange {
        status: 401,
        body: Vec::new(),
    })]);

    let result = poll_path_with(&mut script, "old-queue-token", "old/messages", 0, 0);

    assert_eq!(result, Ok(PollPathResult::RefreshRequired));
    assert_eq!(script.requests.len(), 1);
    assert_eq!(script.requests[0].method, Method::Get);
    assert_eq!(script.requests[0].path, "old/messages");
    assert!(authorized(&script.requests[0], "Bearer old-queue-token"));
}

#[test]
fn rebuilt_queue_request_uses_new_path_and_bearer() {
    let mut script = Script::new([
        Ok(Exchange {
            status: 401,
            body: Vec::new(),
        }),
        Ok(Exchange {
            status: 202,
            body: Vec::new(),
        }),
    ]);

    let first = poll_path_with(&mut script, "old-queue-token", "old/messages", 0, 0);
    assert_eq!(first, Ok(PollPathResult::RefreshRequired));

    let second = poll_path_with(&mut script, "new-queue-token", "new/messages", 0, 0);

    assert_eq!(second, Ok(PollPathResult::Response(Poll::Empty)));
    assert_eq!(script.requests.len(), 2);
    assert_eq!(script.requests[0].method, Method::Get);
    assert_eq!(script.requests[0].path, "old/messages");
    assert!(authorized(&script.requests[0], "Bearer old-queue-token"));
    assert_eq!(script.requests[1].method, Method::Get);
    assert_eq!(script.requests[1].path, "new/messages");
    assert!(authorized(&script.requests[1], "Bearer new-queue-token"));
}
