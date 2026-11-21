// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::io::cache_delete_entry::delete_by_id;

use crate::cache_delete_entry::{
    KEY, MAIN, MERGE, VERSION_A, api, entry, refs, repo, resolve_entry,
};

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
fn an_empty_key_is_refused_for_a_delete_by_id() {
    let api = api(vec![entry(12, "", MERGE, VERSION_A)]);
    let error = delete_by_id(&api, &repo(), "", 12)
        .err()
        .unwrap_or_else(|| unreachable!("an empty key must be refused"));
    assert_eq!(error.to_string(), "pass the exact cache key and its refs");
    assert!(api.deleted.borrow().is_empty());
}
