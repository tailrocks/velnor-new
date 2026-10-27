// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_branch_reader_ledger_passes() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report) = branch_evidence(&task, GROUP_ID)?;
    let ledger = group_ledger_value(
        &task,
        GROUP_READER_ID,
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
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail("reader ledger did not preserve Required pass"));
    }
    if conclusion_of(&final_report, GROUP_READER_ID)? != JobConclusion::Success {
        return Err(fail("reader ledger lost the successful unit conclusion"));
    }
    if !final_report.miss_reasons.is_empty() {
        return Err(fail("reader ledger left unexpected miss reasons"));
    }
    Ok(())
}

fn check_branch_writer_ledger_passes() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report) = branch_evidence(&task, GROUP_ID)?;
    let ledger = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
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
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail("writer ledger did not preserve Required pass"));
    }
    if conclusion_of(&final_report, GROUP_WRITER_ID)? != JobConclusion::Success {
        return Err(fail("writer ledger lost the successful unit conclusion"));
    }
    Ok(())
}

fn check_branch_both_ledgers_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report) = branch_evidence(&task, GROUP_ID)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        HEAD_SHA,
        "pull_request",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
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
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(&[reader, writer]),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "reader-plus-writer ledgers did not fail Required",
    )
}

fn check_branch_mismatch_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report) = branch_evidence(&task, GROUP_ID)?;
    let ledger = group_ledger_value(
        &task,
        GROUP_READER_ID,
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
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "branch-mismatched ledger did not fail Required",
    )
}

fn check_branch_foreign_group_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report) = branch_evidence(&task, GROUP_ID)?;
    let ledger = group_ledger_value(
        &task,
        "rust-ghost-job-read",
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
        &[GROUP_READER_ID, GROUP_WRITER_ID, "rust-ghost-job-read"],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
            ("rust-ghost-job-read", JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "foreign group ledger did not fail Required",
    )
}

fn check_branch_missing_rejected() -> Result<(), Box<dyn Error>> {
    let task_a = task_id()?;
    let task_b = second_task()?;
    let (plan_a, matrix_a, report_a) = branch_evidence(&task_a, GROUP_ID)?;
    let (digest, input_digest, closure_digest) = digests();
    let planned = planned_platform()?;
    let entry_b = group_run_entry(&task_b, &digest, &input_digest, &planned, SIDE_GROUP_ID)?;
    let report_b = task_report(&task_b, &digest, &entry_b, &planned)?;
    let matrix_b = matrix_report(&task_b, &report_b, &entry_b);
    let mut plan = plan_a;
    plan.obligations.push(group_obligation(
        &task_b,
        &digest,
        &input_digest,
        &closure_digest,
        ObligationDecision::Execute,
        SIDE_GROUP_ID,
    ));
    plan.matrix.include.push(entry_b);
    plan.task_ids.push(task_b.clone());
    plan.task_ids.sort();
    plan.jobs.push(PlanJob {
        job_id: SIDE_GROUP_ID.to_owned(),
        disposition: JobDisposition::Run,
    });
    plan.jobs
        .sort_by(|left, right| left.job_id.cmp(&right.job_id));
    plan.validate()?;
    let ledger = group_ledger_value(
        &task_a,
        GROUP_READER_ID,
        HEAD_SHA,
        "pull_request",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge(
        &plan,
        &[matrix_a, serde_json::to_value(&matrix_b)?],
        &[report_a, serde_json::to_value(&report_b)?],
        &[
            GROUP_READER_ID,
            GROUP_WRITER_ID,
            SIDE_READER_ID,
            SIDE_WRITER_ID,
        ],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
            (SIDE_READER_ID, JobConclusion::Success),
            (SIDE_WRITER_ID, JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    planning_failed(
        &final_report,
        "no_entry",
        "missing group ledger did not fail Required",
    )
}

fn check_branch_unaffected_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (digest, input_digest, closure_digest) = digests();
    let mut plan = plan_with(
        &task,
        None,
        group_obligation(
            &task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::NotAffected,
            GROUP_ID,
        ),
        JobDisposition::NotAffected,
    )?;
    plan.jobs = vec![PlanJob {
        job_id: GROUP_ID.to_owned(),
        disposition: JobDisposition::NotAffected,
    }];
    plan.validate()?;
    let ledger = group_ledger_value(
        &task,
        GROUP_READER_ID,
        HEAD_SHA,
        "pull_request",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge(
        &plan,
        &[],
        &[],
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Skipped),
            (GROUP_WRITER_ID, JobConclusion::Skipped),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    planning_failed(
        &final_report,
        "gate_mismatch",
        "unaffected group ledger did not fail Required",
    )
}

#[test]
fn gc1_t35_branch_ledgers() -> Result<(), Box<dyn Error>> {
    check_branch_reader_ledger_passes()?;
    check_branch_writer_ledger_passes()?;
    check_branch_both_ledgers_rejected()?;
    check_branch_mismatch_rejected()?;
    check_branch_foreign_group_rejected()?;
    check_branch_missing_rejected()?;
    check_branch_unaffected_ledger_rejected()?;
    Ok(())
}
