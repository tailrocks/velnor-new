// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::VecDeque;
use velnor_ci::delta_trim::trim_deltas;
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};

struct Api {
    responses: RefCell<VecDeque<Result<Value, CiError>>>,
    gets: RefCell<Vec<String>>,
    deleted: RefCell<Vec<String>>,
}
impl GitHub for Api {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.gets.borrow_mut().push(path.to_owned());
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| CiError::new("unexpected GET"))?
            .map(|value| value.to_string())
    }
}
impl GitHubWrite for Api {
    fn wait(&self, _: std::time::Duration) {}
    fn upload(&self, _: &str, _: &[u8]) -> Result<String, CiError> {
        unreachable!()
    }
    fn patch(&self, _: &str, _: &str) -> Result<String, CiError> {
        unreachable!()
    }
    fn delete(&self, path: &str) -> Result<(), CiError> {
        self.deleted.borrow_mut().push(path.to_owned());
        Ok(())
    }
}
fn run() -> Value {
    json!({"id":123,"status":"completed", "event":"pull_request", "workflow_id":2, "repository":{"id":1}, "head_repository":{"id":1}, "pull_requests":[{"number":42}]})
}
fn entry(id: u64, lane: &str, base: &str) -> Value {
    json!({"id":id, "key":format!("velnor-dlt-pr42-{lane}.root.Linux-X64-0123456789abcdef-{base}-123"), "ref":"refs/pull/42/merge", "created_at":format!("2026-11-17T00:00:{id:02}Z")})
}
fn api(run: Value, pages: Vec<Result<Value, CiError>>) -> Api {
    let mut responses = VecDeque::from([
        Ok(json!({"id":1})),
        Ok(json!({"id":2})),
        Ok(run),
        Ok(json!({"base":{"repo":{"id":1}}, "head":{"repo":{"id":1}}})),
    ]);
    responses.extend(pages);
    Api {
        responses: RefCell::new(responses),
        gets: RefCell::new(Vec::new()),
        deleted: RefCell::new(Vec::new()),
    }
}
fn repo() -> Repo {
    Repo::parse("owner/repo").unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn trusted_cleanup_keeps_newest_delta_per_lane_across_bases_after_reading_every_page() {
    let first = entry(1, "rust-test", "11111111");
    let second = entry(2, "rust-test", "22222222");
    let lint = entry(3, "rust-lint", "11111111");
    let mut malformed = entry(4, "rust-test", "11111111");
    malformed["key"] = json!("velnor-dlt-pr42-invalid");
    let mut other = entry(5, "rust-test", "11111111");
    other["key"] = json!("velnor-dlt-pr43-rust-test.root.Linux-X64-0123456789abcdef-11111111-123");
    let api = api(
        run(),
        vec![
            Ok(json!({"total_count":5,"actions_caches":[first, lint]})),
            Ok(json!({"total_count":5,"actions_caches":[second, malformed, other]})),
        ],
    );
    let result = trim_deltas(&api, &repo(), 123).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(result.contains("deleted 1"), "{result}");
    assert_eq!(
        *api.deleted.borrow(),
        ["/repos/owner/repo/actions/caches/1"]
    );
    let gets = api.gets.borrow();
    assert!(gets.last().is_some_and(|path| path.contains("page=2")));
    assert!(
        gets.iter()
            .filter(|path| path.contains("actions/caches?"))
            .all(|path| path.contains("key=velnor-dlt-pr42-&ref=refs%2Fpull%2F42%2Fmerge"))
    );
}

#[test]
fn unverifiable_runs_refs_and_incomplete_listings_never_delete_any_cache() {
    let mut invalid_runs = Vec::new();
    for (field, value) in [
        ("id", json!(124)),
        ("status", json!("in_progress")),
        ("event", json!("push")),
        ("workflow_id", json!(99)),
        ("repository", json!({"id":99})),
        ("head_repository", json!({"id":99})),
        ("pull_requests", json!([])),
    ] {
        let mut value_run = run();
        value_run[field] = value;
        invalid_runs.push(value_run);
    }
    for invalid in invalid_runs {
        let api = api(
            invalid,
            vec![Ok(json!({"total_count":2,"actions_caches":[
                entry(1,"rust-test","11111111"), entry(2,"rust-test","22222222")
            ]}))],
        );
        assert!(trim_deltas(&api, &repo(), 123).is_err());
        assert!(api.deleted.borrow().is_empty());
    }
    let mut wrong_ref = entry(2, "rust-test", "11111111");
    wrong_ref["ref"] = json!("refs/heads/main");
    let cases = vec![
        vec![
            Ok(json!({"total_count":2,"actions_caches":[entry(1,"rust-test","11111111")]})),
            Ok(json!({"total_count":2,"actions_caches":[entry(1,"rust-test","11111111")]})),
        ],
        vec![
            Ok(json!({"total_count":2,"actions_caches":[entry(1,"rust-test","11111111")]})),
            Ok(json!({"total_count":3,"actions_caches":[entry(2,"rust-test","11111111")]})),
            Ok(json!({"total_count":3,"actions_caches":[entry(3,"rust-test","11111111")]})),
        ],
        vec![
            Ok(
                json!({"total_count":3,"actions_caches":[entry(1,"rust-test","11111111"),entry(2,"rust-test","11111111")]}),
            ),
            Err(CiError::new("page 2 unavailable")),
        ],
        vec![Ok(
            json!({"total_count":2,"actions_caches":[entry(1,"rust-test","11111111"),wrong_ref]}),
        )],
        vec![Ok(json!({"actions_caches":[]}))],
    ];
    for pages in cases {
        let api = api(run(), pages);
        assert!(trim_deltas(&api, &repo(), 123).is_err());
        assert!(api.deleted.borrow().is_empty());
    }
}
