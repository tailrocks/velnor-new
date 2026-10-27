// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_pr_writer_skip_passes() -> Result<(), Box<dyn Error>> {
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
            (GROUP_WRITER_ID, JobConclusion::Skipped),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail(
            "normally-skipped writer did not preserve Required pass",
        ));
    }
    if conclusion_of(&final_report, GROUP_WRITER_ID)? != JobConclusion::Skipped {
        return Err(fail("normally-skipped writer lost its skipped conclusion"));
    }
    if !final_report.miss_reasons.is_empty() {
        return Err(fail("normally-skipped writer left unexpected miss reasons"));
    }
    Ok(())
}

fn check_push_writer_skip_trips() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task, GROUP_ID)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let matrix_value = serde_json::to_value(&matrix)?;
    let report_value = push_report(&typed_report)?;
    let ledger = group_ledger_value(
        &task,
        GROUP_READER_ID,
        HEAD_SHA,
        "push",
        ExecutionBranch::Reader,
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
            (GROUP_WRITER_ID, JobConclusion::Skipped),
        ]),
        RUN_KEY,
        Some(std::slice::from_ref(&ledger)),
        "push",
    )?;
    if final_report.status != FinalStatus::NotRun {
        return Err(fail("push-skipped writer did not trip Required"));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "planned_job_skipped")
    {
        return Err(fail("push-skipped writer hid its miss reason"));
    }
    Ok(())
}

fn check_push_branches_pass() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task, GROUP_ID)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let matrix_value = serde_json::to_value(&matrix)?;
    let report_value = push_report(&typed_report)?;
    let ledger = group_ledger_value(
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
        Some(std::slice::from_ref(&ledger)),
        "push",
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail("push branches did not preserve Required pass"));
    }
    if !final_report.miss_reasons.is_empty() {
        return Err(fail("push branches left unexpected miss reasons"));
    }
    Ok(())
}

fn check_pr_reader_skip_trips() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report) = branch_evidence(&task, GROUP_ID)?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Skipped),
            (GROUP_WRITER_ID, JobConclusion::Skipped),
        ]),
        RUN_KEY,
        Some(&[]),
    )?;
    if final_report.status == FinalStatus::Passed {
        return Err(fail("skipped reader preserved Required pass"));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "planned_job_skipped")
    {
        return Err(fail("skipped reader hid its miss reason"));
    }
    Ok(())
}

fn check_unaffected_skip_passes() -> Result<(), Box<dyn Error>> {
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
        Some(&[]),
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail("unaffected skips did not preserve Required pass"));
    }
    Ok(())
}

#[test]
fn gc1_t35_skipped_branches_fold() -> Result<(), Box<dyn Error>> {
    check_pr_writer_skip_passes()?;
    check_push_writer_skip_trips()?;
    check_push_branches_pass()?;
    check_pr_reader_skip_trips()?;
    check_unaffected_skip_passes()?;
    Ok(())
}
