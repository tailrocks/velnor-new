use std::cell::RefCell;

use super::*;

struct Status {
    answers: RefCell<Vec<u16>>,
    calls: RefCell<Vec<String>>,
}

impl StatusBackend for Status {
    fn status(&self, method: &str, route: &str) -> Result<u16, OrchestratorError> {
        self.calls.borrow_mut().push(format!("{method} {route}"));
        Ok(self.answers.borrow_mut().remove(0))
    }
}

fn revoke_with(answers: &[u16]) -> (Result<(), OrchestratorError>, Vec<String>) {
    let backend = Status {
        answers: RefCell::new(answers.to_vec()),
        calls: RefCell::new(Vec::new()),
    };
    let result = revoke(&backend);
    (result, backend.calls.into_inner())
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
    let still_valid = revoke_with(&[204, 200]).0.expect_err("still valid");
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
