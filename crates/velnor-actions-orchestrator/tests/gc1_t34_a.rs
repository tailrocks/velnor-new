// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t34_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_run_success_preserved()?;
    check_unaffected_skip_preserved()?;
    check_untrusted_source_rejected()?;
    Ok(())
}

pub(crate) fn check_missing_unit_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        required_jobs(&[]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("missing planned unit did not fail Required"));
    }
    if conclusion_of(&final_report, JOB_ID)? != JobConclusion::Missing {
        return Err(fail("missing planned unit was not reported as missing"));
    }
    if !final_report.miss_reasons.contains(&"no_entry".to_owned()) {
        return Err(fail("missing planned unit lost its entry token"));
    }
    Ok(())
}

pub(crate) fn check_failed_unit_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        required_jobs(&[(JOB_ID, JobConclusion::Failure)]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::Failed {
        return Err(fail("failed planned unit did not fail Required"));
    }
    if conclusion_of(&final_report, JOB_ID)? != JobConclusion::Failure {
        return Err(fail("required ledger lost the failed unit conclusion"));
    }
    Ok(())
}

pub(crate) fn check_skipped_run_unit_not_passed() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        required_jobs(&[(JOB_ID, JobConclusion::Skipped)]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::NotRun {
        return Err(fail("skipped planned unit did not aggregate to not-run"));
    }
    if !final_report
        .miss_reasons
        .contains(&"planned_job_skipped".to_owned())
    {
        return Err(fail("skipped planned unit lost its skip token"));
    }
    Ok(())
}

pub(crate) fn check_duplicate_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let duplicated = Value::Array(vec![
        json!({"job_id": JOB_ID, "conclusion": "success"}),
        json!({"job_id": JOB_ID, "conclusion": "success"}),
    ]);
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        duplicated,
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("duplicate unit ledger did not fail Required"));
    }
    if !final_report
        .miss_reasons
        .contains(&"cache_corrupt".to_owned())
    {
        return Err(fail("duplicate unit ledger lost its corruption token"));
    }
    Ok(())
}

pub(crate) fn check_unplanned_inventory_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        required_jobs(&[
            (JOB_ID, JobConclusion::Success),
            ("extra-job", JobConclusion::Success),
        ]),
        RUN_KEY,
    )?;
    if final_report.status != FinalStatus::PlanningFailed {
        return Err(fail("unplanned inventory unit did not fail Required"));
    }
    if !final_report
        .miss_reasons
        .contains(&"cache_corrupt".to_owned())
    {
        return Err(fail("unplanned inventory unit lost its corruption token"));
    }
    Ok(())
}

pub(crate) fn check_unplanned_failure_rejected() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID, "extra-job"],
        required_jobs(&[
            (JOB_ID, JobConclusion::Success),
            ("extra-job", JobConclusion::Failure),
        ]),
        RUN_KEY,
    )?;
    if final_report.status == FinalStatus::Passed {
        return Err(fail("failed unplanned unit did not fail Required"));
    }
    if conclusion_of(&final_report, "extra-job")? != JobConclusion::Failure {
        return Err(fail(
            "required ledger lost the unplanned failure conclusion",
        ));
    }
    Ok(())
}
