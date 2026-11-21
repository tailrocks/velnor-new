// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::io::cache_delete_entry::{delete_by_id, resolve_entry};

use crate::cache_delete_entry::{KEY, MAIN, MERGE, VERSION_A, api, entry, refs, repo};

#[test]
fn an_entry_the_api_returns_without_a_version_is_refused_and_named() {
    let mut bare = entry(41, KEY, MAIN, VERSION_A);
    if let Some(row) = bare.as_object_mut() {
        row.remove("version");
    }
    for asked in [&[MAIN][..], &[MERGE, MAIN][..]] {
        let api = api(vec![bare.clone()]);
        let error = resolve_entry(&api, &repo(), KEY, &refs(asked))
            .err()
            .unwrap_or_else(|| unreachable!("an entry without a version must be refused"));
        assert!(error.to_string().contains("ID 41"), "{error}");
        assert!(api.deleted.borrow().is_empty());
    }
}

#[test]
fn resolving_never_issues_a_delete_even_for_a_clear_candidate() {
    let api = api(vec![entry(12, KEY, MERGE, VERSION_A)]);
    let found = resolve_entry(&api, &repo(), KEY, &refs(&[MERGE, MAIN]))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.id, 12);
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn a_stale_id_is_refused_after_its_entry_is_gone() {
    let api = api(vec![entry(12, KEY, MERGE, VERSION_A)]);
    delete_by_id(&api, &repo(), KEY, 12).unwrap_or_else(|e| unreachable!("{e}"));
    let error = delete_by_id(&api, &repo(), KEY, 12)
        .err()
        .unwrap_or_else(|| unreachable!("a stale ID must be refused"));
    assert!(error.to_string().contains("ID 12"), "{error}");
    assert_eq!(*api.deleted.borrow(), [12]);
}

#[test]
fn an_id_under_another_key_is_refused_and_nothing_is_deleted() {
    let longer = format!("{KEY}-other");
    let api = api(vec![
        entry(20, &longer, MAIN, VERSION_A),
        entry(21, "unrelated", MAIN, VERSION_A),
    ]);
    for id in [20, 21] {
        let error = delete_by_id(&api, &repo(), KEY, id)
            .err()
            .unwrap_or_else(|| unreachable!("an ID under another key must be refused"));
        assert!(error.to_string().contains("nothing was deleted"), "{error}");
    }
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn the_right_id_with_the_right_key_deletes_exactly_that_entry() {
    let api = api(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_A),
    ]);
    let report = delete_by_id(&api, &repo(), KEY, 12).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("deleted cache entry ID 12"), "{report}");
    assert_eq!(*api.deleted.borrow(), [12]);
}

#[test]
fn an_empty_key_is_refused_for_a_delete_by_id() {
    let api = api(vec![entry(12, "", MERGE, VERSION_A)]);
    let error = delete_by_id(&api, &repo(), "", 12)
        .err()
        .unwrap_or_else(|| unreachable!("an empty key must be refused"));
    assert_eq!(error.to_string(), "pass the exact cache key and its refs");
    assert!(api.deleted.borrow().is_empty());
}
