// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::required_gate::{expect, passing, with};

#[test]
fn a_suite_that_is_compiled_out_fails_and_an_all_ignored_suite_passes() -> TestResult {
    let none_listed = |ignored| {
        with(passing(), |run| {
            run.count = Some("0");
            run.listed = Some("0");
            run.ignored = ignored;
        })
    };
    let compiled_out = "a test job of root listed no tests and ran none";
    expect(&none_listed(Some("0")), false, compiled_out)?;
    expect(&none_listed(Some("3")), true, "Required: pass")?;
    expect(&none_listed(None), true, "Required: pass")?;
    expect(&none_listed(Some("")), true, "Required: pass")?;
    let garbled = with(passing(), |run| run.ignored = Some("-1"));
    expect(&garbled, false, "missing or malformed")
}
