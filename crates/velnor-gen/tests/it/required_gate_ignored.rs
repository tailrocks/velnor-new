// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::required_gate::{expect, passing, with};

#[test]
fn a_job_that_selected_no_tests_passes_as_an_empty_shard() -> TestResult {
    let none_listed = |ignored| {
        with(passing(), |run| {
            run.count = Some("0");
            run.listed = Some("0");
            run.ignored = ignored;
        })
    };
    let empty = "warning: rust-test selected no tests of root (empty shard)";
    expect(&none_listed(Some("0")), true, empty)?;
    expect(&none_listed(Some("3")), true, empty)?;
    expect(&none_listed(None), true, empty)?;
    expect(&none_listed(Some("")), true, empty)?;
    let garbled = with(passing(), |run| run.ignored = Some("-1"));
    expect(&garbled, false, "missing or malformed")
}
