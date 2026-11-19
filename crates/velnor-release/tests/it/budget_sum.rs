// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{RootPlan, publish_budget_minutes};

fn root(nanos: u128) -> RootPlan {
    RootPlan {
        root: ".".to_owned(),
        pr_branch_prefix: String::new(),
        publishes_to_crates_io: true,
        packages: Vec::new(),
        inherited: Vec::new(),
        publish_budget_nanos: nanos,
        workspace_tags: Vec::new(),
        version_groups: std::collections::BTreeMap::new(),
    }
}

#[test]
fn budgets_whose_sum_exceeds_the_integer_range_saturate_instead_of_wrapping_to_zero() {
    let half = 1u128 << 127;
    let roots = [root(half), root(half), root(480_000_000_000)];
    assert_eq!(publish_budget_minutes(&roots), u64::MAX);
}
