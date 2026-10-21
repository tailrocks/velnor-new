use std::cell::RefCell;

use super::*;

struct Status {
    answers: RefCell<Vec<u16>>,
    calls: RefCell<Vec<String>>,
    pauses: RefCell<Vec<Duration>>,
}

impl StatusBackend for Status {
    fn status(&self, method: &str, route: &str) -> Result<u16, OrchestratorError> {
        self.calls.borrow_mut().push(format!("{method} {route}"));
        Ok(self.answers.borrow_mut().remove(0))
    }

    fn pause(&self, duration: Duration) {
        self.pauses.borrow_mut().push(duration);
    }
}

fn revoke_with(answers: &[u16]) -> (Result<(), OrchestratorError>, Vec<String>) {
    let (result, calls, _) = revoke_timed(answers);
    (result, calls)
}

fn revoke_timed(answers: &[u16]) -> (Result<(), OrchestratorError>, Vec<String>, Vec<Duration>) {
    let backend = Status {
        answers: RefCell::new(answers.to_vec()),
        calls: RefCell::new(Vec::new()),
        pauses: RefCell::new(Vec::new()),
    };
    let result = revoke(&backend);
    (
        result,
        backend.calls.into_inner(),
        backend.pauses.into_inner(),
    )
}

#[test]
fn revoke_requires_the_token_to_be_dead_afterwards() {
    let (ok, calls) = revoke_with(&[204, 401]);
    assert!(ok.is_ok());
    assert_eq!(
        calls,
        ["DELETE installation/token", "GET installation/repositories"]
    );
    assert!(
        revoke_with(&[401, 401]).0.is_ok(),
        "already expired counts as revoked"
    );
    let still_valid = revoke_with(&[204, 200, 200, 200, 200, 200, 200, 200, 200])
        .0
        .expect_err("still valid");
    assert!(
        still_valid
            .to_string()
            .contains("private_token_still_valid:200")
    );
    let refused = revoke_with(&[500]).0.expect_err("refused");
    assert!(
        refused
            .to_string()
            .contains("private_token_revoke_refused:500")
    );
    let forbidden = revoke_with(&[403]).0.expect_err("forbidden");
    assert!(
        forbidden
            .to_string()
            .contains("private_token_revoke_refused:403")
    );
}

#[test]
fn a_token_that_dies_after_the_first_probe_counts_as_revoked() {
    let (result, calls, pauses) = revoke_timed(&[204, 200, 401]);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        calls,
        [
            "DELETE installation/token",
            "GET installation/repositories",
            "GET installation/repositories",
        ]
    );
    assert_eq!(pauses, [Duration::from_millis(500)]);
}

#[test]
fn the_probe_budget_is_bounded_and_fails_closed_on_the_last_answer() {
    let (result, calls, pauses) = revoke_timed(&[204, 200, 200, 200, 200, 200, 200, 200, 403]);
    let err = result.expect_err("still authenticates");
    assert!(err.to_string().contains("private_token_still_valid:403"));
    assert_eq!(calls.len(), 1 + PROBE_BACKOFF_MS.len() + 1);
    assert_eq!(pauses.len(), PROBE_BACKOFF_MS.len());
    assert!(pauses.iter().sum::<Duration>() <= Duration::from_secs(30));
}
