// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::{ADMIN_RENEWAL, Step, renew_held};

#[test]
fn a_held_session_or_clear_step_renews_the_admin_token_only_once_it_is_old() {
    let old = ADMIN_RENEWAL;
    let young = ADMIN_RENEWAL.saturating_sub(Duration::from_secs(1));
    assert_eq!(renew_held(Step::Session(2), old), Some(2));
    assert_eq!(renew_held(Step::Clear(1), old), Some(1));
    assert_eq!(
        renew_held(Step::Session(2), young),
        None,
        "a token younger than the renewal age must not be renewed"
    );
    assert_eq!(renew_held(Step::Clear(1), young), None);
    for step in [Step::Machine, Step::Sweep, Step::Set(0), Step::Renew(0)] {
        assert_eq!(
            renew_held(step, old * 2),
            None,
            "only a held session or clear step renews the token"
        );
    }
}
