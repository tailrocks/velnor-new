// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::GREEN;
use super::support::{Recorded, check};

const HOSTILE: &str = "Required\n::stop-commands::token";

#[test]
fn a_check_name_with_a_line_break_stays_on_one_line_of_the_report() {
    let verdict = Recorded::new()
        .requiring(&[check(HOSTILE)])
        .green_job("Required", |job| job["name"] = json!(HOSTILE))
        .gate(GREEN)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        verdict
            .report
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{}",
        verdict.report
    );
    assert!(
        verdict.report.contains("%0A::stop-commands::token passed"),
        "{}",
        verdict.report
    );
}
