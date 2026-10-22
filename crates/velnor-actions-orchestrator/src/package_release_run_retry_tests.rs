use super::inventory::{
    EMPTY_WORKFLOW_RUN_ATTEMPTS, MAX_WORKFLOW_RUN_PAGES, WORKFLOW_RUN_PAGE_SIZE,
    retry_empty_workflow_run_inventory, sleep_for_retry,
};
use super::*;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};
use velnor_actions_mise::CheckDeadline;

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn source() -> RunSource<'static> {
    RunSource {
        repository_id: 9,
        sha: SOURCE_SHA,
        branch: "main",
    }
}

fn workflow() -> WorkflowMetadata {
    WorkflowMetadata {
        id: 7,
        name: "CI".into(),
        path: ".github/workflows/ci.yml".into(),
        state: "active".into(),
    }
}

fn run_value(id: u64, run_number: u64) -> Value {
    json!({
        "id": id,
        "run_attempt": 1,
        "run_number": run_number,
        "workflow_id": 7,
        "name": "CI",
        "path": ".github/workflows/ci.yml",
        "head_sha": SOURCE_SHA,
        "head_branch": "main",
        "event": "push",
        "status": "completed",
        "conclusion": "success",
        "repository": {"id": 9, "full_name": "example/app"},
        "head_repository": {"id": 9, "full_name": "example/app"}
    })
}

fn run(id: u64, run_number: u64) -> WorkflowRunResponse {
    serde_json::from_value(run_value(id, run_number)).expect("workflow run fixture")
}

fn route(path: &str) -> String {
    format!("repos/example/app/{path}")
}

fn list_route() -> String {
    route(&format!(
        "actions/workflows/7/runs?head_sha={SOURCE_SHA}&per_page=100&page=1"
    ))
}

fn check() -> Result<(), OrchestratorError> {
    CheckDeadline::check_scope().map_err(|_| internal("package_ci_run_deadline"))
}

#[test]
fn empty_workflow_run_inventory_retries_six_times_with_bounded_backoff() {
    let mut reads = 0;
    let mut waits = Vec::new();
    let inventory = retry_empty_workflow_run_inventory(
        |page| {
            assert_eq!(page, 1);
            reads += 1;
            Ok(Vec::new())
        },
        |duration| {
            waits.push(duration);
            Ok(())
        },
        check,
    )
    .expect("empty inventory is an optional absence after bounded retries");

    assert!(inventory.is_none());
    assert_eq!(reads, 6);
    assert_eq!(waits, [3, 6, 12, 24, 45].map(Duration::from_secs));
}

#[test]
fn empty_inventory_retries_then_collects_every_page_of_first_nonempty_attempt() {
    let mut reads = 0;
    let mut waits = Vec::new();
    let inventory = retry_empty_workflow_run_inventory(
        |page| {
            reads += 1;
            match reads {
                1 => {
                    assert_eq!(page, 1);
                    Ok(Vec::new())
                }
                2 => {
                    assert_eq!(page, 1);
                    Ok((1..=WORKFLOW_RUN_PAGE_SIZE)
                        .map(|id| {
                            let id = u64::try_from(id).expect("bounded run id");
                            run(id, id)
                        })
                        .collect())
                }
                3 => {
                    assert_eq!(page, 2);
                    Ok(vec![run(101, 101)])
                }
                _ => panic!("nonempty inventory must not retry"),
            }
        },
        |duration| {
            waits.push(duration);
            Ok(())
        },
        check,
    )
    .expect("nonempty inventory is returned");

    let inventory = inventory.expect("second attempt returned runs");
    assert_eq!(inventory.len(), 101);
    assert_eq!(reads, 3);
    assert_eq!(waits, [Duration::from_secs(3)]);
}

#[test]
fn nonempty_mismatched_run_is_terminal_and_queries_only_head_sha() {
    let mut foreign = run_value(22, 12);
    foreign["repository"]["full_name"] = json!("foreign/app");
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(list_route(), json!({"workflow_runs": [foreign]}))],
    );

    assert!(
        api.latest_source_ci_optional_with_sleep(&workflow(), source(), |_| Ok(()))
            .is_err()
    );
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 1);
}

#[test]
fn a_manual_run_in_the_same_sha_inventory_does_not_hide_the_push_run() {
    let mut manual = run_value(23, 13);
    manual["event"] = json!("workflow_dispatch");
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(
            list_route(),
            json!({"workflow_runs": [run_value(22, 12), manual]}),
        )],
    );
    let waits = Cell::new(0);

    let latest = api
        .latest_source_ci_optional_with_sleep(&workflow(), source(), |_| {
            waits.set(waits.get() + 1);
            Ok(())
        })
        .expect("mixed source inventory is valid")
        .expect("push run is eligible");

    assert_eq!(latest.id, Some(22));
    assert_eq!(fixture.observed().len(), 1);
    assert_eq!(waits.get(), 0);
    fixture.assert_drained();
}

#[test]
fn nonpush_only_inventory_is_not_retried() {
    let mut manual = run_value(23, 13);
    manual["event"] = json!("workflow_dispatch");
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(list_route(), json!({"workflow_runs": [manual]}))],
    );
    let waits = Cell::new(0);

    assert!(
        api.latest_source_ci_optional_with_sleep(&workflow(), source(), |_| {
            waits.set(waits.get() + 1);
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fixture.observed().len(), 1);
    assert_eq!(waits.get(), 0);
    fixture.assert_drained();
}

#[test]
fn malformed_or_foreign_push_is_terminal_even_beside_a_valid_push() {
    let mut foreign = run_value(23, 13);
    foreign["repository"]["full_name"] = json!("foreign/app");
    let mut malformed = run_value(24, 14);
    malformed["run_number"] = Value::Null;

    for invalid in [foreign, malformed] {
        let (api, fixture) = GitHubApi::with_fixture(
            "example/app",
            vec![(
                list_route(),
                json!({"workflow_runs": [run_value(22, 12), invalid]}),
            )],
        );
        let waits = Cell::new(0);

        assert!(
            api.latest_source_ci_optional_with_sleep(&workflow(), source(), |_| {
                waits.set(waits.get() + 1);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fixture.observed().len(), 1);
        assert_eq!(waits.get(), 0);
        fixture.assert_drained();
    }
}

#[test]
fn malformed_inventory_response_is_terminal_without_retry() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(list_route(), json!({"workflow_runs": "malformed"}))],
    );

    assert!(
        api.latest_source_ci_optional_with_sleep(&workflow(), source(), |_| Ok(()))
            .is_err()
    );
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 1);
}

#[test]
fn required_inventory_keeps_its_missing_run_error_after_empty_retries() {
    let empty = (list_route(), json!({"workflow_runs":[]}));
    let (api, fixture) =
        GitHubApi::with_fixture("example/app", vec![empty; EMPTY_WORKFLOW_RUN_ATTEMPTS]);
    let mut waits = Vec::new();

    let Err(error) = api.latest_source_ci_with_sleep(&workflow(), source(), |duration| {
        waits.push(duration);
        Ok(())
    }) else {
        panic!("required callers must retain the missing-run failure");
    };

    assert!(error.to_string().contains("package_ci_run_missing"));
    assert_eq!(fixture.observed().len(), EMPTY_WORKFLOW_RUN_ATTEMPTS);
    fixture.assert_drained();
    assert_eq!(waits, [3, 6, 12, 24, 45].map(Duration::from_secs));
}

#[test]
fn full_run_page_cap_is_terminal_without_retry() {
    let mut reads = 0;
    let mut waits = Vec::new();
    let result = retry_empty_workflow_run_inventory(
        |page| {
            reads += 1;
            assert_eq!(page, reads);
            Ok((0..WORKFLOW_RUN_PAGE_SIZE)
                .map(|offset| {
                    let id = page * WORKFLOW_RUN_PAGE_SIZE + offset;
                    run(
                        u64::try_from(id).expect("bounded run id"),
                        u64::try_from(offset + 1).expect("bounded run number"),
                    )
                })
                .collect())
        },
        |duration| {
            waits.push(duration);
            Ok(())
        },
        check,
    );

    assert!(result.is_err());
    assert_eq!(reads, MAX_WORKFLOW_RUN_PAGES);
    assert_eq!(waits, Vec::<Duration>::new());
}

#[test]
fn exhausted_parent_deadline_clips_retry_wait_and_prevents_another_read() {
    let deadline = CheckDeadline::after(Duration::from_millis(40)).expect("deadline");
    let reads = Cell::new(0);
    let start = Instant::now();
    let result = deadline.scope(|| {
        retry_empty_workflow_run_inventory(
            |_| {
                reads.set(reads.get() + 1);
                Ok(Vec::new())
            },
            sleep_for_retry,
            check,
        )
    });

    assert!(result.is_err());
    assert_eq!(reads.get(), 1);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[path = "package_release_run_retry_adapter_tests.rs"]
mod adapter_tests;
