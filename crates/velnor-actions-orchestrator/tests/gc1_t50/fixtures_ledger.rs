// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use serde_json::{Value, json};
use velnor_actions_contract::workflow::cache_context::{
    ExecutionBranch, UnitLedgerV1, UnitOutcome,
};
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, JobDisposition, MatrixEntry, ObligationDecision, Plan,
    PlanGenerator, PlanJob, PlanMatrix, PlanObligation, PlanRunner, PlannedPlatform,
    RunnerSelection, WorkflowEvent, plan_id_for_run, trust_for_event,
};

use super::fixtures_platform::{digest, job_binding, unit_result};
use super::{GROUP_ID, HEAD_SHA, RUN_KEY};

pub(super) fn group_ledger_value(
    task: &str,
    job_key: &str,
    event: &str,
    branch: ExecutionBranch,
    outcome: UnitOutcome,
    exit_code: Option<i32>,
) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::to_value(UnitLedgerV1 {
        schema: 1,
        job: job_binding(job_key, event)?,
        group_digest: digest(b"gc1-t50-group")?,
        execution_branch: branch,
        selected_unit_ids: vec![task.to_owned()],
        ordered_results: vec![unit_result(task, outcome, exit_code)?],
        observation_digest: digest(b"gc1-t50-observation")?,
        source_binding_digest: digest(b"gc1-t50-source-binding")?,
    })?)
}

pub(super) fn group_entry(
    task: &str,
    digest: &str,
    input_digest: &str,
    planned: &PlannedPlatform,
) -> Result<MatrixEntry, Box<dyn Error>> {
    let execute = ExecuteTaskIds {
        tasks: BTreeMap::from([("group".to_owned(), ExecuteTaskRef::Single(task.to_owned()))]),
    };
    Ok(MatrixEntry::derive(
        "rust",
        task,
        "ci",
        digest,
        json!({}),
        execute,
        input_digest,
        RUN_KEY,
        GROUP_ID,
        planned.clone(),
    )?)
}

pub(super) fn group_obligation(
    task: &str,
    digest: &str,
    input_digest: &str,
    closure_digest: &str,
    decision: ObligationDecision,
) -> PlanObligation {
    PlanObligation {
        task_id: task.to_owned(),
        decision,
        reason: "gc1-t50".to_owned(),
        task_digest: digest.to_owned(),
        input_digest: input_digest.to_owned(),
        closure_digest: closure_digest.to_owned(),
        job_id: GROUP_ID.to_owned(),
    }
}

pub(super) fn branch_plan(
    task: &str,
    entry: MatrixEntry,
    obligation: PlanObligation,
    disposition: JobDisposition,
) -> Result<Plan, Box<dyn Error>> {
    Ok(Plan {
        schema: Plan::SCHEMA,
        run_key: RUN_KEY.to_owned(),
        plan_id: plan_id_for_run(RUN_KEY)?,
        base: None,
        head: HEAD_SHA.to_owned(),
        event: WorkflowEvent::PullRequest,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-22.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: trust_for_event(WorkflowEvent::PullRequest),
        generator: PlanGenerator {
            version: "0.1.55".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "6dd871a3ac5473b6745b77359310384c1932ce0a7f921e06207a99d6710dcd47".to_owned(),
        },
        packages: Vec::new(),
        obligations: vec![obligation],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec![task.to_owned()],
        jobs: vec![PlanJob {
            job_id: GROUP_ID.to_owned(),
            disposition,
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    })
}

pub(super) fn push_plan(plan: &Plan) -> Plan {
    let mut pushed = plan.clone();
    pushed.event = WorkflowEvent::Push;
    pushed.trust = trust_for_event(WorkflowEvent::Push);
    pushed
}
