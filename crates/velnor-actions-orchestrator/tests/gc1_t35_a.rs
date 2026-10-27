// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_grouped_success_preserved() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, entry) = check_run_fixture()?;
    let group_ledger = valid_ledger(&task)?;
    let merged = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
        Some(std::slice::from_ref(&group_ledger)),
    )?;
    let baseline = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
        Some(&[]),
    )?;
    if merged.status != FinalStatus::Passed {
        return Err(fail("grouped ledger did not preserve Required pass"));
    }
    if merged.run_key != RUN_KEY
        || merged.plan_id != plan.plan_id
        || merged.report_id != format!("final-{RUN_KEY}")
    {
        return Err(fail("grouped ledger rewrote the source run binding"));
    }
    if merged.downloaded_artifact_ids != baseline.downloaded_artifact_ids
        || merged.downloaded_artifact_ids != [entry.artifact_id.clone()]
    {
        return Err(fail("grouped ledger lost the covered artifact binding"));
    }
    if merged.counts.selected != baseline.counts.selected
        || merged.counts.selected != 1
        || merged.counts.executed != baseline.counts.executed
        || merged.counts.executed != 1
    {
        return Err(fail("grouped ledger miscounted the covered unit"));
    }
    if serde_json::to_value(&merged.required_job_results)?
        != serde_json::to_value(&baseline.required_job_results)?
    {
        return Err(fail("grouped ledger rewrote the joined job results"));
    }
    if conclusion_of(&merged, JOB_ID)? != JobConclusion::Success {
        return Err(fail("grouped ledger lost the successful unit conclusion"));
    }
    if !merged.miss_reasons.is_empty() {
        return Err(fail("grouped ledger left unexpected miss reasons"));
    }
    Ok(())
}

fn check_grouped_reader_branch_preserved() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = group_ledger_value(
        &task,
        JOB_ID,
        HEAD_SHA,
        "pull_request",
        ExecutionBranch::Reader,
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
    if final_report.status != FinalStatus::Passed {
        return Err(fail("reader ledger did not preserve Required pass"));
    }
    if conclusion_of(&final_report, JOB_ID)? != JobConclusion::Success {
        return Err(fail("reader ledger lost the successful unit conclusion"));
    }
    Ok(())
}

fn check_absent_ledgers_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report) = branch_evidence(&task, GROUP_ID)?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        RUN_KEY,
        None,
    )?;
    planning_failed(
        &final_report,
        "no_entry",
        "request without ledgers did not fail Required",
    )
}

#[test]
fn gc1_t35_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_grouped_success_preserved()?;
    check_grouped_reader_branch_preserved()?;
    check_absent_ledgers_rejected()?;
    Ok(())
}

pub(crate) fn planning_failed(
    report: &FinalReport,
    token: &str,
    case: &str,
) -> Result<(), Box<dyn Error>> {
    if report.status != FinalStatus::PlanningFailed {
        return Err(fail(case));
    }
    if !report.miss_reasons.iter().any(|reason| reason == token) {
        return Err(fail(case));
    }
    Ok(())
}

pub(crate) fn check_cross_source_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = group_ledger_value(
        &task,
        JOB_ID,
        OTHER_SHA,
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
        "trust_scope_mismatch",
        "cross-source ledger did not fail Required",
    )
}

pub(crate) fn check_cross_event_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = group_ledger_value(
        &task,
        JOB_ID,
        HEAD_SHA,
        "push",
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
        "trust_scope_mismatch",
        "cross-event ledger did not fail Required",
    )
}

pub(crate) fn check_unplanned_job_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let ledger = group_ledger_value(
        &task,
        "ghost-job",
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
        "unplanned job ledger did not fail Required",
    )
}
