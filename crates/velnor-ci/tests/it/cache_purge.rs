// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
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
        }
    }
}

impl GitHub for Api {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.asked.borrow_mut().push(path.to_owned());
        let page: usize = path
            .rsplit_once("page=")
            .and_then(|(_, page)| page.parse().ok())
            .ok_or_else(|| CiError::new("missing page"))?;
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
