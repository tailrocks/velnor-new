// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

struct TwoUnits {
    plan: Plan,
    matrices: Vec<Value>,
    reports: Vec<Value>,
    first: String,
    second: String,
}

fn two_units(first_group: &str, second_group: &str) -> Result<TwoUnits, Box<dyn Error>> {
    let first = task_id()?;
    let second = second_task()?;
    let (digest, input_digest, closure_digest) = digests();
    let planned = planned_platform()?;
    let mut plan = plan_with(
        &first,
        None,
        group_obligation(
            &first,
            &digest,
            &input_digest,
            &closure_digest,
            ObligationDecision::Execute,
            first_group,
        ),
        JobDisposition::Run,
    )?;
    plan.obligations.push(group_obligation(
        &second,
        &digest,
        &input_digest,
        &closure_digest,
        ObligationDecision::Execute,
        second_group,
    ));
    plan.task_ids = vec![first.clone(), second.clone()];
    plan.task_ids.sort();
    let groups: std::collections::BTreeSet<&str> = [first_group, second_group].into();
    plan.jobs = groups
        .into_iter()
        .map(|group| PlanJob {
            lane: None,
            job_id: group.to_owned(),
            disposition: JobDisposition::Run,
        })
        .collect();
    let mut matrices = Vec::new();
    let mut reports = Vec::new();
    for (task, group) in [(&first, first_group), (&second, second_group)] {
        let entry = group_run_entry(task, &digest, &input_digest, &planned, group)?;
        let report = task_report(task, &digest, &entry, &planned)?;
        matrices.push(serde_json::to_value(matrix_report(task, &report, &entry))?);
        reports.push(serde_json::to_value(&report)?);
        plan.matrix.include.push(entry);
    }
    plan.validate()?;
    Ok(TwoUnits {
        plan,
        matrices,
        reports,
        first,
        second,
    })
}

fn ledger(task: &str, job_key: &str, report: &Value) -> Result<Value, Box<dyn Error>> {
    group_ledger_value(
        task,
        job_key,
        HEAD_SHA,
        "pull_request",
        UnitOutcome::Executed,
        Some(0),
        report,
    )
}

fn merge_units(
    units: &TwoUnits,
    job_ids: &[&str],
    ledgers: &[Value],
) -> Result<FinalReport, Box<dyn Error>> {
    let conclusions: Vec<(&str, JobConclusion)> = job_ids
        .iter()
        .map(|id| (*id, JobConclusion::Success))
        .collect();
    merge(
        &units.plan,
        &units.matrices,
        &units.reports,
        job_ids,
        &required_jobs(&conclusions),
        RUN_KEY,
        Some(ledgers),
    )
}

#[test]
fn foreign_group_rejected() -> Result<(), Box<dyn Error>> {
    let task = task_id()?;
    let (plan, matrix, report, _) = check_run_fixture()?;
    let covering = ledger(&task, JOB_ID, &report)?;
    let foreign = ledger(&task, "rust-ghost-job", &report)?;
    let final_report = merge(
        &plan,
        std::slice::from_ref(&matrix),
        std::slice::from_ref(&report),
        &[JOB_ID, "rust-ghost-job"],
        &required_jobs(&[
            (JOB_ID, JobConclusion::Success),
            ("rust-ghost-job", JobConclusion::Success),
        ]),
        RUN_KEY,
        Some(&[covering, foreign]),
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "unplanned crate group ledger did not fail Required",
    )
}

#[test]
fn missing_group_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let units = two_units(JOB_ID, SIDE_JOB_ID)?;
    let final_report = merge_units(
        &units,
        &[JOB_ID, SIDE_JOB_ID],
        &[ledger(&units.first, JOB_ID, &units.reports[0])?],
    )?;
    planning_failed(
        &final_report,
        "no_entry",
        "Run crate group without a ledger did not fail Required",
    )
}

#[test]
fn unclaimed_tofu_unit_rejected() -> Result<(), Box<dyn Error>> {
    let units = two_units("tofu-unit-root", "tofu-unit-root")?;
    let final_report = merge_units(
        &units,
        &["tofu-unit-root"],
        &[ledger(&units.first, "tofu-unit-root", &units.reports[0])?],
    )?;
    planning_failed(
        &final_report,
        "no_entry",
        "unclaimed tofu execute unit did not fail Required",
    )
}

#[test]
fn check_group_without_ledger_passes() -> Result<(), Box<dyn Error>> {
    let units = two_units(JOB_ID, "check-side")?;
    let final_report = merge_units(
        &units,
        &[JOB_ID, "check-side"],
        &[ledger(&units.first, JOB_ID, &units.reports[0])?],
    )?;
    if final_report.status != FinalStatus::Passed || !final_report.miss_reasons.is_empty() {
        return Err(fail(
            "check group without a ledger did not preserve Required pass",
        ));
    }
    Ok(())
}

#[test]
fn check_group_ledger_rejected() -> Result<(), Box<dyn Error>> {
    let units = two_units(JOB_ID, "check-side")?;
    let final_report = merge_units(
        &units,
        &[JOB_ID, "check-side"],
        &[
            ledger(&units.first, JOB_ID, &units.reports[0])?,
            ledger(&units.second, "check-side", &units.reports[1])?,
        ],
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "ledger filed under a check group did not fail Required",
    )
}

#[test]
fn duplicate_unit_claim_rejected() -> Result<(), Box<dyn Error>> {
    let units = two_units(JOB_ID, SIDE_JOB_ID)?;
    let final_report = merge_units(
        &units,
        &[JOB_ID, SIDE_JOB_ID],
        &[
            ledger(&units.first, JOB_ID, &units.reports[0])?,
            ledger(&units.first, SIDE_JOB_ID, &units.reports[0])?,
        ],
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "two ledgers claiming one unit did not fail Required",
    )
}

#[test]
fn crate_job_left_out_of_the_request_rejected() -> Result<(), Box<dyn Error>> {
    let units = two_units(JOB_ID, SIDE_JOB_ID)?;
    let final_report = merge_units(
        &units,
        &[JOB_ID],
        &[ledger(&units.first, JOB_ID, &units.reports[0])?],
    )?;
    planning_failed(
        &final_report,
        "no_entry",
        "Run crate job left out of the request dodged ledger coverage",
    )
}

#[test]
fn cross_group_claims_rejected() -> Result<(), Box<dyn Error>> {
    let units = two_units(JOB_ID, SIDE_JOB_ID)?;
    let final_report = merge_units(
        &units,
        &[JOB_ID, SIDE_JOB_ID],
        &[
            ledger(&units.second, JOB_ID, &units.reports[1])?,
            ledger(&units.first, SIDE_JOB_ID, &units.reports[0])?,
        ],
    )?;
    planning_failed(
        &final_report,
        "cache_corrupt",
        "ledgers claiming units of another crate job did not fail Required",
    )
}
