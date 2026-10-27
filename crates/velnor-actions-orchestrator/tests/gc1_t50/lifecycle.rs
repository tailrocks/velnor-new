// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_context::{ExecutionBranch, UnitOutcome};
use velnor_actions_contract::{
    FinalStatus, JobConclusion, JobDisposition, ObligationDecision, PlanJob,
};

use super::fixtures_ledger::{
    branch_plan, group_entry, group_ledger_value, group_obligation, push_plan,
};
use super::fixtures_merge::{
    branch_typed, merge_with_event, passed, push_report_value, required_jobs,
};
use super::fixtures_platform::{digests, planned_platform, task_id};
use super::scheduling::fail;
use super::{GROUP_ID, GROUP_READER_ID, GROUP_WRITER_ID};

pub(super) fn check_push_warm_cache_hit_passes() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
        "push",
        ExecutionBranch::Writer,
        UnitOutcome::CacheHit,
        None,
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(&[reader, writer]),
        "push",
    )?;
    passed(
        &final_report,
        "warm writer cache-hit ledger did not preserve Required pass",
    )?;
    Ok(())
}

pub(super) fn check_push_cold_writer_wins() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::NeedsPrepare,
        None,
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
        "push",
        ExecutionBranch::Writer,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(&[reader, writer]),
        "push",
    )?;
    passed(
        &final_report,
        "cold writer ledger did not win over the deferring reader",
    )?;
    Ok(())
}

pub(super) fn check_push_absent_writer_with_reader_complete_passes() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(std::slice::from_ref(&reader)),
        "push",
    )?;
    passed(
        &final_report,
        "reader-complete ledger with an absent writer ledger must not be cache_corrupt",
    )?;
    Ok(())
}

pub(super) fn check_push_defers_without_writer_blocks() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "push",
        ExecutionBranch::Reader,
        UnitOutcome::NeedsPrepare,
        None,
    )?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(std::slice::from_ref(&reader)),
        "push",
    )?;
    if final_report.status != FinalStatus::Blocked {
        return Err(fail(
            "outstanding preparation without a writer ledger did not block",
        ));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "needs_prepare")
    {
        return Err(fail("outstanding preparation hid its miss reason"));
    }
    if final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "cache_corrupt")
    {
        return Err(fail(
            "honest-but-incomplete evidence reported as corruption",
        ));
    }
    Ok(())
}

pub(super) fn check_pr_forged_writer_stays_corrupt() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix_value, typed_report, _) = branch_typed(&task)?;
    let task_value = serde_json::to_value(&typed_report)?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "pull_request",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let writer = group_ledger_value(
        &task,
        GROUP_WRITER_ID,
        "pull_request",
        ExecutionBranch::Writer,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &plan,
        std::slice::from_ref(&matrix_value),
        std::slice::from_ref(&task_value),
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Success),
        ]),
        Some(&[reader, writer]),
        "pull_request",
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail(
            "off-push writer ledger next to a reader ledger did not fail",
        ));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "cache_corrupt")
    {
        return Err(fail("off-push writer ledger hid its corruption reason"));
    }
    Ok(())
}

pub(super) fn check_unaffected_with_ledger_mismatches() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (digest, input_digest, closure_digest) = digests();
    let mut plan = branch_plan(
        &task,
        group_entry(&task, &digest, &input_digest, &planned_platform()?)?,
        group_obligation(
            &task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::NotAffected,
        ),
        JobDisposition::NotAffected,
    )?;
    plan.matrix.include = Vec::new();
    plan.task_ids = vec![task.clone()];
    plan.jobs = vec![PlanJob {
        job_id: GROUP_ID.to_owned(),
        disposition: JobDisposition::NotAffected,
    }];
    plan.validate()?;
    let reader = group_ledger_value(
        &task,
        GROUP_READER_ID,
        "pull_request",
        ExecutionBranch::Reader,
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &plan,
        &[],
        &[],
        &[GROUP_READER_ID, GROUP_WRITER_ID],
        &required_jobs(&[
            (GROUP_READER_ID, JobConclusion::Success),
            (GROUP_WRITER_ID, JobConclusion::Skipped),
        ]),
        Some(std::slice::from_ref(&reader)),
        "pull_request",
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("ledger evidence for an unaffected group did not fail"));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "gate_mismatch")
    {
        return Err(fail("unaffected-group ledger hid its gate reason"));
    }
    Ok(())
}
