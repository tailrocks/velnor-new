// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_duplicate_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = valid_ledger(&task)?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
        Some(&[ledger.clone(), ledger]),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "duplicate ledger did not fail Required",
    )
}

fn check_unknown_unit_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = group_ledger_value(
        "ghost-unit",
        JOB_ID,
        HEAD_SHA,
        "pull_request",
        ExecutionBranch::Writer,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "unknown unit ledger did not fail Required",
    )
}

fn check_success_contradiction_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = group_ledger_value(
        &task,
        JOB_ID,
        HEAD_SHA,
        "pull_request",
        ExecutionBranch::Writer,
        UnitOutcome::Failed,
        Some(1),
    )?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "failed ledger under success did not fail Required",
    )
}

fn check_failure_contradiction_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = valid_ledger(&task)?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Failure)]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "successful ledger under failure did not fail Required",
    )
}

fn check_skipped_job_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = valid_ledger(&task)?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Skipped)]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "skipped job ledger did not fail Required",
    )
}

fn check_malformed_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
        Some(std::slice::from_ref(&json!({"schema": 2}))),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "malformed ledger did not fail Required",
    )
}

#[test]
fn gc1_t35_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_cross_source_ledger_rejected()?;
    check_cross_event_ledger_rejected()?;
    check_unplanned_job_ledger_rejected()?;
    check_duplicate_ledger_rejected()?;
    check_unknown_unit_ledger_rejected()?;
    check_success_contradiction_rejected()?;
    check_failure_contradiction_rejected()?;
    check_skipped_job_ledger_rejected()?;
    check_malformed_ledger_rejected()?;
    Ok(())
}
