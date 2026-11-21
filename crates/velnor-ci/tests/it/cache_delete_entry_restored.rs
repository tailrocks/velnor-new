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
        text.contains("ID 13")
            && text.contains("saved within 5 minutes before this run's restore or after it"),
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
        text.contains(
            "was saved at 2026-11-18T01:00:00Z, which is not before the cutoff 2026-11-18T00:00:00Z"
        ),
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
            text.contains("cannot be shown to be the entry this run read"),
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

#[test]
fn a_refusal_names_why_and_prints_the_rerun_command_with_a_stamp_after_the_save_time() {
    let api = api(vec![entry_saved(
        13,
        KEY,
        MAIN,
        VERSION_A,
        "2026-11-18T00:02:00.250Z",
    )]);
    let text = refused(&api, RESTORED_BEFORE);
    assert_eq!(
        text,
        format!(
            "the entry ID 13 ({KEY}) was saved at 2026-11-18T00:02:00.250Z, which is not before the cutoff 2026-11-18T00:00:00Z. The cutoff is the time this run's restore began minus a safety margin of 5 minutes, so the entry was saved within 5 minutes before this run's restore or after it, and it cannot be shown to be the entry this run read; nothing was deleted. Use this only if you know the entry was saved before this run's restore began: rerun step 1 with an explicit --restored-before later than the save time: `velnor-ci cache delete-entry --repo 'tailrocks/velnor-new' --key '{KEY}' --ref '{MAIN}' --restored-before '2026-11-18T00:02:01Z'` (with the GITHUB_API_URL and GITHUB_TOKEN of this run)"
        )
    );
    let rerun = resolve_entry(&api, &repo(), KEY, &refs(&[MAIN]), "2026-11-18T00:02:01Z")
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(rerun.id, 13);
}

#[test]
fn a_key_that_this_message_cannot_print_as_typed_gets_no_literal_command() {
    let odd = "velnor-a%b";
    let api = api(vec![entry_saved(
        13,
        odd,
        MAIN,
        VERSION_A,
        "2026-11-18T00:00:05Z",
    )]);
    let error = resolve_entry(&api, &repo(), odd, &refs(&[MAIN]), RESTORED_BEFORE)
        .err()
        .unwrap_or_else(|| unreachable!("the entry must be refused"))
        .to_string();
    assert!(
        error.contains("the same command with --restored-before '2026-11-18T00:00:06Z'")
            && !error.contains("velnor-ci cache delete-entry"),
        "{error}"
    );
}

#[test]
fn an_exact_id_that_vanishes_before_the_delete_is_reported_as_gone_not_deleted() {
    let api = api(vec![entry(12, KEY, MAIN, VERSION_A)]);
    api.vanish_before_delete(12);
    let error = delete_by_id(&api, &repo(), KEY, 12)
        .err()
        .unwrap_or_else(|| unreachable!("a raced-away ID must not be reported as deleted"));
    assert_eq!(
        error.to_string(),
        format!("entry 12 ({KEY}) was already gone; nothing was deleted")
    );
}
