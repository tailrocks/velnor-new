// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_run_skip_trips() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, _) = check_run_fixture()?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Skipped)]),
        RUN_KEY,
        Some(&[]),
    )?;
    if final_report.status == FinalStatus::Passed {
        return Err(fail("skipped Run job preserved Required pass"));
    }
    if !final_report
        .miss_reasons
        .iter()
        .any(|reason| reason == "planned_job_skipped")
    {
        return Err(fail("skipped Run job hid its miss reason"));
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
            JOB_ID,
        ),
        JobDisposition::NotAffected,
    )?;
    plan.jobs = vec![PlanJob {
        job_id: JOB_ID.to_owned(),
        disposition: JobDisposition::NotAffected,
    }];
    plan.validate()?;
    let final_report = merge(
        &plan,
        &[],
        &[],
        &[JOB_ID],
        &required_jobs(&[(JOB_ID, JobConclusion::Skipped)]),
        RUN_KEY,
        Some(&[]),
    )?;
    if final_report.status != FinalStatus::Passed {
        return Err(fail(
            "unaffected crate job without a ledger did not preserve Required pass",
        ));
    }
    Ok(())
}

#[test]
fn gc1_t35_skipped_jobs_fold() -> Result<(), Box<dyn Error>> {
    check_run_skip_trips()?;
    check_unaffected_skip_passes()?;
    Ok(())
}
