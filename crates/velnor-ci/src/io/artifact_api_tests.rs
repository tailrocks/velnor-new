// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use super::artifact_api::Bound;
use super::http::DEFAULT_TIMEOUT;

#[test]
fn the_reuse_bound_is_a_thirty_second_single_attempt_and_the_default_is_untouched() {
    assert_eq!(Bound::REUSE.timeout, Duration::from_secs(30));
    assert_eq!(Bound::REUSE.attempts, 1);
    assert_eq!(Bound::DEFAULT.timeout, DEFAULT_TIMEOUT);
    assert_eq!(Bound::DEFAULT.attempts, 3);
}
