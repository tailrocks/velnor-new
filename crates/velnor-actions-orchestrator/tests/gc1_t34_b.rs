// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_unaffected_success_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (digest, input_digest, closure_digest) = digests();
    let plan = plan_with(
        &task,
        None,
        obligation(
            &task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::NotAffected,
        ),
        JobDisposition::NotAffected,
    )?;
    plan.validate()?;
    let final_report = merge(
        &plan,
        &[],
        &[],
        &[JOB_ID],
        required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("spurious unaffected success did not fail Required"));
    }
    if !final_report
        .miss_reasons
        .contains(&"gate_mismatch".to_owned())
    {
        return Err(fail("spurious unaffected success lost its gate token"));
    }
    Ok(())
}

fn check_empty_inventory_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[],
        required_jobs(&[]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("empty required inventory did not fail Required"));
    }
    if !final_report.miss_reasons.contains(&"no_entry".to_owned()) {
        return Err(fail("empty required inventory lost its entry token"));
    }
    Ok(())
}

fn check_reused_cache_obligation_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (digest, input_digest, closure_digest) = digests();
    let planned = planned_platform()?;
    let entry = run_entry(&task, &digest, &input_digest, &planned)?;
    let plan = plan_with(
        &task,
        Some(entry.clone()),
        obligation(
            &task,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::ReusedFromTaskCache,
        ),
        JobDisposition::Run,
    )?;
    plan.validate()?;
    let report = task_report(&task, &digest, &entry, &planned)?;
    let matrix = matrix_report(&task, &report, &entry);
    let final_report = merge(
        &plan,
        &[serde_json::to_value(&matrix)?],
        &[serde_json::to_value(&report)?],
        &[JOB_ID],
        required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("reused cache obligation did not fail Required"));
    }
    if !final_report.miss_reasons.contains(&"no_entry".to_owned()) {
        return Err(fail("reused cache obligation lost its entry token"));
    }
    Ok(())
}

fn check_run_key_mismatch_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        required_jobs(&[(JOB_ID, JobConclusion::Success)]),
        "r9999-a9",
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("run key mismatch did not fail Required"));
    }
    if !final_report
        .miss_reasons
        .contains(&"trust_scope_mismatch".to_owned())
    {
        return Err(fail("run key mismatch lost its scope token"));
    }
    Ok(())
}

#[test]
fn gc1_t34_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_missing_unit_rejected()?;
    check_failed_unit_rejected()?;
    check_skipped_run_unit_not_passed()?;
    check_duplicate_ledger_rejected()?;
    check_unplanned_inventory_rejected()?;
    check_unplanned_failure_rejected()?;
    check_unaffected_success_rejected()?;
    check_empty_inventory_rejected()?;
    check_reused_cache_obligation_rejected()?;
    check_run_key_mismatch_rejected()?;
    Ok(())
}
