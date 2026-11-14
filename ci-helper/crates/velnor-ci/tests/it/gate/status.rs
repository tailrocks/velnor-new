// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::github::statuses_path;

use super::{BOTH_PASSED, GREEN, Recorded, VERDICT, absent, both, check, verdict};
use crate::support::repo;

fn with_status(recorded: Recorded, context: &str, state: &str) -> Recorded {
    let statuses = json!({"state": state, "sha": GREEN, "total_count": 1, "statuses": [{"context": context, "state": state}]});
    recorded.set(statuses_path(&repo(), GREEN, 1), statuses)
}

#[test]
fn a_commit_status_named_like_a_required_check_must_pass_too_and_never_stands_in_for_a_job() {
    for (context, state) in [
        (VERDICT, "failure"),
        (VERDICT, "error"),
        ("required verdict", "failure"),
        ("REQUIRED VERDICT", "error"),
    ] {
        assert_eq!(
            with_status(both(), context, state).gate(GREEN),
            Err(format!(
                "check Required verdict did not pass on commit {GREEN}: its commit status is {state}"
            )),
            "{context}"
        );
    }
    let pending = "check Required passed\ncheck Required verdict has a pending commit status\n";
    assert_eq!(
        with_status(both(), VERDICT, "pending").gate(GREEN),
        Ok(verdict(false, GREEN, pending))
    );
    for (context, state) in [(VERDICT, "success"), ("lint", "failure")] {
        let said = with_status(both(), context, state).gate(GREEN);
        assert_eq!(
            said,
            Ok(verdict(true, GREEN, BOTH_PASSED)),
            "{context} {state}"
        );
    }
    let dco_status_only = Recorded::new().requiring(&[check("Required"), check("DCO")]);
    let said = with_status(dco_status_only, "DCO", "success").gate(GREEN);
    assert_eq!(said, Err(absent("DCO")));
}
