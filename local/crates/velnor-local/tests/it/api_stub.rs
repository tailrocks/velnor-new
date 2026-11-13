// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_local::io::lock_index;
use velnor_local::rest::rfc3339;

use crate::support::{Harness, MAIN, Stub, temp_dir};

const LIMIT: u64 = 10_000_000_000;
const CACHES: &str = "/repos/tailrocks/velnor-lab/actions/caches";

fn stub_over(harness: &Harness, name: &str) -> Stub {
    Stub::new(
        &temp_dir(&format!("{name}-repo")),
        &temp_dir(&format!("{name}-state")),
        harness.dir(),
    )
}

fn keys(list: &Value) -> Vec<String> {
    list["actions_caches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["key"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn deleting_a_listed_cache_entry_removes_it_from_the_next_list_and_from_the_cache_server() {
    let harness = Harness::new("api-delete", LIMIT).await;
    harness.save(MAIN, "tools", b"1234").await;
    harness.save(MAIN, "target", b"123456").await;
    let stub = stub_over(&harness, "api-delete");

    let listed = stub.get(CACHES).await;
    let list = listed.json();
    let tools = list["actions_caches"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["key"] == "tools")
        .cloned()
        .unwrap_or_default();
    let before = std::fs::read_dir(harness.dir().join("archives")).map_or(0, Iterator::count);
    let deleted = stub
        .call("DELETE", &format!("{CACHES}/{}", tools["id"]))
        .await;
    let after_list = stub.get(CACHES).await.json();
    let again = stub
        .call("DELETE", &format!("{CACHES}/{}", tools["id"]))
        .await;
    let after = std::fs::read_dir(harness.dir().join("archives")).map_or(0, Iterator::count);

    assert_eq!(listed.status, 200);
    assert_eq!(list["total_count"], 2);
    assert_eq!(tools["ref"], MAIN);
    assert_eq!(tools["version"], "v");
    assert_eq!(tools["size_in_bytes"], 4);
    assert_eq!(tools["created_at"].as_str().map(str::len), Some(20));
    assert_eq!(deleted.status, 204);
    assert!(deleted.body.is_empty());
    assert_eq!(keys(&after_list), ["target"]);
    assert_eq!(after_list["total_count"], 1);
    assert_eq!(again.status, 404);
    assert_eq!((before, after), (2, 1));
    assert_eq!(harness.restore(MAIN, "tools").await["ok"], false);
    assert_eq!(harness.restore(MAIN, "target").await["ok"], true);
}

#[tokio::test]
async fn the_cache_list_filters_sorts_and_pages_like_github() {
    let harness = Harness::new("api-list", LIMIT).await;
    harness.register(1).await;
    harness.save(MAIN, "npm-a", b"1").await;
    harness.save(MAIN, "npm-b", b"123").await;
    harness.save(MAIN, "cargo", b"12").await;
    let stub = stub_over(&harness, "api-list");

    let by_size = stub
        .get(&format!("{CACHES}?sort=size_in_bytes&direction=asc"))
        .await
        .json();
    let prefixed = stub.get(&format!("{CACHES}?key=npm")).await.json();
    let other_ref = stub
        .get(&format!("{CACHES}?ref=refs/pull/1/merge"))
        .await
        .json();
    let second = stub
        .get(&format!(
            "{CACHES}?sort=size_in_bytes&direction=asc&per_page=2&page=2"
        ))
        .await;

    assert_eq!(keys(&by_size), ["npm-a", "cargo", "npm-b"]);
    assert_eq!(by_size["total_count"], 3);
    assert_eq!(prefixed["total_count"], 2);
    assert_eq!(other_ref["total_count"], 0);
    assert_eq!(keys(&second.json()), ["npm-b"]);
    assert_eq!(second.json()["total_count"], 3);
    let link = second.link();
    assert!(link.contains("page=1&per_page=2") && link.contains("rel=\"prev\""));
    assert!(link.contains("rel=\"first\""));
    assert!(!link.contains("rel=\"next\""));
}

#[tokio::test]
async fn open_pull_requests_come_from_the_state_file_and_page_with_a_link_header() {
    let harness = Harness::new("api-pulls", LIMIT).await;
    let state = temp_dir("api-pulls-state");
    let pulls = json!([
        {"number": 1, "state": "open", "title": "one"},
        {"number": 2, "state": "closed", "title": "two"},
        {"number": 3, "state": "open", "title": "three"},
    ]);
    std::fs::write(state.join("pulls.json"), pulls.to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stub = Stub::new(&temp_dir("api-pulls-repo"), &state, harness.dir());
    let path = "/repos/tailrocks/velnor-lab/pulls";

    let open = stub.get(&format!("{path}?state=open")).await.json();
    let all = stub.get(&format!("{path}?state=all")).await.json();
    let first = stub.get(&format!("{path}?per_page=1")).await;

    let numbers = |list: &Value| -> Vec<Value> {
        list.as_array()
            .into_iter()
            .flatten()
            .map(|pull| pull["number"].clone())
            .collect()
    };
    assert_eq!(numbers(&open), [json!(1), json!(3)]);
    assert_eq!(numbers(&all).len(), 3);
    assert_eq!(numbers(&first.json()), [json!(1)]);
    assert!(first.link().contains("page=2&per_page=1"));
    assert!(first.link().contains("rel=\"next\""));
}

#[tokio::test]
async fn any_other_request_gets_501_with_a_json_body_that_names_the_path() {
    let harness = Harness::new("api-501", LIMIT).await;
    let stub = stub_over(&harness, "api-501");

    for (method, path) in [
        ("GET", "/repos/o/r/issues"),
        ("GET", "/rate_limit"),
        ("DELETE", "/repos/o/r/pulls/1"),
        ("POST", "/repos/o/r/actions/caches"),
        ("GET", "/repos/o/r/actions/runs/1"),
    ] {
        let reply = stub.call(method, path).await;
        assert_eq!(reply.status, 501, "{method} {path}");
        assert_eq!(reply.json()["path"], path);
        assert!(
            reply.json()["message"]
                .as_str()
                .is_some_and(|message| message.contains(path)),
            "{method} {path}"
        );
        assert_eq!(
            reply
                .headers
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("application/json; charset=utf-8")
        );
    }
}

#[tokio::test]
async fn a_stub_delete_waits_for_the_index_lock() {
    let harness = Harness::new("api-lock", LIMIT).await;
    harness.save(MAIN, "tools", b"1234").await;
    let stub = stub_over(&harness, "api-lock");
    let id = stub.get(CACHES).await.json()["actions_caches"][0]["id"].clone();
    let path = format!("{CACHES}/{id}");

    let lock = lock_index(harness.dir())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let blocked =
        tokio::time::timeout(Duration::from_millis(300), stub.call("DELETE", &path)).await;
    drop(lock);
    let released = stub.call("DELETE", &path).await;

    assert!(blocked.is_err());
    assert_eq!(released.status, 204);
}

#[tokio::test]
async fn saves_and_stub_deletes_running_together_lose_no_update() {
    let harness = Harness::new("api-race", LIMIT).await;
    for number in 0..12 {
        harness.save(MAIN, &format!("old-{number}"), b"1").await;
    }
    let stub = stub_over(&harness, "api-race");
    let listed = stub.get(&format!("{CACHES}?per_page=100")).await.json();
    let ids: Vec<Value> = listed["actions_caches"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| entry["id"].clone())
        .collect();

    let saving = async {
        for number in 0..12 {
            harness.save(MAIN, &format!("new-{number}"), b"2").await;
        }
    };
    let deleting = async {
        for id in &ids {
            assert_eq!(
                stub.call("DELETE", &format!("{CACHES}/{id}")).await.status,
                204
            );
        }
    };
    tokio::join!(saving, deleting);

    let after = stub.get(&format!("{CACHES}?per_page=100")).await.json();
    let mut keys = keys(&after);
    keys.sort();
    let mut expected: Vec<String> = (0..12).map(|number| format!("new-{number}")).collect();
    expected.sort();
    assert_eq!(keys, expected);
    assert_eq!(harness.restore(MAIN, "old-3").await["ok"], false);
    assert_eq!(harness.restore(MAIN, "new-3").await["ok"], true);
}

fn mode(path: &std::path::Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

async fn failed_delete_keeps_the_entry_and_its_archive(name: &str, locked: &str) {
    let harness = Harness::new(name, LIMIT).await;
    harness.save(MAIN, "tools", b"1234").await;
    let stub = stub_over(&harness, name);
    let id = stub.get(CACHES).await.json()["actions_caches"][0]["id"].clone();
    let blocked = harness.dir().join(locked);

    mode(&blocked, 0o555);
    let failed = stub.call("DELETE", &format!("{CACHES}/{id}")).await;
    mode(&blocked, 0o755);

    assert_eq!(failed.status, 500);
    assert_eq!(keys(&stub.get(CACHES).await.json()), ["tools"]);
    assert_eq!(harness.restore(MAIN, "tools").await["ok"], true);
    let blob = harness.restore(MAIN, "tools").await["signed_download_url"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert_eq!(harness.get(&blob).await.body.as_ref(), b"1234");
}

#[tokio::test]
async fn a_delete_that_cannot_remove_the_archive_fails_and_keeps_the_entry() {
    failed_delete_keeps_the_entry_and_its_archive("api-keep-archive", "archives").await;
}

#[tokio::test]
async fn a_delete_whose_index_write_fails_puts_the_archive_back() {
    failed_delete_keeps_the_entry_and_its_archive("api-keep-index", ".").await;
}

#[tokio::test]
async fn an_unreadable_index_is_an_error_not_a_stale_answer() {
    let harness = Harness::new("api-stale", LIMIT).await;
    harness.save(MAIN, "tools", b"1234").await;
    std::fs::write(harness.dir().join("index.json"), "not json")
        .unwrap_or_else(|e| unreachable!("{e}"));

    let reply = harness
        .twirp(
            MAIN,
            "GetCacheEntryDownloadURL",
            &json!({"key": "tools", "version": "v"}),
        )
        .await;

    assert_eq!(reply.status, 500);
    assert_eq!(reply.json()["code"], "internal");
}

#[tokio::test]
async fn pull_request_state_that_cannot_be_read_is_a_server_error() {
    let harness = Harness::new("api-nopulls", LIMIT).await;
    let state = temp_dir("api-nopulls-state");
    let stub = Stub::new(&temp_dir("api-nopulls-repo"), &state, harness.dir());
    let path = "/repos/tailrocks/velnor-lab/pulls";

    let missing = stub.get(path).await;
    std::fs::write(state.join("pulls.json"), "{\"number\": 1")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let malformed = stub.get(path).await;
    let files = stub.get(&format!("{path}/1/files")).await;

    assert_eq!(
        (missing.status, malformed.status, files.status),
        (500, 500, 500)
    );
    assert_eq!(missing.json()["message"], "pulls.json is unreadable");
}

#[test]
fn timestamps_are_written_as_utc_rfc3339() {
    assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
    assert_eq!(rfc3339(1_794_000_000), "2026-11-06T21:20:00Z");
    assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
}
