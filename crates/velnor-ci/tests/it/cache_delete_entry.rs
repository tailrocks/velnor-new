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
    failing_ref: Option<&'static str>,
    second_page: Option<Vec<Value>>,
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
        if self.failing_ref.is_some() && reference.as_deref() == self.failing_ref {
            return Err(CiError::new("GitHub answered 502"));
        }
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
        if let (2, Some(second)) = (page, &self.second_page) {
            return Ok(json!({"total_count":total,"actions_caches":second}).to_string());
        }
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
        failing_ref: None,
        second_page: None,
        asked: RefCell::default(),
        deleted: RefCell::default(),
    }
}

fn refs(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
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
    let report =
        delete_entry(&api, &repo(), KEY, &refs(&[MERGE])).unwrap_or_else(|e| unreachable!("{e}"));
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
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MAIN]))
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
    delete_entry(&api, &repo(), KEY, &refs(&[MAIN])).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [21]);
}

#[test]
fn a_key_without_any_entry_on_the_ref_is_refused() {
    let api = api(vec![entry(11, KEY, MAIN, VERSION_A)]);
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MERGE]))
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
    assert!(delete_entry(&api, &repo(), KEY, &refs(&[MAIN])).is_err());
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn an_entry_on_another_ref_is_ignored_even_when_the_listing_returns_it() {
    let mut api = api(vec![
        entry(31, KEY, MAIN, VERSION_A),
        entry(32, KEY, MERGE, VERSION_A),
    ]);
    api.filters_by_ref = false;
    delete_entry(&api, &repo(), KEY, &refs(&[MAIN])).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [31]);
}

#[test]
fn a_match_on_the_second_page_is_found_and_deleted() {
    let mut entries: Vec<Value> = (1..=PAGE_SIZE as u64)
        .map(|id| entry(id, &format!("{KEY}-{id}"), MAIN, VERSION_A))
        .collect();
    entries.push(entry(500, KEY, MAIN, VERSION_B));
    let api = api(entries);
    delete_entry(&api, &repo(), KEY, &refs(&[MAIN])).unwrap_or_else(|e| unreachable!("{e}"));
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
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("an ambiguous match must be refused"));
    let text = error.to_string();
    assert!(text.contains("ID 1 ") && text.contains("ID 500 "), "{text}");
    assert!(api.deleted.borrow().is_empty());
}

fn both_refs() -> Api {
    api(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_A),
    ])
}

fn split_versions() -> Api {
    api(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_B),
    ])
}

#[test]
fn the_same_version_on_two_refs_deletes_only_the_entry_on_the_first_ref() {
    let api = both_refs();
    delete_entry(&api, &repo(), KEY, &refs(&[MERGE, MAIN])).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [12]);
}

#[test]
fn the_order_of_the_refs_decides_which_entry_is_deleted() {
    let api = both_refs();
    delete_entry(&api, &repo(), KEY, &refs(&[MAIN, MERGE])).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [11]);
}

#[test]
fn the_key_only_on_a_later_ref_deletes_that_entry() {
    let api = api(vec![entry(11, KEY, MAIN, VERSION_A)]);
    delete_entry(&api, &repo(), KEY, &refs(&[MERGE, MAIN])).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(*api.deleted.borrow(), [11]);
}

#[test]
fn two_entries_on_the_first_ref_refuse_and_delete_nothing() {
    let api = api(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_A),
        entry(13, KEY, MERGE, VERSION_B),
    ]);
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MERGE, MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("two entries on the first ref must be refused"));
    let text = error.to_string();
    assert!(text.contains("ID 12 ") && text.contains("ID 13 "), "{text}");
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn a_failing_listing_of_the_first_ref_refuses_instead_of_skipping_to_the_next() {
    let mut api = both_refs();
    api.failing_ref = Some(MERGE);
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MERGE, MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("a failed listing must refuse"));
    assert!(error.to_string().contains("502"), "{error}");
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn no_refs_and_an_empty_ref_among_them_are_refused() {
    let api = both_refs();
    assert!(delete_entry(&api, &repo(), KEY, &[]).is_err());
    assert!(delete_entry(&api, &repo(), KEY, &refs(&[MERGE, ""])).is_err());
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn a_key_with_no_entry_on_any_ref_names_every_ref_searched() {
    let api = api(vec![entry(11, "other", MAIN, VERSION_A)]);
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MERGE, MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("no entry anywhere must be refused"));
    assert_eq!(
        error.to_string(),
        format!("no cache entry has the key {KEY} on {MERGE} or {MAIN}; nothing was deleted")
    );
}

#[test]
fn a_different_version_on_a_later_ref_refuses_and_names_both_entries() {
    let api = split_versions();
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MERGE, MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("a version split across refs must be refused"));
    let text = error.to_string();
    assert!(
        text.contains(&format!("ID 12 version {VERSION_B} on {MERGE}"))
            && text.contains(&format!("ID 11 version {VERSION_A} on {MAIN}"))
            && text.contains("nothing was deleted")
            && text.contains("/repos/tailrocks/velnor-new/actions/caches/<ID>"),
        "{text}"
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn the_refusal_does_not_depend_on_which_ref_comes_first() {
    let api = split_versions();
    assert!(delete_entry(&api, &repo(), KEY, &refs(&[MAIN, MERGE])).is_err());
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn a_failing_listing_of_a_later_ref_refuses_and_deletes_nothing() {
    let mut api = both_refs();
    api.failing_ref = Some(MAIN);
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MERGE, MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("a failed listing of any ref must refuse"));
    assert!(error.to_string().contains("502"), "{error}");
    assert!(api.deleted.borrow().is_empty());
}

fn long_listing(second_page: Vec<Value>) -> Api {
    let mut entries = vec![entry(1, KEY, MAIN, VERSION_A)];
    entries.extend(
        (2..=PAGE_SIZE as u64).map(|id| entry(id, &format!("{KEY}-{id}"), MAIN, VERSION_A)),
    );
    entries.push(entry(500, KEY, MAIN, VERSION_B));
    let mut api = api(entries);
    api.second_page = Some(second_page);
    api
}

#[test]
fn a_repeated_id_between_pages_refuses_and_deletes_nothing() {
    let api = long_listing(vec![entry(
        PAGE_SIZE as u64,
        &format!("{KEY}-{PAGE_SIZE}"),
        MAIN,
        VERSION_A,
    )]);
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("a shifted listing must be refused"));
    assert!(error.to_string().contains("repeats"), "{error}");
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn a_collected_count_that_differs_from_the_total_refuses_and_deletes_nothing() {
    let api = long_listing(vec![
        entry(600, &format!("{KEY}-600"), MAIN, VERSION_A),
        entry(700, &format!("{KEY}-700"), MAIN, VERSION_A),
    ]);
    let error = delete_entry(&api, &repo(), KEY, &refs(&[MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("a listing that does not add up must be refused"));
    assert!(error.to_string().contains("total"), "{error}");
    assert!(api.deleted.borrow().is_empty());
}
