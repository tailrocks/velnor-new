// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::gate::{GateMode, Verdict, release_gate_with};
use velnor_ci::github::{run_jobs_path, runs_path};

use super::support::{Recorded, absent, each};
use super::{GREEN, GREEN_CI};
use crate::support::{FakeGitHub, repo};

const OWN: u64 = 777;
const WORKFLOW: u64 = 9000;
const GATE: &str = "image-gate";
const PUBLISH: &str = "image-svc-publish";

fn in_the_gates_workflow() -> Recorded {
    Recorded::new()
        .green_run(|run| run["workflow_id"] = json!(WORKFLOW))
        .edit(&runs_path(&repo(), GREEN, 1), |runs| {
            let mut own = each(&mut runs["workflow_runs"])
                .find(|run| run["id"] == json!(GREEN_CI))
                .map_or(Value::Null, |run| run.clone());
            own["id"] = json!(OWN);
            own["status"] = json!("in_progress");
            own["conclusion"] = Value::Null;
            if let Some(list) = runs["workflow_runs"].as_array_mut() {
                list.push(own);
                runs["total_count"] = json!(list.len());
            }
        })
        .set(
            run_jobs_path(&repo(), OWN, 1),
            json!({"total_count": 1, "jobs": [{"name": GATE, "status": "in_progress", "conclusion": null}]}),
        )
}

fn with_job(recorded: Recorded, name: &str, status: &str, conclusion: &Value) -> Recorded {
    recorded.edit(&run_jobs_path(&repo(), GREEN_CI, 1), |jobs| {
        if let Some(list) = jobs["jobs"].as_array_mut() {
            list.push(json!({"name": name, "status": status, "conclusion": conclusion}));
            jobs["total_count"] = json!(list.len());
        }
    })
}

fn gate(
    recorded: &Recorded,
    ignore: &[String],
    published: Option<&str>,
) -> Result<Verdict, String> {
    let mut fake = FakeGitHub::default();
    for (path, body) in &recorded.0 {
        fake.respond(path.clone(), &body.to_string());
    }
    release_gate_with(
        &fake,
        &repo(),
        GREEN,
        &[],
        Some(OWN),
        GateMode {
            ignore_jobs: ignore,
            require_published: published,
            ..GateMode::RELEASE
        },
    )
    .map_err(|error| error.to_string())
}

fn ignoring_the_gate() -> Vec<String> {
    vec![GATE.to_owned()]
}

#[test]
fn a_required_job_of_the_gates_own_workflow_counts_through_its_completed_sibling_run() {
    let recorded = in_the_gates_workflow();
    assert_eq!(gate(&recorded, &[], None), Err(absent("Required")));
    let said = gate(&recorded, &ignoring_the_gate(), None);
    assert!(said.as_ref().is_ok_and(|verdict| verdict.ready), "{said:?}");
}

#[test]
fn a_running_job_of_the_workflow_is_waited_for_and_the_gate_itself_is_not() {
    let running = in_the_gates_workflow()
        .green_run(|run| run["status"] = json!("in_progress"))
        .green_job("Required", |job| {
            job["status"] = json!("in_progress");
            job["conclusion"] = Value::Null;
        });
    let said = gate(&running, &ignoring_the_gate(), None);
    assert!(said.is_ok_and(|verdict| !verdict.ready));
}

#[test]
fn the_push_run_must_have_finished_the_publish_job_before_a_release_is_authorized() {
    let published = with_job(
        in_the_gates_workflow(),
        PUBLISH,
        "completed",
        &json!("success"),
    );
    let said = gate(&published, &ignoring_the_gate(), Some(PUBLISH));
    assert!(said.as_ref().is_ok_and(|verdict| verdict.ready), "{said:?}");
    let running = with_job(
        in_the_gates_workflow(),
        PUBLISH,
        "in_progress",
        &Value::Null,
    );
    let said = gate(&running, &ignoring_the_gate(), Some(PUBLISH));
    assert!(said.is_ok_and(|verdict| !verdict.ready));
    let cancelled = with_job(
        in_the_gates_workflow(),
        PUBLISH,
        "completed",
        &json!("cancelled"),
    );
    let said = gate(&cancelled, &ignoring_the_gate(), Some(PUBLISH));
    assert!(said.is_err_and(|text| text.contains("was not published")));
    let missing = gate(
        &in_the_gates_workflow(),
        &ignoring_the_gate(),
        Some(PUBLISH),
    );
    assert!(missing.is_err_and(|text| text.contains("was not published")));
}
