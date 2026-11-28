// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn the_reuse_charge_covers_the_longest_run_url_github_can_build() {
    let longest = format!(
        "https://github.com/{}/{}/actions/runs/{}",
        "o".repeat(39),
        "r".repeat(100),
        u64::MAX
    );
    let charged = reuse_bytes(&[]);
    assert!(
        charged
            >= property_bytes(REUSE_OUTPUT, 5)
                + property_bytes(REUSED_OUTPUT, 2)
                + property_bytes(REUSE_RUN_OUTPUT, longest.len()),
        "{charged} bytes charged for a {} byte url",
        longest.len()
    );
}
