// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use velnor_model::REUSE_RUN_MAX_BYTES;

use crate::verdict_world::{BASE, HEAD, REPOSITORY, World, jobs, json_body, path, tree};

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

const MOVED: &str = "3333333333333333333333333333333333333333";

#[test]
fn a_pull_request_head_that_moved_since_it_was_queued_is_refused() {
    let mut world = World::new();
    let moved_tree = tree(&world.checkout);
    world.routes.insert(
        path("/pulls/7"),
        json_body(&json!({"head": {"sha": MOVED, "repo": {"full_name": REPOSITORY}}})),
    );
    world.routes.insert(
        path(&format!("/compare/{BASE}...{MOVED}?per_page=1")),
        json_body(&json!({"behind_by": 0})),
    );
    world.routes.insert(
        path(&format!("/git/commits/{MOVED}")),
        json_body(&json!({"tree": {"sha": moved_tree}})),
    );
    let runs = world.routes.remove(&runs_key()).unwrap_or_default();
    world.routes.insert(
        path(&format!(
            "/actions/workflows/ci.yml/runs?head_sha={MOVED}&event=pull_request&per_page=100"
        )),
        String::from_utf8_lossy(&runs)
            .replace(HEAD, MOVED)
            .into_bytes(),
    );
    let result = world.run();
    assert_eq!(result.outputs["reuse"], "false", "{}", result.stdout());
    assert!(
        result.stdout().contains("not the queued commit"),
        "{}",
        result.stdout()
    );
}

#[test]
fn two_jobs_with_the_name_of_a_reusable_job_are_refused() {
    for name in ["rust", "Alint"] {
        let mut world = World::new();
        let rust = (
            "rust",
            "success",
            &[("Format", "success"), ("Test", "success")][..],
        );
        let mut listed = vec![
            ("Required", "success", &[][..]),
            rust,
            ("Alint", "success", &[][..]),
        ];
        listed.push(if name == "rust" {
            ("rust", "skipped", &[("Test", "skipped")][..])
        } else {
            ("Alint", "skipped", &[][..])
        });
        world.routes.insert(jobs_key(100), jobs(&listed));
        let result = world.run();
        assert_eq!(result.outputs["reuse"], "false", "{name}");
        assert!(
            result
                .stdout()
                .contains(&format!("more than one job of the run is named {name}")),
            "{name}: {}",
            result.stdout()
        );
    }
}

#[test]
fn a_run_url_is_accepted_up_to_the_bytes_the_needs_budget_charges_and_no_further() {
    let base = format!("/{REPOSITORY}/actions/runs/100").len();
    for (extra, reuse) in [(0, "true"), (1, "false")] {
        let mut world = World::new();
        let server = format!(
            "https://{}",
            "a".repeat(REUSE_RUN_MAX_BYTES - base - 8 + extra)
        );
        world
            .overrides
            .push(("GITHUB_SERVER_URL".to_owned(), server));
        assert_eq!(world.run().outputs["reuse"], reuse, "{extra}");
    }
}
