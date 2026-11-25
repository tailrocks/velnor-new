// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::github::{run_jobs_path, runs_path};

use super::support::{Recorded, absent, each};
use super::{GREEN, GREEN_CI};
use crate::support::repo;

const OWN: u64 = 777;
const SIBLING: u64 = 778;
const WORKFLOW: u64 = 9000;

fn with_runs_of_the_gate_workflow(recorded: Recorded, ids: &[u64]) -> Recorded {
    let mut recorded = recorded.green_run(|run| run["workflow_id"] = json!(1));
    for id in ids {
        recorded = recorded
            .edit(&runs_path(&repo(), GREEN, 1), |runs| {
                let template = each(&mut runs["workflow_runs"])
                    .find(|run| run["id"] == json!(GREEN_CI))
                    .map_or(Value::Null, |run| run.clone());
                let mut own = template;
                own["id"] = json!(id);
                own["status"] = json!("in_progress");
                own["workflow_id"] = json!(WORKFLOW);
                if let Some(list) = runs["workflow_runs"].as_array_mut() {
                    list.push(own);
                    runs["total_count"] = json!(list.len());
                }
            })
            .set(
                run_jobs_path(&repo(), *id, 1),
                json!({"total_count": 0, "jobs": []}),
            );
    }
    recorded
}

#[test]
fn a_commit_whose_only_running_push_run_is_the_gate_itself_is_ready() {
    let recorded = with_runs_of_the_gate_workflow(Recorded::new(), &[OWN]);
    let said = recorded.gate_in_run(GREEN, &[], Some(OWN));
    assert!(said.as_ref().is_ok_and(|verdict| verdict.ready), "{said:?}");
}

#[test]
fn the_gate_run_does_not_make_a_check_that_no_run_reports_look_like_one_that_has_not_started() {
    let recorded = with_runs_of_the_gate_workflow(Recorded::new(), &[OWN])
        .green_job("Required", |job| job["name"] = json!("Other"));
    assert_eq!(
        recorded.gate_in_run(GREEN, &[], Some(OWN)),
        Err(absent("Required"))
    );
    let waiting = recorded.gate_in_run(GREEN, &[], None);
    assert!(
        waiting.is_ok_and(|verdict| !verdict.ready),
        "a run that is not the gate still counts as running"
    );
}

#[test]
fn another_run_of_the_gate_workflow_for_the_same_commit_is_not_evidence_either() {
    let recorded = with_runs_of_the_gate_workflow(Recorded::new(), &[OWN, SIBLING])
        .green_job("Required", |job| job["name"] = json!("Other"));
    assert_eq!(
        recorded.gate_in_run(GREEN, &[], Some(OWN)),
        Err(absent("Required"))
    );
}
