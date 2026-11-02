// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_context::UnitOutcome;
use velnor_actions_contract::{
    FinalStatus, JobConclusion, JobDisposition, ObligationDecision, PlanJob,
};

use super::GROUP_ID;
use super::fail;
use super::fixtures_ledger::{
    branch_plan, group_entry, group_ledger_value, group_obligation, push_plan,
};
use super::fixtures_merge::{
    branch_typed, merge_with_event, passed, push_report_value, required_jobs,
};
use super::fixtures_platform::{digests, planned_platform, task_id};

pub(super) fn check_push_warm_cache_hit_passes() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let warm = group_ledger_value(&task, GROUP_ID, "push", UnitOutcome::CacheHit, None)?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_ID],
        &required_jobs(&[(GROUP_ID, JobConclusion::Success)]),
        Some(std::slice::from_ref(&warm)),
        "push",
    )?;
    passed(
        &final_report,
        "warm cache-hit ledger did not preserve Required pass",
    )?;
    Ok(())
}

pub(super) fn check_push_deferring_ledger_blocks() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, typed_report, _) = branch_typed(&task)?;
    let pushed = push_plan(&plan);
    pushed.validate()?;
    let report_value = push_report_value(&typed_report)?;
    let deferring = group_ledger_value(&task, GROUP_ID, "push", UnitOutcome::NeedsPrepare, None)?;
    let final_report = merge_with_event(
        &pushed,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report_value),
        &[GROUP_ID],
        &required_jobs(&[(GROUP_ID, JobConclusion::Success)]),
        Some(std::slice::from_ref(&deferring)),
        "push",
    )?;
    if final_report.status != FinalStatus::Blocked {
        return Err(fail(
            "deferring ledger without a covering ledger did not block",
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
    let ledger = group_ledger_value(
        &task,
        GROUP_ID,
        "pull_request",
        UnitOutcome::Executed,
        Some(0),
    )?;
    let final_report = merge_with_event(
        &plan,
        &[],
        &[],
        &[GROUP_ID],
        &required_jobs(&[(GROUP_ID, JobConclusion::Success)]),
        Some(std::slice::from_ref(&ledger)),
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
