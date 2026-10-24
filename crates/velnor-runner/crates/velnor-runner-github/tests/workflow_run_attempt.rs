//! Focused tests for GitHub workflow source records.

use std::collections::VecDeque;

use serde_json::json;
use velnor_runner_github::workflow_run::{WorkflowRunError, get_workflow_run_attempt};
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Script(VecDeque<Result<Exchange, TransportFail>>);

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.path, "/repos/owner/repo/actions/runs/17/attempts/3");
        self.0.pop_front().unwrap_or(Err(TransportFail::Reset))
    }
}

fn response(id: i64, attempt: i64) -> Exchange {
    let body = json!({
        "id": id,
        "event": "push",
        "path": ".github/workflows/ci.yml",
        "head_sha": SHA,
        "workflow_id": 4,
        "run_attempt": attempt,
        "repository": {
            "id": 5,
            "name": "repo",
            "full_name": "owner/repo",
            "owner": {"login": "owner"}
        }
    })
    .to_string()
    .into_bytes();
    Exchange { status: 200, body }
}

#[test]
fn reads_exact_attempt_route_and_checks_both_ids() {
    let mut script = Script(VecDeque::from([Ok(response(17, 3))]));
    let run = get_workflow_run_attempt(&mut script, "owner", "repo", 17, 3, "token")
        .expect("exact attempt");
    assert_eq!((run.id, run.run_attempt), (17, 3));
}

#[test]
fn rejects_wrong_run_or_attempt() {
    for result in [response(18, 3), response(17, 2)] {
        let mut script = Script(VecDeque::from([Ok(result)]));
        assert_eq!(
            get_workflow_run_attempt(&mut script, "owner", "repo", 17, 3, "token"),
            Err(WorkflowRunError::Malformed)
        );
    }
}

#[test]
fn rejects_invalid_inputs_bad_responses_and_uncertain_transport() {
    let mut unused = Script(VecDeque::new());
    assert_eq!(
        get_workflow_run_attempt(&mut unused, "owner", "repo", 0, 3, "token"),
        Err(WorkflowRunError::InvalidRequest)
    );
    let mut malformed = Script(VecDeque::from([Ok(Exchange {
        status: 200,
        body: b"{}".to_vec(),
    })]));
    assert_eq!(
        get_workflow_run_attempt(&mut malformed, "owner", "repo", 17, 3, "token"),
        Err(WorkflowRunError::Malformed)
    );
    let mut timeout = Script(VecDeque::from([Err(TransportFail::Timeout)]));
    assert_eq!(
        get_workflow_run_attempt(&mut timeout, "owner", "repo", 17, 3, "token"),
        Err(WorkflowRunError::Uncertain)
    );
    let mut reset = Script(VecDeque::from([Err(TransportFail::Reset)]));
    assert_eq!(
        get_workflow_run_attempt(&mut reset, "owner", "repo", 17, 3, "token"),
        Err(WorkflowRunError::Uncertain)
    );
    let mut oversized = Script(VecDeque::from([Ok(Exchange {
        status: 200,
        body: vec![b' '; 1024 * 1024 + 1],
    })]));
    assert_eq!(
        get_workflow_run_attempt(&mut oversized, "owner", "repo", 17, 3, "token"),
        Err(WorkflowRunError::ResponseTooLarge)
    );
    let mut denied = Script(VecDeque::from([Ok(Exchange {
        status: 403,
        body: Vec::new(),
    })]));
    assert_eq!(
        get_workflow_run_attempt(&mut denied, "owner", "repo", 17, 3, "token"),
        Err(WorkflowRunError::HttpStatus(403))
    );
}
