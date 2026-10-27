// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_run_fixture() -> Result<(Plan, Value, Value, MatrixEntry), Box<dyn Error>> {
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
            ObligationDecision::Execute,
        ),
        JobDisposition::Run,
    )?;
    plan.validate()?;
    let report = task_report(&task, &digest, &entry, &planned)?;
    report.validate()?;
    let matrix = matrix_report(&task, &report, &entry);
    matrix.validate()?;
    Ok((
        plan,
        serde_json::to_value(&matrix)?,
        serde_json::to_value(&report)?,
        entry,
    ))
}

pub(crate) fn job_binding(
    job_key: &str,
    source_sha: &str,
    event: &str,
) -> Result<JobBindingV1, Box<dyn Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 4242,
        run_attempt: 1,
        job_id: 7,
        job_key: job_key.to_owned(),
        event: event.to_owned(),
        ref_name: "refs/pull/1/merge".to_owned(),
        workflow_sha: HEAD_SHA.to_owned(),
        source_sha: source_sha.to_owned(),
        plan_digest: digest(b"gc1-t35-plan")?,
    })
}

pub(crate) fn unit_result(
    task: &str,
    outcome: UnitOutcome,
    exit_code: Option<i32>,
) -> Result<UnitResultV1, Box<dyn Error>> {
    Ok(UnitResultV1 {
        unit_id: task.to_owned(),
        recipe_id: digest(b"gc1-t35-recipe")?,
        task_identity: task.to_owned(),
        argv_digest: digest(b"gc1-t35-argv")?,
        effective_input_digest: digest(b"gc1-t35-input")?,
        outcome,
        exit_code,
        started_at: "2026-10-26T00:00:00Z".to_owned(),
        finished_at: "2026-10-26T00:00:01Z".to_owned(),
        report_digest: digest(b"gc1-t35-report")?,
    })
}

pub(crate) fn group_ledger_value(
    task: &str,
    job_key: &str,
    source_sha: &str,
    event: &str,
    branch: ExecutionBranch,
    outcome: UnitOutcome,
    exit_code: Option<i32>,
) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::to_value(UnitLedgerV1 {
        schema: 1,
        job: job_binding(job_key, source_sha, event)?,
        group_digest: digest(b"gc1-t35-group")?,
        execution_branch: branch,
        selected_unit_ids: vec![task.to_owned()],
        ordered_results: vec![unit_result(task, outcome, exit_code)?],
        observation_digest: digest(b"gc1-t35-observation")?,
        source_binding_digest: digest(b"gc1-t35-source-binding")?,
    })?)
}

pub(crate) fn valid_ledger(task: &str) -> Result<Value, Box<dyn Error>> {
    group_ledger_value(
        task,
        JOB_ID,
        HEAD_SHA,
        "pull_request",
        ExecutionBranch::Writer,
        UnitOutcome::Executed,
        Some(0),
    )
}
