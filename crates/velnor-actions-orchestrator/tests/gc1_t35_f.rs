// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_pr_needs_prepare_blocked() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let deferring = group_ledger_value(
        &task,
        JOB_ID,
        HEAD_SHA,
        "pull_request",
        UnitOutcome::NeedsPrepare,
        None,
        &report,
    )?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
        Some(std::slice::from_ref(&deferring)),
    )?;
    if final_report.status != FinalStatus::Blocked {
        return Err(fail("deferring crate ledger did not block Required"));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "needs_prepare")
    {
        return Err(fail("deferring crate ledger hid its miss reason"));
    }
    Ok(())
}

#[test]
fn gc1_t35_needs_prepare() -> Result<(), Box<dyn Error>> {
    check_pr_needs_prepare_blocked()?;
    Ok(())
}
