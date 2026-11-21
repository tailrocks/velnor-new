// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};
use velnor_ci::io::cache_purge::purge_private;

struct Api {
    entries: RefCell<Vec<Value>>,
    asked: RefCell<Vec<String>>,
    deleted: RefCell<Vec<u64>>,
    fail_delete: Option<u64>,
    listings: Cell<usize>,
    arrivals: RefCell<VecDeque<Vec<Value>>>,
    running: RefCell<Vec<(&'static str, u64)>>,
}

fn entry(id: u64, key: &str, reference: &str) -> Value {
    json!({"id":id,"key":key,"ref":reference,"created_at":"2026-11-17T00:00:00Z","last_accessed_at":"2026-11-17T00:00:00Z","size_in_bytes":10})
}

impl Api {
    fn new(entries: Vec<Value>) -> Self {
        Self {
            entries: RefCell::new(entries),
            asked: RefCell::default(),
            deleted: RefCell::default(),
            fail_delete: None,
            listings: Cell::default(),
            arrivals: RefCell::default(),
            running: RefCell::default(),
        }
    }

    fn with_runs(entries: Vec<Value>, runs: &[(&'static str, u64)]) -> Self {
        let api = Self::new(entries);
        *api.running.borrow_mut() = runs.to_vec();
        api
    }

    fn arriving_after_each_relist(batches: Vec<Vec<Value>>) -> Self {
        let api = Self::new(mixed());
        *api.arrivals.borrow_mut() = batches.into();
        api
    }
}

impl GitHub for Api {
    fn get(&self, path: &str) -> Result<String, CiError> {
        if path.contains("/actions/runs?") {
            let status = path
                .split("status=")
                .nth(1)
                .and_then(|rest| rest.split('&').next())
                .ok_or_else(|| CiError::new("unexpected run listing"))?;
            let total: u64 = self
                .running
                .borrow()
                .iter()
                .filter(|(running, _)| *running == status)
                .map(|(_, count)| count)
                .sum();
            return Ok(json!({"total_count":total,"workflow_runs":[]}).to_string());
        }
        self.asked.borrow_mut().push(path.to_owned());
        let page: usize = path
            .rsplit_once("page=")
            .and_then(|(_, page)| page.parse().ok())
            .ok_or_else(|| CiError::new("missing page"))?;
        if page == 1 {
            let listing = self.listings.get();
            self.listings.set(listing + 1);
            if listing > 0
                && let Some(batch) = self.arrivals.borrow_mut().pop_front()
            {
                self.entries.borrow_mut().extend(batch);
            }
        }
        let entries = self.entries.borrow();
        let rows: Vec<_> = entries
            .iter()
            .skip((page - 1) * 100)
            .take(100)
            .cloned()
            .collect();
        Ok(json!({"total_count":entries.len(),"actions_caches":rows}).to_string())
    }
}

impl GitHubWrite for Api {
    fn wait(&self, _: Duration) {}
    fn upload(&self, _: &str, _: &[u8]) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn patch(&self, _: &str, _: &str) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn delete(&self, path: &str) -> Result<(), CiError> {
        let id: u64 = path
            .rsplit('/')
            .next()
            .and_then(|id| id.parse().ok())
            .ok_or_else(|| CiError::new("invalid id"))?;
        if self.fail_delete == Some(id) {
            return Err(CiError::new("HTTP 403"));
        }
        self.deleted.borrow_mut().push(id);
        self.entries.borrow_mut().retain(|entry| entry["id"] != id);
        Ok(())
    }
}

fn repo() -> Repo {
    Repo::parse("tailrocks/lab").unwrap_or_else(|e| unreachable!("{e}"))
}

fn mixed() -> Vec<Value> {
    vec![
        entry(
            1,
            "velnor-private-src-Linux-X64-all.roots-aa",
            "refs/heads/main",
        ),
        entry(
            2,
            "velnor-private-tgt-build.root.Linux-X64-bb-cc-1",
            "refs/heads/main",
        ),
        entry(
            3,
            "velnor-private-tgt-build.root.Linux-X64-bb-cc-2",
            "refs/pull/7/merge",
        ),
        entry(4, "velnor-src-Linux-X64-all.roots-aa", "refs/heads/main"),
        entry(
            5,
            "velnor-tgt-build.root.Linux-X64-bb-cc-1",
            "refs/heads/main",
        ),
        entry(
            6,
            "velnor-dlt-pr7-rust.root.Linux-X64-bb-12345678-1",
            "refs/pull/7/merge",
        ),
        entry(7, "other-velnor-private-src-x", "refs/heads/main"),
        entry(8, "velnor-private", "refs/heads/main"),
    ]
}

#[test]
fn dry_run_counts_private_entries_and_deletes_nothing() {
    let api = Api::new(mixed());
    let report = purge_private(&api, &repo(), false).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("3 entries"), "{report}");
    assert!(report.contains("30 bytes"), "{report}");
    assert!(report.contains("would delete 3 entries"), "{report}");
    assert!(report.contains("--yes"), "{report}");
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn yes_deletes_only_keys_starting_with_the_private_prefix_on_every_ref() {
    let api = Api::new(mixed());
    let report = purge_private(&api, &repo(), true).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("deleted 3 entries"), "{report}");
    assert_eq!(*api.deleted.borrow(), vec![1, 2, 3]);
    let left: Vec<_> = api
        .entries
        .borrow()
        .iter()
        .map(|e| e["id"].clone())
        .collect();
    assert_eq!(
        left,
        vec![4, 5, 6, 7, 8]
            .into_iter()
            .map(Value::from)
            .collect::<Vec<_>>()
    );
    let asked = api.asked.borrow();
    assert!(
        asked
            .iter()
            .all(|path| path.contains("key=velnor-private-") && !path.contains("ref=")),
        "{asked:?}"
    );
}

#[test]
fn every_page_of_the_listing_is_purged() {
    let entries = (1..=250)
        .map(|id| {
            entry(
                id,
                &format!("velnor-private-tgt-job.root.Linux-X64-{id}"),
                "refs/heads/main",
            )
        })
        .collect();
    let api = Api::new(entries);
    let report = purge_private(&api, &repo(), true).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("deleted 250 entries"), "{report}");
    assert!(api.entries.borrow().is_empty());
}

#[test]
fn a_repository_without_private_entries_reports_zero_and_succeeds() {
    let api = Api::new(vec![entry(
        4,
        "velnor-src-Linux-X64-all.roots-aa",
        "refs/heads/main",
    )]);
    let report = purge_private(&api, &repo(), true).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("0 entries"), "{report}");
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn a_failed_delete_is_an_error() {
    let mut api = Api::new(mixed());
    api.fail_delete = Some(2);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some("deleted 1 of 3; failed at entry ID 2: HTTP 403")
    );
    assert_eq!(*api.deleted.borrow(), vec![1]);
}

#[test]
fn the_command_is_a_dry_run_unless_yes_is_passed() {
    use clap::Parser;
    use velnor_ci::cli::{CacheCommand, Cli, Command};
    let parse = |extra: &[&str]| {
        let mut args = vec!["velnor-ci", "cache", "purge-private"];
        args.extend(extra);
        match Cli::try_parse_from(args).map(|cli| cli.command) {
            Ok(Command::Cache(CacheCommand::PurgePrivate(args))) => Some(args.yes),
            _ => None,
        }
    };
    assert_eq!(parse(&[]), Some(false));
    assert_eq!(parse(&["--repo", "tailrocks/lab"]), Some(false));
    assert_eq!(parse(&["--yes"]), Some(true));
}

#[test]
fn a_write_that_lands_during_the_purge_is_deleted_on_a_later_pass() {
    let late = vec![entry(
        9,
        "velnor-private-tgt-late.root.Linux-X64-cc-1",
        "refs/heads/main",
    )];
    let api = Api::arriving_after_each_relist(vec![late]);
    let report = purge_private(&api, &repo(), true).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("deleted 4 entries"), "{report}");
    assert_eq!(*api.deleted.borrow(), vec![1, 2, 3, 9]);
    assert!(api.entries.borrow().iter().all(|e| e["id"] != 9));
}

#[test]
fn a_write_that_keeps_arriving_fails_after_the_bounded_passes() {
    let batches = (100..110)
        .map(|id| {
            vec![entry(
                id,
                &format!("velnor-private-tgt-late.root.Linux-X64-{id}"),
                "refs/heads/main",
            )]
        })
        .collect();
    let api = Api::arriving_after_each_relist(batches);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some("1 velnor-private- entries remain after the purge; run it again")
    );
    assert_eq!(*api.deleted.borrow(), vec![1, 2, 3, 100, 101]);
}

#[test]
fn in_progress_runs_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("in_progress", 2), ("queued", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "3 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn queued_runs_alone_refuse_the_purge() {
    let api = Api::with_runs(mixed(), &[("queued", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn waiting_runs_alone_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("waiting", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn action_required_runs_alone_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("action_required", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn requested_runs_alone_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("requested", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn pending_runs_alone_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("pending", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn a_dry_run_warns_about_running_workflows_and_succeeds() {
    let api = Api::with_runs(mixed(), &[("in_progress", 2)]);
    let report = purge_private(&api, &repo(), false).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("would delete 3 entries"), "{report}");
    assert!(
        report.contains("2 workflow runs are in progress; wait for them or disable workflows"),
        "{report}"
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn the_deleted_count_is_cumulative_across_passes() {
    let late = vec![entry(
        9,
        "velnor-private-tgt-late.root.Linux-X64-cc-1",
        "refs/heads/main",
    )];
    let mut api = Api::arriving_after_each_relist(vec![late]);
    *api.entries.borrow_mut() = vec![entry(
        1,
        "velnor-private-src-Linux-X64-all.roots-aa",
        "refs/heads/main",
    )];
    api.fail_delete = Some(9);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some("deleted 1 of 2; failed at entry ID 9: HTTP 403")
    );
    assert_eq!(*api.deleted.borrow(), vec![1]);
}
