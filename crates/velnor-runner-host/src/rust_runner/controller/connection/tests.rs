// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use super::{DEFAULT_BRANCH_TTL, DefaultBranch};

#[test]
fn the_default_branch_is_known_until_its_age_reaches_the_limit() {
    let fetched = Instant::now();
    let branch = DefaultBranch {
        name: "trunk".to_owned(),
        fetched,
    };
    assert_eq!(branch.known(fetched), Some("trunk"));
    assert_eq!(
        branch.known(fetched + DEFAULT_BRANCH_TTL.saturating_sub(Duration::from_secs(1))),
        Some("trunk"),
        "a default branch one second younger than the limit must still be served from the cache"
    );
    assert_eq!(
        branch.known(fetched + DEFAULT_BRANCH_TTL),
        None,
        "a default branch as old as the limit must be fetched again"
    );
}
