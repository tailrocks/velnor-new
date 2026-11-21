// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};
use velnor_ci::io::cache_delete_entry::delete_entry;

const KEY: &str = "velnor-tools-Linux-X64-mise2026.11.10-aa";
const MAIN: &str = "refs/heads/main";
const MERGE: &str = "refs/pull/7/merge";
const VERSION_A: &str = "a1a1";
const VERSION_B: &str = "b2b2";
const PAGE_SIZE: usize = 100;

struct Api {
    entries: Vec<Value>,
    filters_by_ref: bool,
    asked: RefCell<Vec<String>>,
    deleted: RefCell<Vec<u64>>,
}

fn entry(id: u64, key: &str, reference: &str, version: &str) -> Value {
    json!({"id":id,"key":key,"ref":reference,"version":version,"created_at":"2026-11-17T00:00:00Z","last_accessed_at":"2026-11-17T00:00:00Z","size_in_bytes":10})
}

fn query<'a>(path: &'a str, name: &str) -> Option<&'a str> {
    path.split_once('?')?
        .1
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
}

impl GitHub for Api {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.asked.borrow_mut().push(path.to_owned());
        let key = query(path, "key").unwrap_or_default();
        let reference = query(path, "ref").map(|text| text.replace("%2F", "/"));
        let page: usize = query(path, "page")
            .and_then(|text| text.parse().ok())
            .unwrap_or(1);
        let rows: Vec<&Value> = self
            .entries
            .iter()
            .filter(|row| row["key"].as_str().is_some_and(|k| k.starts_with(key)))
            .filter(|row| {
                !self.filters_by_ref || reference.as_deref().is_none_or(|r| row["ref"] == r)
            })
            .collect();
        let total = rows.len();
        let rows: Vec<&Value> = rows
            .into_iter()
            .skip(page.saturating_sub(1) * PAGE_SIZE)
            .take(PAGE_SIZE)
            .collect();
        Ok(json!({"total_count":total,"actions_caches":rows}).to_string())
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
        let id = path.rsplit('/').next().and_then(|id| id.parse().ok());
        self.deleted
            .borrow_mut()
            .push(id.ok_or_else(|| CiError::new("invalid id"))?);
        Ok(())
    }
}

fn api(entries: Vec<Value>) -> Api {
    Api {
        entries,
        filters_by_ref: true,
        asked: RefCell::default(),
        deleted: RefCell::default(),
    }
}

fn repo() -> Repo {
    Repo::parse("tailrocks/velnor-new").unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn exactly_one_entry_on_the_ref_is_deleted_by_its_id() {
    let api = api(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_B),
    ]);
    let report = delete_entry(&api, &repo(), KEY, MERGE).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [12]);
    assert!(report.contains("12"), "{report}");
    let asked = api.asked.borrow();
    assert!(
        asked[0].starts_with(&format!(
            "/repos/tailrocks/velnor-new/actions/caches?key={KEY}&ref=refs%2Fpull%2F7%2Fmerge"
        )),
        "{asked:?}"
    );
}

#[test]
fn two_versions_under_one_key_on_one_ref_are_refused_and_nothing_is_deleted() {
    let api = api(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MAIN, VERSION_B),
    ]);
    let error = delete_entry(&api, &repo(), KEY, MAIN)
        .err()
        .unwrap_or_else(|| unreachable!("two versions under one key must be refused"));
    let text = error.to_string();
    assert!(
        text.contains(&format!("ID 11 version {VERSION_A}"))
            && text.contains(&format!("ID 12 version {VERSION_B}")),
        "{text}"
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn an_entry_whose_key_only_starts_with_the_key_is_never_a_candidate() {
    let longer = format!("{KEY}-other");
    let api = api(vec![
        entry(20, &longer, MAIN, VERSION_A),
        entry(21, KEY, MAIN, VERSION_A),
    ]);
    delete_entry(&api, &repo(), KEY, MAIN).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [21]);
}

#[test]
fn a_key_without_any_entry_on_the_ref_is_refused() {
    let api = api(vec![entry(11, KEY, MAIN, VERSION_A)]);
    let error = delete_entry(&api, &repo(), KEY, MERGE)
        .err()
        .unwrap_or_else(|| unreachable!("a key with no entry on the ref must be refused"));
    assert_eq!(
        error.to_string(),
        format!("no cache entry has the key {KEY} on {MERGE}; nothing was deleted")
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn an_empty_listing_is_refused() {
    let api = api(Vec::new());
    assert!(delete_entry(&api, &repo(), KEY, MAIN).is_err());
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn an_entry_on_another_ref_is_ignored_even_when_the_listing_returns_it() {
    let mut api = api(vec![
        entry(31, KEY, MAIN, VERSION_A),
        entry(32, KEY, MERGE, VERSION_A),
    ]);
    api.filters_by_ref = false;
    delete_entry(&api, &repo(), KEY, MAIN).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [31]);
}

#[test]
fn a_match_on_the_second_page_is_found_and_deleted() {
    let mut entries: Vec<Value> = (1..=PAGE_SIZE as u64)
        .map(|id| entry(id, &format!("{KEY}-{id}"), MAIN, VERSION_A))
        .collect();
    entries.push(entry(500, KEY, MAIN, VERSION_B));
    let api = api(entries);
    delete_entry(&api, &repo(), KEY, MAIN).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [500]);
    assert!(
        api.asked
            .borrow()
            .iter()
            .any(|path| path.ends_with("page=2")),
        "{:?}",
        api.asked.borrow()
    );
}

#[test]
fn an_ambiguity_spanning_two_pages_deletes_nothing_and_names_both_ids() {
    let mut entries = vec![entry(1, KEY, MAIN, VERSION_A)];
    entries.extend(
        (2..=PAGE_SIZE as u64).map(|id| entry(id, &format!("{KEY}-{id}"), MAIN, VERSION_A)),
    );
    entries.push(entry(500, KEY, MAIN, VERSION_B));
    let api = api(entries);
    let error = delete_entry(&api, &repo(), KEY, MAIN)
        .err()
        .unwrap_or_else(|| unreachable!("an ambiguous match must be refused"));
    let text = error.to_string();
    assert!(text.contains("ID 1 ") && text.contains("ID 500 "), "{text}");
    assert!(api.deleted.borrow().is_empty());
}
