// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};
use velnor_ci::io::cache_purge::purge_private;

mod if_public;
mod running;

struct Api {
    entries: RefCell<Vec<Value>>,
    asked: RefCell<Vec<String>>,
    deleted: RefCell<Vec<u64>>,
    fail_deletes: Vec<u64>,
    listings: Cell<usize>,
    arrivals: RefCell<VecDeque<Vec<Value>>>,
    running: RefCell<Vec<(&'static str, u64)>>,
    run_listings: Cell<usize>,
    promoted_to: Option<&'static str>,
    workflows: RefCell<Vec<&'static str>>,
    workflows_total: Cell<Option<u64>>,
    repository: RefCell<Option<Value>>,
    run_listing: RefCell<Option<Result<String, String>>>,
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
            fail_deletes: Vec::new(),
            listings: Cell::default(),
            arrivals: RefCell::default(),
            running: RefCell::default(),
            run_listings: Cell::default(),
            promoted_to: None,
            workflows: RefCell::default(),
            workflows_total: Cell::default(),
            run_listing: RefCell::default(),
            repository: RefCell::default(),
        }
    }

    fn with_runs(entries: Vec<Value>, runs: &[(&'static str, u64)]) -> Self {
        let api = Self::new(entries);
        *api.running.borrow_mut() = runs.to_vec();
        api
    }

    fn with_run_listing(entries: Vec<Value>, listing: Result<&str, &str>) -> Self {
        let api = Self::new(entries);
        *api.run_listing.borrow_mut() = Some(listing.map(str::to_owned).map_err(str::to_owned));
        api
    }

    fn with_workflows(entries: Vec<Value>, states: &[&'static str]) -> Self {
        let api = Self::new(entries);
        *api.workflows.borrow_mut() = states.to_vec();
        api
    }

    fn with_incomplete_workflow_listing(entries: Vec<Value>, listed: &[&'static str]) -> Self {
        let api = Self::with_workflows(entries, listed);
        api.workflows_total.set(Some(listed.len() as u64 + 1));
        api
    }

    fn with_repository(entries: Vec<Value>, private: bool, visibility: &str) -> Self {
        let api = Self::new(entries);
        *api.repository.borrow_mut() =
            Some(json!({"full_name":"tailrocks/lab","private":private,"visibility":visibility}));
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
        if path == "/repos/tailrocks/lab" {
            return self
                .repository
                .borrow()
                .as_ref()
                .map(Value::to_string)
                .ok_or_else(|| CiError::new("HTTP 500"));
        }
        if path.contains("/actions/workflows?") {
            let states = self.workflows.borrow();
            let rows: Vec<Value> = states.iter().map(|state| json!({"state": state})).collect();
            let total = self.workflows_total.get().unwrap_or(rows.len() as u64);
            return Ok(json!({"total_count":total,"workflows":rows}).to_string());
        }
        if path.contains("/actions/runs?") {
            if let Some(listing) = self.run_listing.borrow().clone() {
                return listing.map_err(CiError::new);
            }
            let status = path
                .split("status=")
                .nth(1)
                .and_then(|rest| rest.split('&').next())
                .ok_or_else(|| CiError::new("unexpected run listing"))?;
            let call = self.run_listings.get();
            self.run_listings.set(call + 1);
            let total: u64 = self
                .running
                .borrow()
                .iter()
                .map(|(running, count)| match self.promoted_to {
                    Some(to) if call > 0 && *running == "queued" => (to, count),
                    _ => (*running, count),
                })
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
        if self.fail_deletes.contains(&id) {
            return Err(CiError::new("HTTP 403"));
        }
        self.deleted.borrow_mut().push(id);
        self.entries.borrow_mut().retain(|entry| entry["id"] != id);
        Ok(())
    }
}

fn failed_ids(error: &CiError) -> Vec<u64> {
    error
        .to_string()
        .split("ID ")
        .skip(1)
        .filter_map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
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
    api.fail_deletes = vec![2];
    let error = purge_private(&api, &repo(), true).err();
    assert_eq!(error.as_ref().map(failed_ids), Some(vec![2]));
    assert_eq!(*api.deleted.borrow(), vec![1, 3]);
}

#[test]
fn every_failed_delete_is_reported_together_by_yes() {
    let mut api = Api::new(mixed());
    api.fail_deletes = vec![1, 3];
    let error = purge_private(&api, &repo(), true).err();
    assert_eq!(error.as_ref().map(failed_ids), Some(vec![1, 3]));
    assert_eq!(*api.deleted.borrow(), vec![2]);
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
    api.fail_deletes = vec![9];
    let error = purge_private(&api, &repo(), true).err();
    assert_eq!(error.as_ref().map(failed_ids), Some(vec![9]));
    assert_eq!(*api.deleted.borrow(), vec![1]);
}
