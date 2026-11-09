// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

use super::{Job, Run, validate_release_jobs};

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const Q: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fixture() -> (Value, Value) {
    (
        json!({"id": 100, "run_attempt": 2, "event": "push", "path": ".github/workflows/product-release-images.yml", "head_branch": "main", "head_sha": SOURCE, "head_repository": {"id": 1_390_620_900_u64, "full_name": "tailrocks/velnor-new"}, "status": "in_progress", "conclusion": null, "referenced_workflows": [{"path": format!("tailrocks/velnor-new/.github/workflows/source-preview-mac-proof.yml@{Q}"), "ref": Q, "sha": Q}]}),
        json!([{ "id": 1, "run_id": 100, "head_sha": SOURCE, "name": "publish-images", "status": "completed", "conclusion": "success" }, { "id": 2, "run_id": 100, "run_attempt": 2, "head_sha": SOURCE, "name": "arbitrary server job display", "check_run_url": "https://api.github.com/repos/tailrocks/velnor-new/check-runs/55", "status": "in_progress", "conclusion": null }]),
    )
}

fn accepted(run: Value, jobs: Value) -> bool {
    let run: Run = serde_json::from_value(run).expect("valid run fixture");
    let jobs: Vec<Job> = serde_json::from_value(jobs).expect("valid jobs fixture");
    validate_release_jobs(SOURCE, 100, 2, &jobs, Some((Q, 55, &run))).is_ok()
}

#[test]
fn q_job_requires_matching_server_run_and_job_identity() {
    let (run, jobs) = fixture();
    assert!(accepted(run.clone(), jobs.clone()));
    let mut without_ref = run.clone();
    without_ref["referenced_workflows"][0]["ref"] = Value::Null;
    assert!(accepted(without_ref, jobs.clone()));
    for (path, value) in [
        ("/id", json!(101)),
        ("/run_attempt", json!(1)),
        ("/event", json!("workflow_dispatch")),
        ("/path", json!(".github/workflows/other.yml")),
        ("/head_branch", json!("other")),
        ("/head_sha", json!("different")),
        ("/head_repository/id", json!(1)),
        ("/head_repository/full_name", json!("other/repo")),
        ("/head_repository", Value::Null),
        ("/status", json!("completed")),
        ("/conclusion", json!("success")),
        ("/referenced_workflows/0/path", json!("other")),
        ("/referenced_workflows/0/ref", json!("other")),
        ("/referenced_workflows/0/sha", json!("other")),
        ("/referenced_workflows", json!([])),
    ] {
        let mut invalid = run.clone();
        *invalid.pointer_mut(path).expect("run field") = value;
        assert!(
            !accepted(invalid, jobs.clone()),
            "accepted invalid run {path}"
        );
    }
    let mut duplicate = run.clone();
    duplicate["referenced_workflows"] = json!([
        run["referenced_workflows"][0],
        run["referenced_workflows"][0]
    ]);
    assert!(
        !accepted(duplicate, jobs.clone()),
        "accepted duplicate referenced workflow"
    );
    for (path, value) in [
        ("/0/name", json!("other")),
        ("/0/run_id", json!(101)),
        ("/0/head_sha", json!("other")),
        ("/0/status", json!("in_progress")),
        ("/0/conclusion", json!("failure")),
        (
            "/1/check_run_url",
            json!("https://api.github.com/repos/tailrocks/velnor-new/check-runs/56"),
        ),
        ("/1/run_id", json!(101)),
        ("/1/run_attempt", json!(1)),
        ("/1/head_sha", json!("other")),
        ("/1/status", json!("completed")),
        ("/1/conclusion", json!("success")),
    ] {
        let mut invalid = jobs.clone();
        *invalid.pointer_mut(path).expect("job field") = value;
        assert!(
            !accepted(run.clone(), invalid),
            "accepted invalid job {path}"
        );
    }
    let mut duplicate_publisher = jobs.clone();
    duplicate_publisher
        .as_array_mut()
        .expect("jobs")
        .push(jobs[0].clone());
    assert!(
        !accepted(run.clone(), duplicate_publisher),
        "accepted duplicate publisher"
    );
    let mut duplicate = jobs.clone();
    duplicate
        .as_array_mut()
        .expect("jobs")
        .push(jobs[1].clone());
    assert!(!accepted(run, duplicate), "accepted duplicate check run");
}
