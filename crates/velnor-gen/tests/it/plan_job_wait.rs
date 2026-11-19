// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

#[test]
fn a_lane_waits_for_a_successful_plan_job_and_a_job_that_plans_itself_has_no_plan_job_to_wait_for()
-> TestResult {
    let multi = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    for slug in ["root", "lab"] {
        let header = format!("  rust-{slug}:\n");
        let body = &multi[multi.find(&header).ok_or("lane")? + header.len()..];
        let condition = body
            .lines()
            .find_map(|line| line.strip_prefix("    if: "))
            .ok_or("lane if")?;
        assert!(
            condition.contains(&format!(
                "!cancelled() && needs.rust-plan.result == ''success'' && needs.rust-plan.outputs.rust_{slug}_mode != ''none''"
            )),
            "{condition}"
        );
    }
    let single = workflow(&view(&["Cargo.toml"], None)?)?;
    assert!(!single.contains("needs.rust-plan"), "{single}");
    assert!(
        single.contains(
            "steps.plan.outcome == ''success'' && steps.plan.outputs.rust_root_mode != ''none''"
        ),
        "{single}"
    );
    Ok(())
}
