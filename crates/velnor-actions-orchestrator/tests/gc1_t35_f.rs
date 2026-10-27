// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_push_writer_wins_over_reader() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task, GROUP_ID)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let matrix_value = serde_json::to_value(&matrix)?;
    let report_value = push_report(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        HEAD_SHA,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
        HEAD_SHA,
        "push",
        ExecutionBranch::Writer,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix_value),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(&[reader, writer]),
        "push",
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail("writer ledger did not win over the reader ledger"));
    }
    if !final_report.miss_reasons.is_empty() {
        return Err(fail("writer-wins left unexpected miss reasons"));
    }
    Ok(())
}

fn check_push_needs_prepare_blocked() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task, GROUP_ID)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let matrix_value = serde_json::to_value(&matrix)?;
    let report_value = push_report(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        HEAD_SHA,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::NeedsPrepare,
        None,
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix_value),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Skipped),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&reader)),
        "push",
    )?;
    if final_report.status != FinalStatus::Blocked {
        return Err(fail("missing writer preparation did not block Required"));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "needs_prepare")
    {
        return Err(fail("missing writer preparation hid its miss reason"));
    }
    Ok(())
}

fn check_pr_needs_prepare_blocked() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report) = branch_evidence(&task, GROUP_ID)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        HEAD_SHA,
        "pull_request",
        ExecutionBranch::Reader,
        UnitOutcome::NeedsPrepare,
        None,
    )?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Skipped),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&reader)),
    )?;
    if final_report.status != FinalStatus::Blocked {
        return Err(fail("incomplete reader ledger did not block Required"));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "needs_prepare")
    {
        return Err(fail("incomplete reader ledger hid its miss reason"));
    }
    Ok(())
}

fn check_push_cold_preparation_passes() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task, GROUP_ID)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let matrix_value = serde_json::to_value(&matrix)?;
    let report_value = push_report(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        HEAD_SHA,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::NeedsPrepare,
        None,
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
        HEAD_SHA,
        "push",
        ExecutionBranch::Writer,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix_value),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(&[reader, writer]),
        "push",
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail("writer preparation did not preserve Required pass"));
    }
    if !final_report.miss_reasons.is_empty() {
        return Err(fail("writer preparation left unexpected miss reasons"));
    }
    Ok(())
}

#[test]
fn gc1_t35_one_ledger_wins() -> Result<(), Box<dyn Error>> {
    check_push_writer_wins_over_reader()?;
    check_push_cold_preparation_passes()?;
    check_push_needs_prepare_blocked()?;
    check_pr_needs_prepare_blocked()?;
    Ok(())
}
