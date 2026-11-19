// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{judge, passing, set, with};
use crate::facts::TestResult;

fn skipped_push(
    flag: Option<&'static str>,
    event: &'static str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let run = with(passing(), |run| {
        run.count = None;
        run.listed = None;
        run.event = Some(event);
        if let Some(flag) = flag {
            run.outputs.push(("skip_push", flag));
        }
    });
    Ok(judge(&run)?.0)
}

#[test]
fn a_push_after_a_passed_merge_group_run_need_not_report_tests() -> TestResult {
    assert!(skipped_push(Some("true"), "push")?);
    Ok(())
}

#[test]
fn the_skip_waiver_holds_for_a_push_only_and_only_for_true() -> TestResult {
    for (flag, event) in [
        (Some("true"), "pull_request"),
        (Some("true"), "merge_group"),
        (Some("true"), "schedule"),
        (Some("false"), "push"),
        (Some(""), "push"),
        (Some("yes"), "push"),
        (None, "push"),
    ] {
        assert!(!skipped_push(flag, event)?, "{flag:?} {event}");
    }
    Ok(())
}

#[test]
fn the_skip_waiver_does_not_hide_a_failed_job() -> TestResult {
    let run = with(passing(), |run| {
        run.event = Some("push");
        run.outputs.push(("skip_push", "true"));
        set(&mut run.results, "rust-lint", "failure");
    });
    assert!(!judge(&run)?.0);
    Ok(())
}
