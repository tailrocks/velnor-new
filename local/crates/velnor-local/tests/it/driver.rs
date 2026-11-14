// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde_json::Value;
use velnor_local::event::{NO_COMMIT, PullRequest, Push, Repository};
use velnor_local::rest::runs;
use velnor_local::scenario::{Scenario, Step, overlapping};
use velnor_local::standin::{self, Job, Record, Run};

const SHA_A: &str = "007c9fa8aac37d2aa7bc4a26876bf2654699f6bc";
const SHA_B: &str = "b151fd5994a1e3e2b2ba84bf7d3c3d1e5a5e4f60";

fn read(relative: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    std::fs::read(&path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

fn kind(value: Option<&Value>) -> &'static str {
    match value {
        None => "absent",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
        Some(Value::String(text))
            if text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            "sha"
        }
        Some(Value::String(_)) => "string",
    }
}

fn field<'a>(payload: &'a Value, dotted: &str) -> Option<&'a Value> {
    dotted
        .split('.')
        .try_fold(payload, |value, name| value.get(name))
}

fn assert_like_github(leg: &str, payload: &Value) {
    let observed: Value = serde_json::from_slice(&read("conformance/event-payload.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let leg = &observed["legs"][leg];
    let fields = leg["fields"].as_object().into_iter().flatten();
    for (name, expected) in fields {
        assert_eq!(
            kind(field(payload, name)),
            expected.as_str().unwrap_or_default(),
            "{name}"
        );
    }
    let mut top_level: Vec<&str> = payload
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys().map(String::as_str))
        .collect();
    top_level.sort_unstable();
    let expected: Vec<&str> = leg["top_level"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(top_level, expected);
}

const REPOSITORY: Repository<'static> = Repository {
    full_name: "tailrocks/velnor-lab",
    default_branch: "main",
};

#[test]
fn a_push_payload_has_the_shape_github_sent_in_the_probe() {
    let payload = REPOSITORY.push(&Push {
        branch: "main",
        before: NO_COMMIT,
        after: SHA_A,
    });
    assert_like_github("push", &payload);
    assert_eq!(payload["ref"], "refs/heads/main");
    assert_eq!(payload["created"], true);
}

#[test]
fn a_pull_request_payload_has_the_shape_github_sent_in_the_probe() {
    let payload = REPOSITORY.pull_request(&PullRequest {
        number: 3,
        base: ("main", SHA_A),
        head: ("warm", SHA_B),
        changed_files: 1,
    });
    assert_like_github("pull_request", &payload);
    assert_eq!(payload["pull_request"]["base"]["sha"], SHA_A);
    assert_eq!(payload["pull_request"]["head"]["ref"], "warm");
    assert_eq!(payload["number"], 3);
}

#[test]
fn the_warm_scenario_follows_the_default_branch_and_unknown_steps_option_values_and_escaping_paths_are_refused()
 {
    let warm = Scenario::parse(&read("scenarios/warm.json"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .on_default_branch("trunk");
    assert_eq!(
        warm.steps,
        [
            Step::Push("trunk".to_owned()),
            Step::Commit {
                branch: "warm".to_owned(),
                append: "rust-toolchain.toml".to_owned(),
                line: "# velnor-local warm".to_owned(),
            },
            Step::OpenPullRequest {
                head: "warm".to_owned(),
                base: "trunk".to_owned(),
            },
        ]
    );
    let unknown = br#"{"description": "x", "steps": [{"rebase": "main"}]}"#;
    assert!(Scenario::parse(unknown).is_err());
    assert!(
        warm.refuse_options(["tailrocks/velnor-lab", "main"])
            .is_ok()
    );
    assert!(warm.refuse_options(["--default-branch=x"]).is_err());
    let option = br#"{"description": "x", "steps": [{"push": "--upload-pack=x"}]}"#;
    let option = Scenario::parse(option).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(option.refuse_options(["main"]).is_err());
    let escape = br#"{"description": "x", "steps": [{"commit": {"branch": "b", "append": "../outside", "line": "x"}}]}"#;
    let escape = Scenario::parse(escape).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(escape.refuse_options(["main"]).is_err());
}

fn recorded<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_slice(&read(&format!("testdata/runner-server/{name}.json")))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_recorded_stand_in_run_reads_as_the_github_run_jobs_and_log_velnor_ci_collects() {
    let runs: Vec<Run> = recorded("workflow-runs");
    let jobs: Vec<Job> = recorded("jobs");
    let records: Vec<Record> = recorded("timeline");
    let job = runs::job(&jobs[0], &records);
    assert_eq!(job["id"], 2);
    assert_eq!(job["name"], "ci");
    assert_eq!(job["conclusion"], "success");
    assert_eq!(job["started_at"], "2026-11-13T22:32:39Z");
    assert_eq!(job["completed_at"], "2026-11-13T22:33:26Z");
    let steps = job["steps"].as_array().cloned().unwrap_or_default();
    let names: Vec<&str> = steps
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect();
    assert_eq!(names.len(), 14);
    assert_eq!(names.first(), Some(&"Set up job"));
    assert_eq!(
        names.get(6),
        Some(&"Run mbx nextest run --workspace --locked")
    );
    assert_eq!(names.last(), Some(&"Complete job"));
    assert_eq!(steps[6]["started_at"], "2026-11-13T22:33:03Z");
    assert_eq!(steps[6]["completed_at"], "2026-11-13T22:33:24Z");
    let titled: Run = serde_json::from_value(serde_json::json!({
        "id": 2, "fileName": ".github/workflows/mbx-default.yml", "displayName": "warm into main", "eventName": "pull_request",
        "sha": SHA_A, "result": "succeeded", "status": "completed",
    }))
    .unwrap_or_else(|e| unreachable!("{e}"));
    let pull = runs::run(&titled, std::slice::from_ref(&job));
    assert_eq!(pull["name"], "mbx default");
    assert_eq!(pull["display_title"], "warm into main");
    let invalid: Run = serde_json::from_value(serde_json::json!({
        "id": 3, "fileName": ".github/workflows/broken.yml", "displayName": null,
        "result": "failed", "status": "completed",
    }))
    .unwrap_or_else(|e| unreachable!("{e}"));
    let invalid = runs::run(&invalid, &[]);
    assert_eq!(invalid["name"], ".github/workflows/broken.yml");
    assert_eq!(invalid["conclusion"], "failure");
    let run = runs::run(&runs[0], &[job]);
    assert_eq!(run["name"], "mbx default");
    assert_eq!(run["conclusion"], "success");
    assert_eq!(run["status"], "completed");
    assert_eq!(run["event"], "push");
    assert_eq!(run["head_sha"], SHA_A);
    assert_eq!(run["run_started_at"], "2026-11-13T22:32:39Z");
    assert_eq!(run["updated_at"], "2026-11-13T22:33:26Z");
    let started = run["run_started_at"].as_str().and_then(standin::seconds);
    assert_eq!(started, Some(1_794_609_159));
    assert_eq!(
        standin::conclusion(runs[1].result.as_deref()),
        Some("failure")
    );
    assert_eq!(runs::log_ids(&records), (12..=25).collect::<Vec<u64>>());
}

#[test]
fn a_state_directory_that_overlaps_the_checkout_is_refused() {
    let checkout = Path::new("/work/lab");
    assert!(overlapping(Path::new("/work/lab/state"), checkout));
    assert!(overlapping(Path::new("/work"), checkout));
    assert!(overlapping(checkout, checkout));
    assert!(!overlapping(Path::new("/work/lab-state"), checkout));
    assert!(!overlapping(Path::new("/tmp/state"), checkout));
}
