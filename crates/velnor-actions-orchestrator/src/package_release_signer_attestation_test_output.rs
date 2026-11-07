// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

thread_local! {
    static TEST_VERIFIED_OUTPUT: std::cell::RefCell<std::collections::VecDeque<Vec<u8>>> = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

pub(crate) fn inject_verified_output_for_test(output: Vec<u8>) {
    TEST_VERIFIED_OUTPUT.with(|slot| {
        slot.borrow_mut().push_back(output);
    });
}

pub(crate) fn assert_test_verified_outputs_consumed() {
    TEST_VERIFIED_OUTPUT
        .with(|slot| assert!(slot.borrow().is_empty(), "unused injected verifier output"));
}

mod injected_output_tests {
    use super::{
        super::{AttestationCommandIdentity, verify_one},
        TEST_VERIFIED_OUTPUT,
    };
    use std::{path::Path, time::Duration};
    use velnor_actions_mise::CheckDeadline;

    #[test]
    fn injected_verifier_output_is_consumed_once_on_its_test_thread() {
        TEST_VERIFIED_OUTPUT.with(|slot| slot.borrow_mut().push_back(b"fixture".to_vec()));
        let identity = AttestationCommandIdentity {
            repository: "owner/repo",
            workflow_uri: "workflow",
            workflow_sha: "sha",
            source_sha: "sha",
            source_ref: "refs/heads/main",
            source_token: "fixture",
        };
        let output = verify_one(
            Path::new("fixture"),
            &identity,
            CheckDeadline::after(Duration::from_secs(1)).expect("deadline"),
        )
        .expect("injected output");
        assert_eq!(output, b"fixture");
        TEST_VERIFIED_OUTPUT.with(|slot| assert!(slot.borrow().is_empty()));
    }
}

pub(super) fn take() -> Option<Vec<u8>> {
    TEST_VERIFIED_OUTPUT.with(|slot| slot.borrow_mut().pop_front())
}
