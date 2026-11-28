// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use crate::verdict_world::{BASE, HEAD, REPOSITORY, World, jobs, json_body, path};

fn runs_body(ids: &[u64]) -> Vec<u8> {
    let runs: Vec<_> = ids
        .iter()
        .map(|id| {
            json!({"id": id, "event": "pull_request", "status": "completed",
                "head_sha": HEAD, "pull_requests": [{"number": 7}]})
        })
        .collect();
    json_body(&json!({"total_count": runs.len(), "workflow_runs": runs}))
}

fn runs_key() -> String {
    path(&format!(
        "/actions/workflows/ci.yml/runs?head_sha={HEAD}&event=pull_request&per_page=100"
    ))
}

fn jobs_key(run: u64) -> String {
    path(&format!(
        "/actions/runs/{run}/jobs?filter=latest&per_page=100&page=1"
    ))
}

fn pull_of(repository: Option<&str>) -> Vec<u8> {
    json_body(
        &json!({"head": {"sha": HEAD, "repo": repository.map(|name| json!({"full_name": name}))}}),
    )
}

fn leaves_reuse_off(key: &str, bodies: &[Option<&[u8]>]) {
    let mut world = World::new();
    let served = world.routes.clone();
    for body in bodies {
        world.routes = served.clone();
        match body {
            None => {
                world.routes.remove(key);
            }
            Some(bytes) => {
                world.routes.insert(key.to_owned(), bytes.to_vec());
            }
        }
        let result = world.run();
        assert_eq!(result.output.status.code(), Some(0), "{key} {body:?}");
        assert_eq!(
            result.outputs.get("reuse").map(String::as_str),
            Some("false"),
            "{key} {body:?}: {}",
            result.stdout()
        );
    }
}

macro_rules! fails_safe {
    ($($name:ident: $key:expr,)*) => {$(
        mod $name {
            use super::*;

            #[test]
            fn an_absent_or_garbled_answer_leaves_reuse_off() {
                leaves_reuse_off(&$key, &[None, Some(b"{\"garbage")]);
            }

            #[test]
            fn an_empty_object_or_array_leaves_reuse_off() {
                leaves_reuse_off(&$key, &[Some(b"{}"), Some(b"[]")]);
            }
        }
    )*};
}

fails_safe! {
    pull_request: path("/pulls/7"),
    comparison: path(&format!("/compare/{BASE}...{HEAD}?per_page=1")),
    head_commit: path(&format!("/git/commits/{HEAD}")),
    branch_rules: path("/rules/branches/main?per_page=100&page=1"),
    workflow_runs: runs_key(),
    run_jobs: jobs_key(100),
    run_artifacts: path("/actions/runs/100/artifacts?per_page=100&page=1"),
    artifact_zip: path("/actions/artifacts/5000/zip"),
}

#[test]
fn ten_newer_runs_without_a_passed_required_check_hide_an_older_green_run() {
    let mut world = World::new();
    let ids: Vec<u64> = (100..=110).collect();
    world.routes.insert(runs_key(), runs_body(&ids));
    for id in 101..=110 {
        world.routes.insert(
            jobs_key(id),
            jobs(&[("Required (ignored event)", "success", &[])]),
        );
    }
    assert_eq!(world.run().outputs["reuse"], "false");
}

#[test]
fn a_newer_failed_run_of_the_same_head_does_not_hide_an_older_green_one() {
    let mut world = World::new();
    world.routes.insert(runs_key(), runs_body(&[100, 101]));
    world.routes.insert(
        jobs_key(101),
        jobs(&[
            ("Required", "failure", &[]),
            (
                "rust",
                "failure",
                &[("Format", "success"), ("Test", "failure")],
            ),
        ]),
    );
    assert_eq!(world.run().outputs["reuse"], "true");
}

#[test]
fn a_head_repository_that_is_not_exactly_this_one_is_refused() {
    for head in [
        None,
        Some(REPOSITORY.to_uppercase()),
        Some("evil/velnor-new".to_owned()),
    ] {
        let mut world = World::new();
        world
            .routes
            .insert(path("/pulls/7"), pull_of(head.as_deref()));
        assert_eq!(world.run().outputs["reuse"], "false", "{head:?}");
    }
}

#[test]
fn two_live_verdict_artifacts_are_refused() {
    let mut world = World::new();
    world.routes.insert(
        path("/actions/runs/100/artifacts?per_page=100&page=1"),
        json_body(&json!({"total_count": 2, "artifacts": [
            {"id": 5000, "name": "velnor-verdict", "expired": false},
            {"id": 5001, "name": "velnor-verdict", "expired": false}]})),
    );
    assert_eq!(world.run().outputs["reuse"], "false");
}
