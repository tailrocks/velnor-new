// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{OrchestratorError, poll_pending_admission};
use std::cell::Cell;

#[test]
fn retries_only_explicitly_pending_required_checks() {
    let calls = Cell::new(0);
    let waits = Cell::new(0);
    let result = poll_pending_admission(
        || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(OrchestratorError::Internal {
                    problem: "crate_release_required_pending".into(),
                })
            } else {
                Ok(7)
            }
        },
        || true,
        || waits.set(waits.get() + 1),
    );
    assert_eq!(result.expect("success"), 7);
    assert_eq!(waits.get(), 2);
    let calls = Cell::new(0);
    let result: Result<(), _> = poll_pending_admission(
        || {
            calls.set(calls.get() + 1);
            Err(OrchestratorError::Internal {
                problem: "crate_release_check_app".into(),
            })
        },
        || true,
        || panic!("permanent error was retried"),
    );
    assert!(result.is_err());
    assert_eq!(calls.get(), 1);
    let result: Result<(), _> = poll_pending_admission(
        || {
            Err(OrchestratorError::Internal {
                problem: "crate_release_required_pending".into(),
            })
        },
        || false,
        || panic!("expired deadline was retried"),
    );
    assert!(result.is_err());
}
