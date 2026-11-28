// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use crate::resolve_wire::zip_of;
use crate::verdict_world::{
    BASE, HEAD, PLAN, World, checkout, jobs, jobs_on, json_body, path, recorded, tree,
};

const RUN_URL: &str = "https://github.com/tailrocks/velnor-new/actions/runs/100";

fn refused(world: &World, reason: &str) {
    let result = world.run();
    assert_eq!(result.output.status.code(), Some(0), "{:?}", result.output);
    assert_eq!(result.outputs["reuse"], "false", "{}", result.stdout());
    assert_eq!(result.outputs["reuse_run"], "");
    assert_eq!(result.outputs["reused"], "");
    let stdout = result.stdout();
    let line = stdout
        .lines()
        .find(|line| line.starts_with("velnor: no verdict for this key ("))
        .unwrap_or_default();
    assert!(line.contains(reason), "{reason}: {stdout}");
}

fn artifact_zip(world: &mut World, bytes: Vec<u8>) {
    world
        .routes
        .insert(path("/actions/artifacts/5000/zip"), bytes);
}

#[test]
fn a_green_pull_request_run_of_the_same_tree_and_plan_is_reused() {
    let result = World::new().run();
    assert_eq!(result.outputs["reuse"], "true", "{}", result.stdout());
    assert_eq!(result.outputs["reuse_run"], RUN_URL);
    assert_eq!(result.reused(), ["lint-alint", "rust.format", "rust.test"]);
    assert!(
        result
            .stdout()
            .contains(&format!("velnor: reusing verdict of {RUN_URL}"))
    );
}

#[test]
fn the_queue_ref_ends_in_the_base_commit_and_the_pull_request_head_comes_from_the_pull_request() {
    assert_ne!(BASE, HEAD);
    let result = World::new().run();
    assert_eq!(result.outputs["reuse"], "true", "{}", result.stdout());
    for read in [
        path("/pulls/7"),
        path(&format!("/compare/{BASE}...{HEAD}?per_page=1")),
    ] {
        assert!(
            result.requests.contains(&read),
            "{read}: {:?}",
            result.requests
        );
    }
}

#[test]
fn only_what_the_pull_request_run_passed_is_reused() {
    let mut world = World::new();
    world.routes.insert(
        path("/actions/runs/100/jobs?filter=latest&per_page=100&page=1"),
        jobs(&[
            ("Required", "success", &[]),
            (
                "rust",
                "success",
                &[("Format", "success"), ("Test", "skipped")],
            ),
            ("Alint", "skipped", &[]),
        ]),
    );
    let result = world.run();
    assert_eq!(result.outputs["reuse"], "true", "{}", result.stdout());
    assert_eq!(result.reused(), ["rust.format"]);
}

#[test]
fn nothing_passed_means_nothing_is_reused() {
    let mut world = World::new();
    world.routes.insert(
        path("/actions/runs/100/jobs?filter=latest&per_page=100&page=1"),
        jobs(&[
            ("Required", "success", &[]),
            ("rust", "success", &[("Test", "skipped")]),
        ]),
    );
    refused(&world, "no reusable step or job passed in that run");
}

#[test]
fn a_tree_other_than_the_pull_request_head_is_refused() {
    let mut world = World::new();
    world.routes.insert(
        path(&format!("/git/commits/{HEAD}")),
        json_body(&json!({"tree": {"sha": "f".repeat(40)}})),
    );
    refused(&world, "the pull request head has tree");
}

#[test]
fn a_pull_request_behind_the_queue_base_is_refused() {
    let mut world = World::new();
    world.routes.insert(
        path(&format!("/compare/{BASE}...{HEAD}?per_page=1")),
        json_body(&json!({"behind_by": 1})),
    );
    refused(&world, "the pull request head is 1 commits behind");
}

#[test]
fn a_default_branch_that_allows_force_pushes_is_refused() {
    let mut world = World::new();
    world.routes.insert(
        path("/rules/branches/main?per_page=100&page=1"),
        json_body(&json!([{"type": "merge_queue"}])),
    );
    refused(&world, "no active non_fast_forward rule");
}

#[test]
fn a_pull_request_from_a_fork_is_refused() {
    let mut world = World::new();
    world.routes.insert(
        path("/pulls/7"),
        json_body(&json!({"head": {"sha": HEAD, "repo": {"full_name": "someone/velnor-new"}}})),
    );
    refused(&world, "the pull request is not from this repository");
}

#[test]
fn a_run_whose_required_check_failed_is_refused() {
    let mut world = World::new();
    world.routes.insert(
        path("/actions/runs/100/jobs?filter=latest&per_page=100&page=1"),
        jobs(&[("Required", "failure", &[]), ("Alint", "success", &[])]),
    );
    refused(&world, "passed its Required check");
}

#[test]
fn a_run_of_another_pull_request_is_refused() {
    let mut world = World::new();
    world.routes.insert(
        path(&format!(
            "/actions/workflows/ci.yml/runs?head_sha={HEAD}&event=pull_request&per_page=100"
        )),
        json_body(&json!({"total_count": 1, "workflow_runs": [{
            "id": 100, "event": "pull_request", "status": "completed",
            "head_sha": HEAD, "pull_requests": [{"number": 8}]}]})),
    );
    refused(&world, "passed its Required check");
}

#[test]
fn a_verdict_that_is_missing_or_unreadable_is_refused() {
    let mut missing = World::new();
    missing.routes.insert(
        path("/actions/runs/100/artifacts?per_page=100&page=1"),
        json_body(&json!({"total_count": 0, "artifacts": []})),
    );
    refused(&missing, "the run has no usable velnor-verdict");
    let mut garbage = World::new();
    artifact_zip(&mut garbage, zip_of("verdict.json", b"not json"));
    refused(&garbage, "the run has no usable velnor-verdict");
    let mut other_format = World::new();
    let format_one = format!(
        r#"{{"format":1,"attempt":1,"tree":"{}","plan":{PLAN}}}"#,
        tree(&other_format.checkout)
    );
    artifact_zip(
        &mut other_format,
        zip_of("verdict.json", format_one.as_bytes()),
    );
    refused(&other_format, "format 1 is not 2");
}

#[test]
fn a_verdict_of_another_tree_is_refused() {
    let mut world = World::new();
    let elsewhere = checkout("other content");
    artifact_zip(
        &mut world,
        zip_of("verdict.json", recorded(&elsewhere, PLAN).as_bytes()),
    );
    refused(&world, "the verdict is for another tree");
}

#[test]
fn a_verdict_of_another_plan_is_refused() {
    let mut world = World::new();
    world.plan = r#"{"rust_root_mode":"all","rust_root_tests":"run"}"#.to_owned();
    refused(&world, "the verdict is for another plan");
}

#[test]
fn anything_the_lookup_cannot_read_is_refused_without_failing_the_step() {
    let mut world = World::new();
    world.routes.remove(&path("/pulls/7"));
    refused(&world, "cannot read the pull request");
}

#[test]
fn an_event_other_than_merge_group_never_reuses() {
    for event in ["push", "pull_request", "Merge_Group"] {
        let mut other = World::new();
        other.event = event.to_owned();
        refused(&other, &format!("the event is {event}, not merge_group"));
    }
}

#[test]
fn a_queue_entry_is_judged_by_the_rules_of_its_own_target_branch() {
    let head_ref = format!("gh-readonly-queue/release/1.x/pr-7-{BASE}");
    let mut released = World::new();
    released
        .overrides
        .push(("VELNOR_MERGE_GROUP_HEAD_REF".to_owned(), head_ref.clone()));
    released.routes.insert(
        path("/rules/branches/release%2F1.x?per_page=100&page=1"),
        json_body(&json!([{"type": "non_fast_forward"}])),
    );
    released.routes.insert(
        path("/rules/branches/main?per_page=100&page=1"),
        json_body(&json!([])),
    );
    let result = released.run();
    assert_eq!(result.outputs["reuse"], "true", "{}", result.stdout());
    let mut open = World::new();
    open.overrides
        .push(("VELNOR_MERGE_GROUP_HEAD_REF".to_owned(), head_ref));
    open.routes.insert(
        path("/rules/branches/release%2F1.x?per_page=100&page=1"),
        json_body(&json!([])),
    );
    refused(&open, "release/1.x has no active non_fast_forward rule");
}

#[test]
fn a_verdict_of_an_older_attempt_than_the_latest_jobs_is_refused() {
    let mut world = World::new();
    world.routes.insert(
        path("/actions/runs/100/jobs?filter=latest&per_page=100&page=1"),
        jobs_on(
            &[("Required", "success", &[]), ("Alint", "success", &[])],
            2,
        ),
    );
    refused(
        &world,
        "the verdict is of attempt 1, the run's latest attempt is 2",
    );
}

fn jobs_route() -> String {
    path("/actions/runs/100/jobs?filter=latest&per_page=100&page=1")
}

#[test]
fn a_jobs_listing_without_run_attempt_is_refused() {
    let mut world = World::new();
    world.routes.insert(
        jobs_route(),
        json_body(&json!({"total_count": 1, "jobs": [
            {"id": 2, "name": "Alint", "conclusion": "success",
             "created_at": "2026-11-20T00:00:00Z", "steps": []}]})),
    );
    refused(&world, "cannot read jobs");
}

#[test]
fn a_latest_attempt_of_zero_is_refused_even_for_a_verdict_of_attempt_zero() {
    let mut world = World::new();
    let verdict = json!({"format": 2, "attempt": 0, "tree": tree(&world.checkout),
        "plan": serde_json::from_str::<serde_json::Value>(PLAN).unwrap_or_default()});
    artifact_zip(
        &mut world,
        zip_of("verdict.json", verdict.to_string().as_bytes()),
    );
    world.routes.insert(
        jobs_route(),
        jobs_on(
            &[("Alint", "success", &[]), ("Required", "success", &[])],
            0,
        ),
    );
    refused(&world, "the run's jobs report no run attempt");
}

#[test]
fn a_reused_job_with_two_steps_of_one_name_is_refused() {
    for second in ["skipped", "success"] {
        let mut world = World::new();
        world.routes.insert(
            jobs_route(),
            jobs(&[
                ("Required", "success", &[]),
                ("rust", "success", &[("Test", "success"), ("Test", second)]),
            ]),
        );
        refused(&world, "job rust has more than one step named Test");
    }
}

#[test]
fn a_repeated_step_name_in_a_job_nothing_reuses_changes_nothing() {
    let mut world = World::new();
    world.routes.insert(
        jobs_route(),
        jobs(&[
            ("Required", "success", &[]),
            ("Alint", "success", &[]),
            (
                "other",
                "success",
                &[("Test", "success"), ("Test", "skipped")],
            ),
        ]),
    );
    let result = world.run();
    assert_eq!(result.outputs["reuse"], "true", "{}", result.stdout());
    assert_eq!(result.reused(), ["lint-alint"]);
}

#[test]
fn record_writes_the_attempt_tree_and_plan_and_is_repeatable() {
    let world = World::new();
    let first = recorded(&world.checkout, PLAN);
    let again = recorded(&world.checkout, PLAN);
    assert_eq!(first, again);
    let parsed: serde_json::Value =
        serde_json::from_str(&first).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(parsed["format"], 2);
    assert_eq!(parsed["attempt"], 1);
    assert_eq!(parsed["tree"], crate::verdict_world::tree(&world.checkout));
    assert_eq!(parsed["plan"]["rust_root_mode"], "some");
    assert!(parsed.get("runner").is_none());
    let temp = world.checkout.root().join("runner-temp/velnor-verdict");
    let files: Vec<_> = std::fs::read_dir(&temp)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(files, ["verdict.json"]);
}
