// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::io::cache_delete_entry::{delete_by_id, resolve_entry};

use crate::cache_delete_entry::{
    KEY, MAIN, RESTORED_BEFORE, VERSION_A, api, entry, entry_saved, refs, repo,
};

const SAVED_LATER: &str = "2026-11-18T01:00:00Z";

fn refused(api: &crate::cache_delete_entry::Api, restored_before: &str) -> String {
    resolve_entry(api, &repo(), KEY, &refs(&[MAIN]), restored_before)
        .err()
        .unwrap_or_else(|| unreachable!("the entry must be refused"))
        .to_string()
}

#[test]
fn a_stale_notice_cannot_resolve_an_entry_saved_after_its_restore() {
    let api = api(vec![entry(12, KEY, MAIN, VERSION_A)]);
    let first = resolve_entry(&api, &repo(), KEY, &refs(&[MAIN]), RESTORED_BEFORE)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(first.id, 12);
    delete_by_id(&api, &repo(), KEY, first.id).unwrap_or_else(|e| unreachable!("{e}"));
    api.save(entry_saved(13, KEY, MAIN, VERSION_A, SAVED_LATER));
    let text = refused(&api, RESTORED_BEFORE);
    assert!(
        text.contains("ID 13") && text.contains("not before this run's restore"),
        "{text}"
    );
    assert!(!text.contains("--id"), "{text}");
    assert_eq!(*api.deleted.borrow(), [12]);
}

#[test]
fn an_entry_evicted_and_saved_again_is_refused_for_the_older_restore() {
    let api = api(vec![entry(12, KEY, MAIN, VERSION_A)]);
    velnor_ci::github::GitHubWrite::delete(&api, "/repos/tailrocks/velnor-new/actions/caches/12")
        .unwrap_or_else(|e| unreachable!("{e}"));
    api.save(entry_saved(13, KEY, MAIN, VERSION_A, SAVED_LATER));
    let text = refused(&api, RESTORED_BEFORE);
    assert!(
        text.contains("was saved at 2026-11-18T01%3A00%3A00Z"),
        "{text}"
    );
    assert_eq!(*api.deleted.borrow(), [12]);
}

#[test]
fn an_entry_saved_at_the_stamp_or_one_second_after_is_refused() {
    for saved in [RESTORED_BEFORE, "2026-11-18T00:00:01Z"] {
        let api = api(vec![entry_saved(13, KEY, MAIN, VERSION_A, saved)]);
        let text = refused(&api, RESTORED_BEFORE);
        assert!(
            text.contains("not the entry this run read"),
            "{saved}: {text}"
        );
        assert!(api.deleted.borrow().is_empty());
    }
}

#[test]
fn an_entry_saved_one_second_before_the_stamp_is_resolved() {
    let api = api(vec![entry_saved(
        12,
        KEY,
        MAIN,
        VERSION_A,
        "2026-11-17T23:59:59Z",
    )]);
    let found = resolve_entry(&api, &repo(), KEY, &refs(&[MAIN]), RESTORED_BEFORE)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.id, 12);
}

#[test]
fn a_missing_or_unparsable_created_at_is_refused() {
    let mut missing = entry(12, KEY, MAIN, VERSION_A);
    if let Some(object) = missing.as_object_mut() {
        object.remove("created_at");
    }
    for row in [
        missing,
        entry_saved(12, KEY, MAIN, VERSION_A, ""),
        entry_saved(12, KEY, MAIN, VERSION_A, "yesterday"),
    ] {
        let api = api(vec![row]);
        let text = refused(&api, RESTORED_BEFORE);
        assert!(text.contains("no usable created_at"), "{text}");
        assert!(api.deleted.borrow().is_empty());
    }
}

#[test]
fn an_absent_or_unparsable_restored_before_is_refused() {
    let api = api(vec![entry(12, KEY, MAIN, VERSION_A)]);
    for stamp in ["", "yesterday", "2026-11-18"] {
        let text = refused(&api, stamp);
        assert!(text.contains("--restored-before"), "{stamp}: {text}");
    }
    assert!(api.deleted.borrow().is_empty());
}
