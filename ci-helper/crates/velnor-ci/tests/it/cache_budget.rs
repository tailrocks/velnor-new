// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::Duration;
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};
use velnor_ci::io::cache_api::{budget, trim};

struct Api {
    entries: RefCell<Vec<Value>>,
    prs: Vec<Value>,
    fail: Option<String>,
    fail_delete: bool,
    delete_status: Option<u16>,
    changed_total: bool,
    incomplete: bool,
    status: Value,
    confirmations: RefCell<Vec<String>>,
    elapsed: Cell<Duration>,
    deletions: RefCell<VecDeque<Duration>>,
}

impl Api {
    fn new() -> Self {
        let entries = (1..=101).map(|id| json!({"id":id,"key":format!("velnor-dlt-pr101-rust.root.Linux-X64-0123456789abcdef-12345678-{id}"),"ref":"refs/pull/101/merge","created_at":"2026-11-17T00:00:00Z","last_accessed_at":"2026-11-17T00:00:00Z","size_in_bytes":10})).collect();
        Self {
            entries: RefCell::new(entries),
            prs: (1..=101).map(|number| json!({"number":number})).collect(),
            fail: None,
            fail_delete: false,
            delete_status: None,
            changed_total: false,
            incomplete: false,
            status: json!({"state":"open"}),
            confirmations: RefCell::default(),
            elapsed: Cell::new(Duration::ZERO),
            deletions: RefCell::default(),
        }
    }
}

impl GitHub for Api {
    fn get(&self, path: &str) -> Result<String, CiError> {
        if self.fail.as_ref().is_some_and(|fail| path.contains(fail)) {
            return Err(CiError::new("listing failed"));
        }
        if path.contains("/pulls/") {
            self.confirmations.borrow_mut().push(path.to_owned());
            return Ok(if path.ends_with("/1") {
                json!({"state":"closed"})
            } else {
                self.status.clone()
            }
            .to_string());
        }
        let page: usize = path
            .rsplit_once("page=")
            .and_then(|(_, page)| page.parse().ok())
            .ok_or_else(|| CiError::new("missing page"))?;
        let entries = self.entries.borrow();
        let source = if path.contains("/pulls?") {
            &self.prs
        } else {
            &entries
        };
        let rows: Vec<_> = source
            .iter()
            .skip((page - 1) * 100)
            .take(100)
            .cloned()
            .collect();
        Ok(if path.contains("/pulls?") {
            json!(rows)
        } else {
            json!({"total_count":source.len() + usize::from(self.changed_total && page > 1) + usize::from(self.incomplete),"actions_caches":rows})
        }
        .to_string())
    }
}
impl GitHubWrite for Api {
    fn wait(&self, duration: Duration) {
        self.elapsed.set(self.elapsed.get() + duration);
    }

    fn upload(&self, _: &str, _: &[u8]) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn patch(&self, _: &str, _: &str) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn delete(&self, path: &str) -> Result<(), CiError> {
        let now = self.elapsed.get();
        let mut deletions = self.deletions.borrow_mut();
        deletions.retain(|at| now.saturating_sub(*at) < Duration::from_secs(60));
        if deletions.len() >= 400 {
            return Err(CiError::new("HTTP 429: 400 cache deletions per minute"));
        }
        deletions.push_back(now);
        if self.fail_delete {
            return Err(CiError::new("delete failed"));
        }
        let id: u64 = path
            .rsplit('/')
            .next()
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| CiError::new("invalid id"))?;
        if let Some(status) = self.delete_status {
            if !matches!(status, 404 | 410) {
                return Err(CiError::new(format!("HTTP {status}")));
            }
            self.entries.borrow_mut().retain(|entry| entry["id"] != id);
            if id == 1 {
                return Err(CiError::missing(format!("HTTP {status}")));
            }
        }
        self.entries.borrow_mut().retain(|entry| entry["id"] != id);
        Ok(())
    }
}
fn repo() -> Repo {
    Repo::parse("tailrocks/lab").unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn repository_budget_lists_all_pages_and_dry_run_preserves_external_state() {
    let api = Api::new();
    let report = budget(&api, &repo(), 0, 2000, true).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("Delta | 101 | 1010"), "{report}");
    assert!(report.contains("would delete 0"), "{report}");
    assert_eq!(api.entries.borrow().len(), 101);
    assert_eq!(api.confirmations.borrow().len(), 0);
    let report = budget(&api, &repo(), 0, 0, true).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("would delete 101"), "{report}");
    assert_eq!(api.entries.borrow().len(), 101);
    assert_eq!(api.elapsed.get(), Duration::ZERO);
    budget(&api, &repo(), 0, 0, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(api.entries.borrow().is_empty());
}

#[test]
fn incomplete_listings_delete_nothing_and_deletion_errors_reach_the_caller() {
    for failure in [
        "caches?&per_page=100&page=2",
        "pulls?state=open&per_page=100&page=2",
    ] {
        let mut api = Api::new();
        api.fail = Some(failure.to_owned());
        assert!(budget(&api, &repo(), 0, 0, false).is_err());
        assert_eq!(api.entries.borrow().len(), 101);
    }
    let mut api = Api::new();
    api.fail_delete = true;
    assert!(budget(&api, &repo(), 0, 0, false).is_err());
    assert_eq!(api.entries.borrow().len(), 101);
}

#[test]
fn trim_uses_the_same_complete_listing_and_propagates_delete_failure() {
    let api = Api::new();
    trim(&api, &repo(), "refs/heads/main", "velnor-dlt-", 2, 0)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let ids: Vec<_> = api
        .entries
        .borrow()
        .iter()
        .map(|entry| entry["id"].as_u64().unwrap_or(0))
        .collect();
    assert_eq!(ids, [100, 101]);
    let mut api = Api::new();
    api.fail_delete = true;
    assert!(trim(&api, &repo(), "refs/heads/main", "velnor-dlt-", 2, 0).is_err());
}

#[test]
fn inconsistent_cache_snapshots_fail_before_any_deletion() {
    for scenario in 0..3 {
        let mut api = Api::new();
        match scenario {
            0 => {
                api.entries.borrow_mut()[100]["id"] = json!(1);
            }
            1 => api.changed_total = true,
            _ => api.incomplete = true,
        }
        assert!(budget(&api, &repo(), 0, 0, false).is_err());
        assert_eq!(api.entries.borrow().len(), 101);
    }
}

#[test]
fn omitted_open_pull_requests_are_confirmed_before_closed_deltas_are_deleted() {
    let mut api = Api::new();
    api.prs.truncate(100);
    let report = budget(&api, &repo(), 0, 2000, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("deleted 0"), "{report}");
    assert_eq!(api.entries.borrow().len(), 101);
    assert_eq!(api.confirmations.borrow().len(), 1);
    api.status = json!({"state":"closed"});
    budget(&api, &repo(), 0, 2000, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(api.entries.borrow().len(), 0);
    for status in [
        json!({}),
        json!({"state":"invalid"}),
        json!({"state":false}),
    ] {
        let mut api = Api::new();
        api.prs.clear();
        api.entries.borrow_mut()[0]["ref"] = json!("refs/pull/1/merge");
        api.status = status;
        assert!(budget(&api, &repo(), 0, 2000, false).is_err());
        assert_eq!(api.entries.borrow().len(), 101);
    }
    let mut api = Api::new();
    api.prs.clear();
    api.entries.borrow_mut()[0]["ref"] = json!("refs/pull/1/merge");
    api.status = json!({"state":"closed"});
    api.fail = Some("pulls/101".to_owned());
    assert!(budget(&api, &repo(), 0, 2000, false).is_err());
    assert_eq!(api.entries.borrow().len(), 101);
}

#[test]
fn concurrent_cache_removal_does_not_stop_trim_or_budget() {
    for status in [404, 410, 403, 500] {
        for trimming in [false, true] {
            let mut api = Api::new();
            api.entries.borrow_mut().truncate(2);
            api.delete_status = Some(status);
            let result = if trimming {
                trim(&api, &repo(), "refs/pull/101/merge", "velnor-dlt-", 0, 0)
            } else {
                budget(&api, &repo(), 0, 0, false)
            };
            if matches!(status, 404 | 410) {
                assert!(result.is_ok(), "{status}: {result:?}");
                assert!(api.entries.borrow().is_empty());
            } else {
                assert!(result.is_err(), "{status}");
                assert_eq!(api.entries.borrow().len(), 2);
            }
        }
    }
}

#[test]
fn cache_budget_cli_enforces_the_eight_gigabyte_limit() {
    use clap::Parser;
    use velnor_ci::cli::Cli;
    for value in ["0", "123", "8000000000"] {
        assert!(
            Cli::try_parse_from(["velnor-ci", "cache", "budget", "--budget-bytes", value]).is_ok(),
            "{value}"
        );
    }
    assert!(Cli::try_parse_from(["velnor-ci", "cache", "budget"]).is_ok());
    assert!(
        Cli::try_parse_from([
            "velnor-ci",
            "cache",
            "budget",
            "--budget-bytes",
            "8000000001"
        ])
        .is_err()
    );
}

#[test]
fn bounded_trim_leaves_later_pages_but_repository_budget_requires_and_uses_them() {
    let large = || {
        let api = Api::new();
        let template = api.entries.borrow()[0].clone();
        *api.entries.borrow_mut() = (1..=2001)
            .map(|id| {
                let mut entry = template.clone();
                entry["id"] = json!(id);
                entry
            })
            .collect();
        api
    };
    for count in [2000, 2001] {
        let api = large();
        api.entries.borrow_mut().truncate(count);
        let report = trim(&api, &repo(), "refs/heads/main", "velnor-dlt-", 2, 0)
            .unwrap_or_else(|e| unreachable!("{e}"));
        let ids: Vec<_> = api
            .entries
            .borrow()
            .iter()
            .map(|entry| entry["id"].as_u64().unwrap_or(0))
            .collect();
        let expected: Vec<_> = (1999..=u64::try_from(count).unwrap_or(0)).collect();
        assert_eq!(ids, expected);
        assert_eq!(report.contains("::warning::"), count > 2000, "{report}");
    }
    let api = large();
    budget(&api, &repo(), 0, 0, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(api.entries.borrow().is_empty());
    let mut api = large();
    api.fail = Some("caches?&per_page=100&page=21".to_owned());
    assert!(budget(&api, &repo(), 0, 0, false).is_err());
    assert_eq!(api.entries.borrow().len(), 2001);
}

#[test]
fn the_budget_deletes_an_older_target_family_and_keeps_a_recently_restored_sources_lock() {
    let host = "Linux-X64";
    let lock = |id: u64| format!("velnor-src-{host}-all.roots-{id:064x}");
    let target = |compat: char, id: u64| {
        format!(
            "velnor-tgt-rust-lint.lab.{host}-{}-{id:064x}-{}-{id}",
            compat.to_string().repeat(16),
            "c".repeat(40)
        )
    };
    let row = |id: u64, key: String, accessed: &str| json!({"id":id,"key":key,"ref":"refs/heads/main","created_at":format!("2026-11-17T00:00:{id:02}Z"),"last_accessed_at":accessed,"size_in_bytes":10});
    let build = |restored: &str| {
        let api = Api::new();
        *api.entries.borrow_mut() = vec![
            row(1, lock(1), restored),
            row(2, lock(2), "2026-11-17T00:00:00Z"),
            row(3, lock(3), "2026-11-17T00:00:00Z"),
            row(4, target('a', 4), "2026-11-17T00:00:00Z"),
            row(5, target('b', 5), "2026-11-17T00:00:00Z"),
        ];
        api
    };
    let ids = |api: &Api| -> Vec<u64> {
        api.entries
            .borrow()
            .iter()
            .filter_map(|entry| entry["id"].as_u64())
            .collect()
    };
    let now = velnor_ci::time::parse_utc("2026-11-17T12:00:00Z").unwrap_or(0);
    let api = build("2026-11-17T06:00:00Z");
    budget(&api, &repo(), now, 0, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(ids(&api), [1, 2, 3, 5]);
    let api = build("2026-11-15T06:00:00Z");
    budget(&api, &repo(), now, 0, false).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(ids(&api), [2, 3, 5]);
}
