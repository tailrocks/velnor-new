// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::cell::RefCell;
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};
use velnor_ci::io::cache_api::{budget, trim};

struct Api {
    entries: RefCell<Vec<Value>>,
    prs: Vec<Value>,
    fail: Option<String>,
    fail_delete: bool,
    changed_total: bool,
    incomplete: bool,
    status: Value,
    confirmations: RefCell<Vec<String>>,
}

impl Api {
    fn new() -> Self {
        let entries = (1..=101).map(|id| json!({"id":id,"key":format!("velnor-dlt-rust-root-Linux-X64-0123456789abcdef-12345678-{id}"),"ref":"refs/pull/101/merge","created_at":"2026-11-17T00:00:00Z","last_accessed_at":"2026-11-17T00:00:00Z","size_in_bytes":10})).collect();
        Self {
            entries: RefCell::new(entries),
            prs: (1..=101).map(|number| json!({"number":number})).collect(),
            fail: None,
            fail_delete: false,
            changed_total: false,
            incomplete: false,
            status: json!({"state":"open"}),
            confirmations: RefCell::default(),
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
    fn upload(&self, _: &str, _: &[u8]) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn patch(&self, _: &str, _: &str) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn delete(&self, path: &str) -> Result<(), CiError> {
        if self.fail_delete {
            return Err(CiError::new("delete failed"));
        }
        let id: u64 = path
            .rsplit('/')
            .next()
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| CiError::new("invalid id"))?;
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
    trim(&api, &repo(), "refs/heads/main", "velnor-dlt-", 2)
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
    assert!(trim(&api, &repo(), "refs/heads/main", "velnor-dlt-", 2).is_err());
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
