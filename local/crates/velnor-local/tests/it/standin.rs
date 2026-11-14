// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use velnor_local::io::api_stub::{Answer, StandIn};

use crate::support::Stub;

#[derive(Debug)]
struct RecordedStandIn {
    answers: BTreeMap<String, Vec<u8>>,
}

impl StandIn for RecordedStandIn {
    fn get<'a>(&'a self, path: &'a str) -> Answer<'a> {
        Box::pin(std::future::ready(self.answers.get(path).cloned()))
    }
}

fn recorded_standin(runs: &Value, jobs: &Value, records: &Value) -> Stub {
    let timeline = records.to_string().into_bytes();
    let timeline_id = jobs[0]["timeLineId"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut answers = BTreeMap::from([
        (
            "/_apis/v1/Message/workflow/runs".to_owned(),
            runs.to_string().into_bytes(),
        ),
        (
            "/_apis/v1/Message?runid=2".to_owned(),
            jobs.to_string().into_bytes(),
        ),
        (
            "/_apis/v1/Message".to_owned(),
            jobs.to_string().into_bytes(),
        ),
        (format!("/_apis/v1/Timeline/{timeline_id}"), timeline),
    ]);
    answers.insert("/_apis/v1/Message?runid=3".to_owned(), b"[]".to_vec());
    for record in records.as_array().into_iter().flatten() {
        if let Some(id) = record["log"]["id"].as_u64() {
            let line = format!("2026-11-13T22:33:00.0000000Z log {id}\n");
            answers.insert(format!("/_apis/v1/Logfiles/{id}"), line.into_bytes());
        }
    }
    Stub::over_standin(Arc::new(RecordedStandIn { answers }))
}

fn recorded_json(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/runner-server")
        .join(format!("{name}.json"));
    serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|e| unreachable!("{e}")))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[tokio::test]
async fn a_recorded_stand_in_run_is_served_as_the_run_jobs_and_log_velnor_ci_collects() {
    let stub = recorded_standin(
        &recorded_json("workflow-runs"),
        &recorded_json("jobs"),
        &recorded_json("timeline"),
    );
    let run = stub
        .call("GET", "/repos/tailrocks/velnor-lab/actions/runs/2")
        .await;
    assert_eq!(run.status, 200);
    let run = run.json();
    assert_eq!(run["name"], "mbx default");
    assert_eq!(run["conclusion"], "success");
    assert_eq!(run["status"], "completed");
    assert_eq!(run["event"], "push");
    assert_eq!(run["run_attempt"], 1);
    assert_eq!(run["run_started_at"], "2026-11-13T22:32:39Z");
    assert_eq!(run["updated_at"], "2026-11-13T22:33:26Z");
    let jobs = stub
        .call(
            "GET",
            "/repos/tailrocks/velnor-lab/actions/runs/2/attempts/1/jobs?per_page=100&page=1",
        )
        .await
        .json();
    assert_eq!(jobs["total_count"], 1);
    let job = &jobs["jobs"][0];
    assert_eq!(job["id"], 2);
    assert_eq!(job["name"], "ci");
    assert_eq!(job["conclusion"], "success");
    assert_eq!(job["started_at"], "2026-11-13T22:32:39Z");
    assert_eq!(job["completed_at"], "2026-11-13T22:33:26Z");
    let steps = job["steps"].as_array().cloned().unwrap_or_default();
    assert_eq!(steps.len(), 14);
    assert_eq!(steps[0]["name"], "Set up job");
    assert_eq!(steps[6]["name"], "Run mbx nextest run --workspace --locked");
    assert_eq!(steps[6]["started_at"], "2026-11-13T22:33:03Z");
    assert_eq!(steps[6]["completed_at"], "2026-11-13T22:33:24Z");
    assert_eq!(steps[13]["name"], "Complete job");
    let log = stub
        .call("GET", "/repos/tailrocks/velnor-lab/actions/jobs/2/logs")
        .await;
    assert_eq!(log.status, 200);
    let expected = (12..=25)
        .map(|id| format!("2026-11-13T22:33:00.0000000Z log {id}\n"))
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(String::from_utf8_lossy(&log.body), expected);
    let missing = stub
        .call("GET", "/repos/tailrocks/velnor-lab/actions/runs/9")
        .await;
    assert_eq!(missing.status, 404);
}

#[tokio::test]
async fn a_run_is_reported_with_its_latest_attempt_and_a_broken_workflow_by_its_file() {
    let mut jobs = recorded_json("jobs");
    let mut second = jobs[0].clone();
    second["attempt"] = 2.into();
    second["requestId"] = 3.into();
    second["result"] = "failed".into();
    jobs.as_array_mut()
        .into_iter()
        .for_each(|list| list.push(second.clone()));
    let mut runs = recorded_json("workflow-runs");
    runs.as_array_mut().into_iter().for_each(|list| {
        list.push(serde_json::json!({"id": 3, "fileName": ".github/workflows/broken.yml", "displayName": null, "result": "failed", "status": "completed"}));
    });
    runs[0]["displayName"] = "warm into main".into();
    let mut records = recorded_json("timeline");
    records[0]["startTime"] = "2026-11-13T22:32:3\u{e9}".into();
    if let Some(list) = records.as_array_mut() {
        list.push(serde_json::json!({
            "id": "inner", "parentId": "dd54b3f9-5650-4957-b8b2-3e0ad80c3ea5",
            "name": "Run cargo nextest", "log": {"id": 99}
        }));
    }
    let stub = recorded_standin(&runs, &jobs, &records);
    let run = stub
        .call("GET", "/repos/tailrocks/velnor-lab/actions/runs/2")
        .await
        .json();
    assert_eq!(run["run_attempt"], 2);
    assert_eq!(run["name"], "mbx default");
    assert_eq!(run["display_title"], "warm into main");
    assert_eq!(run["run_started_at"], "2026-11-13T22:32:3\u{e9}Z");
    let log = stub
        .call("GET", "/repos/tailrocks/velnor-lab/actions/jobs/2/logs")
        .await;
    let log = String::from_utf8_lossy(&log.body).into_owned();
    assert!(log.contains(
        "log 18\n2026-11-13T22:33:00.0000000Z log 99\n2026-11-13T22:33:00.0000000Z log 19\n"
    ));
    let latest = stub
        .call(
            "GET",
            "/repos/tailrocks/velnor-lab/actions/runs/2/attempts/2/jobs",
        )
        .await
        .json();
    assert_eq!(latest["total_count"], 1);
    assert_eq!(latest["jobs"][0]["id"], 3);
    assert_eq!(latest["jobs"][0]["conclusion"], "failure");
    let listed = stub
        .call("GET", "/repos/tailrocks/velnor-lab/actions/runs/2/jobs")
        .await
        .json();
    assert_eq!(listed, latest);
    let broken = stub
        .call("GET", "/repos/tailrocks/velnor-lab/actions/runs/3")
        .await
        .json();
    assert_eq!(broken["name"], ".github/workflows/broken.yml");
    assert_eq!(broken["conclusion"], "failure");
    let jobless = stub
        .call("GET", "/repos/tailrocks/velnor-lab/actions/runs/3/jobs")
        .await;
    assert_eq!(jobless.status, 200);
    assert_eq!(jobless.json()["total_count"], 0);
}
