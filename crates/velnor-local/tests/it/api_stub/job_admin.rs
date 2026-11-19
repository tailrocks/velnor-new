// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Cache administration on the job port mirrors GitHub: a job lists and deletes `actions/caches`
//! only with `actions: write` on an event that is not a pull request. runner.server hands such a
//! job a signed grant as `github.token`, and the grant counts only for a run the driver's ledger
//! admits; `--job-cache-admin` grants every job instead.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use velnor_local::io::api_stub::{Answer, StandIn};

use super::{CACHES, LIMIT, archives, keys};
use crate::support::{Harness, MAIN, PULL_2, Stub, job_token, temp_dir};

const RUNS: &str = "/_apis/v1/Message/workflow/runs";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

#[derive(Debug)]
struct Runs(Vec<u8>);

impl StandIn for Runs {
    fn get<'a>(&'a self, path: &'a str) -> Answer<'a> {
        Box::pin(std::future::ready((path == RUNS).then(|| self.0.clone())))
    }
}

fn run(event: &str, git_ref: &str) -> Value {
    json!([{"id": 1, "fileName": ".github/workflows/cache.yml", "eventName": event, "ref": git_ref, "sha": SHA}])
}

fn recorded(event: &str, git_ref: &str, existing: &[u64]) -> Value {
    json!({"id": format!("1-{event}"), "side": "trusted", "event": event, "ref": git_ref, "sha": SHA, "workflow": null, "existing": existing})
}

fn bound(harness: &Harness, name: &str, runs: Option<&Value>, ledger: Option<&[Value]>) -> Stub {
    let (repo, state) = (
        temp_dir(&format!("{name}-repo")),
        temp_dir(&format!("{name}-state")),
    );
    let folder = ledger.map(|entries| {
        let folder: PathBuf = state.join("authority");
        std::fs::create_dir_all(&folder).unwrap_or_else(|e| unreachable!("{e}"));
        let lines = entries.iter().map(Value::to_string).collect::<Vec<_>>();
        std::fs::write(folder.join("ledger.jsonl"), lines.join("\n"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        folder
    });
    let standin =
        runs.map(|runs| Arc::new(Runs(runs.to_string().into_bytes())) as Arc<dyn StandIn>);
    Stub::job_grants(&repo, &state, harness.dir(), (standin, folder))
}

fn stub(harness: &Harness, name: &str, override_all: bool) -> Stub {
    if override_all {
        let repo = temp_dir(&format!("{name}-repo"));
        return Stub::job_cache_admin(&repo, &temp_dir(&format!("{name}-state")), harness.dir());
    }
    let dispatched = [recorded("push", MAIN, &[])];
    bound(harness, name, Some(&run("push", MAIN)), Some(&dispatched))
}

async fn seeded(name: &str) -> Harness {
    let harness = Harness::new(name, LIMIT).await;
    harness.register(2).await;
    for key in ["a", "b", "c"] {
        harness.save(MAIN, key, key.as_bytes()).await;
    }
    harness.save(PULL_2, "pr", b"p").await;
    harness
}

async fn refused(stub: &Stub, harness: &Harness, bearer: Option<&str>) {
    let before = archives(harness);
    let id = stub
        .call_as("GET", CACHES, "x")
        .await
        .json()
        .get("actions_caches")
        .and_then(|list| list.get(0))
        .map_or_else(|| "1".to_owned(), |entry| entry["id"].to_string());
    for (method, path) in [
        ("GET", format!("{CACHES}?per_page=100")),
        ("DELETE", format!("{CACHES}/{id}")),
    ] {
        let reply = match bearer {
            Some(bearer) => stub.call_as(method, &path, bearer).await,
            None => stub.call(method, &path).await,
        };
        assert_eq!(reply.status, 403, "{method} with {bearer:?}");
    }
    assert_eq!(archives(harness), before);
    assert_eq!(harness.restore(MAIN, "a").await["ok"], true);
}

#[tokio::test]
async fn a_push_job_with_actions_write_lists_page_by_page_and_deletes() {
    let harness = seeded("job-admin-push").await;
    let stub = stub(&harness, "job-admin-push", false);
    let token = job_token("push-actions-write");

    let first = stub
        .call_as("GET", &format!("{CACHES}?&per_page=2&page=1"), &token)
        .await;
    let second = stub
        .call_as("GET", &format!("{CACHES}?&per_page=2&page=2"), &token)
        .await;
    let past = stub
        .call_as("GET", &format!("{CACHES}?&per_page=2&page=3"), &token)
        .await;

    assert_eq!(first.status, 200);
    assert_eq!(first.json()["total_count"], 4);
    assert!(first.link().contains("rel=\"next\""));
    assert!(!second.link().contains("rel=\"next\""));
    assert_eq!(past.status, 200);
    let mut all = keys(&first.json());
    all.extend(keys(&second.json()));
    all.sort_unstable();
    assert_eq!(all, ["a", "b", "c", "pr"]);
    assert_eq!(keys(&past.json()), Vec::<String>::new());

    let id = first.json()["actions_caches"][0]["id"].clone();
    let deleted = stub
        .call_as("DELETE", &format!("{CACHES}/{id}"), &token)
        .await;
    let again = stub
        .call_as("DELETE", &format!("{CACHES}/{id}"), &token)
        .await;
    let after = stub.call_as("GET", CACHES, &token).await.json();
    assert_eq!(deleted.status, 204);
    assert_eq!(again.status, 404);
    assert_eq!(after["total_count"], 3);
}

#[tokio::test]
async fn a_push_job_with_only_actions_read_lists_but_cannot_delete() {
    let harness = seeded("job-admin-read").await;
    let stub = stub(&harness, "job-admin-read", false);
    let token = job_token("push-actions-read");
    let before = archives(&harness);

    let listed = stub.call_as("GET", CACHES, &token).await;
    let id = listed.json()["actions_caches"][0]["id"].clone();
    let deleted = stub
        .call_as("DELETE", &format!("{CACHES}/{id}"), &token)
        .await;

    assert_eq!(listed.status, 200);
    assert_eq!(listed.json()["total_count"], 4);
    assert_eq!(deleted.status, 403);
    assert_eq!(archives(&harness), before);
}

#[test]
fn runner_server_grants_a_job_on_its_calculated_permission_whether_or_not_the_job_spells_it_out() {
    let snippet = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/docker/runner-server/job-grant.cs.snippet"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!snippet.contains("jobPermissions"), "{snippet}");
    assert!(snippet.contains("calculatedPermissions.TryGetValue(\"actions\""));
    assert!(snippet.contains("actionsPermission == \"read\""));
    assert!(snippet.contains("new Claim(\"actions\", actionsPermission)"));
    assert!(snippet.contains("!wevent.StartsWith(\"pull_request\")"));
    assert!(snippet.contains("new Claim(\"runid\", runid.ToString())"));
}

#[tokio::test]
async fn a_pull_request_job_with_actions_write_gets_403() {
    let harness = seeded("job-admin-pr").await;
    let stub = stub(&harness, "job-admin-pr", false);
    for name in [
        "pull-request-actions-write",
        "pull-request-target-actions-write",
    ] {
        refused(&stub, &harness, Some(&job_token(name))).await;
    }
}

#[tokio::test]
async fn a_schedule_job_with_actions_write_is_granted_like_a_push() {
    let harness = seeded("job-admin-schedule").await;
    let dispatched = [recorded("schedule", MAIN, &[])];
    let runs = run("schedule", MAIN);
    let stub = bound(
        &harness,
        "job-admin-schedule",
        Some(&runs),
        Some(&dispatched),
    );
    let listed = stub
        .call_as("GET", CACHES, &job_token("schedule-actions-write"))
        .await;
    assert_eq!(listed.status, 200);
}

#[tokio::test]
async fn the_override_grants_every_job_whatever_it_carries() {
    let harness = seeded("job-admin-override").await;
    let stub = stub(&harness, "job-admin-override", true);

    for bearer in [
        "",
        "ghs_job",
        job_token("pull-request-actions-write").as_str(),
    ] {
        let reply = stub.call_as("GET", CACHES, bearer).await;
        assert_eq!(reply.status, 200, "bearer {bearer:?}");
    }
    assert_eq!(stub.get(CACHES).await.status, 200);
    let id = stub.get(CACHES).await.json()["actions_caches"][0]["id"].clone();
    assert_eq!(
        stub.call_as("DELETE", &format!("{CACHES}/{id}"), "")
            .await
            .status,
        204
    );
}

#[tokio::test]
async fn an_unknown_token_gets_403() {
    let harness = seeded("job-admin-unknown").await;
    let stub = stub(&harness, "job-admin-unknown", false);
    let signed = ["expired", "tampered", "other-repository", "wrong-marker"].map(job_token);
    let unsigned = [None, Some(""), Some("ghs_job"), Some("a.b.c")];
    for bearer in unsigned {
        refused(&stub, &harness, bearer).await;
    }
    for token in &signed {
        refused(&stub, &harness, Some(token)).await;
    }
}

#[tokio::test]
async fn the_control_token_does_not_unlock_the_job_port() {
    let harness = seeded("job-admin-control").await;
    let stub = stub(&harness, "job-admin-control", false);
    refused(&stub, &harness, Some("test-control")).await;
}

#[tokio::test]
async fn a_forged_push_run_the_driver_never_dispatched_gets_403() {
    let harness = seeded("job-admin-forged").await;
    let token = job_token("push-actions-write");
    let forged = run("push", MAIN);
    let pull_request = [recorded("pull_request", PULL_2, &[])];
    for (name, ledger) in [
        ("job-admin-forged-pr", &pull_request[..]),
        ("job-admin-forged-empty", &[][..]),
    ] {
        let stub = bound(&harness, name, Some(&forged), Some(ledger));
        refused(&stub, &harness, Some(&token)).await;
    }
}

#[tokio::test]
async fn a_grant_claiming_push_for_a_run_the_ledger_records_as_a_pull_request_gets_403() {
    let harness = seeded("job-admin-relabelled").await;
    let runs = run("pull_request", PULL_2);
    let dispatched = [recorded("pull_request", PULL_2, &[])];
    let stub = bound(
        &harness,
        "job-admin-relabelled",
        Some(&runs),
        Some(&dispatched),
    );
    refused(&stub, &harness, Some(&job_token("push-actions-write"))).await;
}

#[tokio::test]
async fn a_stale_grant_for_a_run_older_than_the_ledger_entry_gets_403() {
    let harness = seeded("job-admin-stale").await;
    let runs = run("push", MAIN);
    let dispatched = [recorded("push", MAIN, &[1])];
    let stub = bound(&harness, "job-admin-stale", Some(&runs), Some(&dispatched));
    refused(&stub, &harness, Some(&job_token("push-actions-write"))).await;
}

#[tokio::test]
async fn without_the_ledger_or_runner_server_no_grant_counts() {
    let harness = seeded("job-admin-unbound").await;
    let token = job_token("push-actions-write");
    let (runs, dispatched) = (run("push", MAIN), [recorded("push", MAIN, &[])]);
    for (name, runs, ledger) in [
        ("job-admin-no-ledger", Some(&runs), None),
        ("job-admin-no-standin", None, Some(&dispatched[..])),
    ] {
        refused(&bound(&harness, name, runs, ledger), &harness, Some(&token)).await;
    }
}
