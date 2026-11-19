// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::budget::{Population, evaluate};

use crate::budget::{budget, history, jobs, run, sample};

#[test]
fn a_run_without_a_setup_measurement_leaves_the_window_missing_and_not_a_shorter_median() {
    let histories = vec![history(
        Population::MainPush,
        vec![
            sample(3, 100, 100, Some(10)),
            sample(2, 100, 100, None),
            sample(1, 100, 100, Some(10)),
        ],
    )];
    let evaluation = evaluate(&budget(0, 500, 0, 30), &histories);
    assert!(evaluation.breaches.is_empty(), "{:?}", evaluation.breaches);
    assert_eq!(
        evaluation.missing,
        ["the default branch setup seconds of the measured runs is not measured"]
    );
    assert!(!evaluation.passed());
    let complete = vec![history(
        Population::MainPush,
        vec![
            sample(3, 100, 100, Some(10)),
            sample(2, 100, 100, Some(12)),
            sample(1, 100, 100, Some(10)),
        ],
    )];
    assert!(evaluate(&budget(0, 500, 0, 30), &complete).passed());
}

fn full_page(first_id: u64, event: &str, branch: &str) -> Vec<serde_json::Value> {
    (first_id..first_id + 100)
        .map(|id| {
            serde_json::json!({"id": id, "event": event, "run_attempt": 1, "head_branch": branch,
             "status": "completed", "conclusion": "success", "pull_requests": []})
        })
        .collect()
}

fn page_path(page: u32) -> String {
    format!(
        "/repos/tailrocks/velnor-new/actions/workflows/rust-ci.yml/runs?status=success&per_page=100&page={page}"
    )
}

fn sampled(responses: &mut std::collections::BTreeMap<String, String>, run_id: u64, event: &str) {
    let mut body: serde_json::Value =
        serde_json::from_str(&run("2026-11-01T00:00:10Z", "2026-11-01T00:01:10Z"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    body["event"] = event.into();
    responses.insert(
        format!("/repos/tailrocks/velnor-new/actions/runs/{run_id}"),
        body.to_string(),
    );
    responses.insert(
        format!(
            "/repos/tailrocks/velnor-new/actions/runs/{run_id}/attempts/1/jobs?per_page=100&page=1"
        ),
        jobs(
            "2026-11-01T00:00:20Z",
            "2026-11-01T00:00:50Z",
            "2026-11-01T00:00:30Z",
        ),
    );
}

fn budget_with_last(api: &str, name: &str) -> String {
    let dir = crate::support::fresh_dir("velnor-ci-budget", name);
    std::fs::create_dir_all(dir.join(".velnor")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(
        dir.join(".velnor/rust-ci.toml"),
        "[budget]\npr_wall_seconds = 500\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([
            "bench",
            "budget",
            "--repo",
            "tailrocks/velnor-new",
            "--last",
            "1",
        ])
        .current_dir(&dir)
        .env("GITHUB_API_URL", api)
        .env("GITHUB_TOKEN", "recorded")
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn repository_response() -> String {
    serde_json::json!({"id": 1, "default_branch": "main"}).to_string()
}

#[test]
fn the_runs_are_not_paged_past_the_page_that_fills_every_window() {
    let mut first = full_page(1000, "pull_request", "feature");
    first[1] = serde_json::json!({"id": 2000, "event": "push", "run_attempt": 1,
        "head_branch": "main", "status": "completed", "conclusion": "success", "pull_requests": []});
    first[2] = serde_json::json!({"id": 3000, "event": "schedule", "run_attempt": 1,
        "head_branch": "main", "status": "completed", "conclusion": "success", "pull_requests": []});
    let mut responses = std::collections::BTreeMap::new();
    responses.insert(
        "/repos/tailrocks/velnor-new".to_owned(),
        repository_response(),
    );
    responses.insert(
        page_path(1),
        serde_json::json!({"total_count": 500, "workflow_runs": first}).to_string(),
    );
    for (id, event) in [(1000, "pull_request"), (2000, "push"), (3000, "schedule")] {
        sampled(&mut responses, id, event);
    }
    let text = budget_with_last(&crate::io::serve(responses), "enough");
    assert!(
        text.contains("pull request: 1 runs") && !text.contains("404"),
        "{text}"
    );
}

#[test]
fn a_history_with_no_scheduled_runs_is_read_for_a_bounded_number_of_pages() {
    let mut responses = std::collections::BTreeMap::new();
    responses.insert(
        "/repos/tailrocks/velnor-new".to_owned(),
        repository_response(),
    );
    for page in 1..=10_u32 {
        responses.insert(
            page_path(page),
            serde_json::json!({"total_count": 5000,
                "workflow_runs": full_page(u64::from(page) * 1000, "pull_request", "feature")})
            .to_string(),
        );
    }
    sampled(&mut responses, 1000, "pull_request");
    let text = budget_with_last(&crate::io::serve(responses), "bounded");
    assert!(
        text.contains("pull request: 1 runs") && !text.contains("404"),
        "{text}"
    );
}
