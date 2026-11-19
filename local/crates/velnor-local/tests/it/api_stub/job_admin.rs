// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `--job-cache-admin`: the job port serves the cache list and delete that `velnor-ci cache
//! budget` and the cache maintenance job call, from the cache server's index.

use super::{CACHES, LIMIT, keys};
use crate::support::{Harness, MAIN, PULL_2, Stub, temp_dir};

fn job_stub(harness: &Harness, name: &str) -> Stub {
    Stub::job_cache_admin(
        &temp_dir(&format!("{name}-repo")),
        &temp_dir(&format!("{name}-state")),
        harness.dir(),
    )
}

#[tokio::test]
async fn a_job_lists_the_cache_entries_page_by_page_as_the_helper_asks() {
    let harness = Harness::new("job-admin-list", LIMIT).await;
    harness.register(2).await;
    for key in ["a", "b", "c"] {
        harness.save(MAIN, key, key.as_bytes()).await;
    }
    harness.save(PULL_2, "pr", b"p").await;
    let stub = job_stub(&harness, "job-admin-list");

    let first = stub
        .call_as("GET", &format!("{CACHES}?&per_page=2&page=1"), "job-token")
        .await;
    let second = stub
        .call_as("GET", &format!("{CACHES}?&per_page=2&page=2"), "job-token")
        .await;
    let past = stub
        .call_as("GET", &format!("{CACHES}?&per_page=2&page=3"), "job-token")
        .await;

    assert_eq!(first.status, 200);
    assert_eq!(first.json()["total_count"], 4);
    assert_eq!(keys(&first.json()).len(), 2);
    assert!(first.link().contains("rel=\"next\""));
    assert_eq!(keys(&second.json()).len(), 2);
    assert!(!second.link().contains("rel=\"next\""));
    assert_eq!(past.status, 200);
    assert_eq!(keys(&past.json()), Vec::<String>::new());
    let mut all = keys(&first.json());
    all.extend(keys(&second.json()));
    all.sort_unstable();
    assert_eq!(all, ["a", "b", "c", "pr"]);
}

#[tokio::test]
async fn a_job_deletes_a_cache_entry_by_id_and_the_cache_server_no_longer_restores_it() {
    let harness = Harness::new("job-admin-delete", LIMIT).await;
    harness.register(1).await;
    harness.save(MAIN, "tools", b"1234").await;
    harness.save(MAIN, "target", b"123456").await;
    let stub = job_stub(&harness, "job-admin-delete");
    let listed = stub.call_as("GET", CACHES, "job-token").await.json();
    let id = listed["actions_caches"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["key"] == "tools")
        .map(|entry| entry["id"].clone())
        .unwrap_or_default();

    let deleted = stub
        .call_as("DELETE", &format!("{CACHES}/{id}"), "job-token")
        .await;
    let again = stub
        .call_as("DELETE", &format!("{CACHES}/{id}"), "job-token")
        .await;
    let after = stub.call_as("GET", CACHES, "job-token").await.json();

    assert_eq!(deleted.status, 204);
    assert_eq!(again.status, 404);
    assert_eq!(keys(&after), ["target"]);
    assert_eq!(harness.restore(MAIN, "tools").await["ok"], false);
    assert_eq!(harness.restore(MAIN, "target").await["ok"], true);
}

#[tokio::test]
async fn the_job_token_is_accepted_whatever_it_holds_even_an_empty_one() {
    let harness = Harness::new("job-admin-token", LIMIT).await;
    harness.save(MAIN, "tools", b"1234").await;
    let stub = job_stub(&harness, "job-admin-token");

    // `github.token` is empty in a local run, so the helper sends "Bearer ".
    for bearer in ["", "ghs_job", "not even a token"] {
        let reply = stub.call_as("GET", CACHES, bearer).await;
        assert_eq!(reply.status, 200, "bearer {bearer:?}");
    }
    assert_eq!(stub.get(CACHES).await.status, 200);
}
