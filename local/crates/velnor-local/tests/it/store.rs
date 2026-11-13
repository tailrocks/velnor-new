// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{
    Access, ENTRY_LIFETIME_SECONDS, Event, REPOSITORY_LIMIT_BYTES, access_for,
};
use velnor_local::store::{Index, Lookup, RESERVATION_SECONDS};

const SAVED: u64 = 1_794_000_000;

fn main_access() -> Access {
    access_for(
        &Event::Push {
            branch: "main".to_owned(),
        },
        "main",
    )
}

fn at<'a>(access: &'a Access, key: &'a str) -> Lookup<'a> {
    Lookup {
        repository: "o/r",
        access,
        key,
        version: "v",
    }
}

fn reserve(index: &mut Index, key: &str, blob: &str, now: u64) -> Option<(u64, Vec<String>)> {
    let access = main_access();
    let (record, replaced) = index.create(&at(&access, key), blob.to_owned(), now).ok()?;
    Some((
        record.id,
        replaced.into_iter().map(|record| record.blob).collect(),
    ))
}

fn saved_entry() -> Index {
    let mut index = Index::default();
    let (id, _) = reserve(&mut index, "tools", "saved", SAVED).unwrap_or_default();
    assert_eq!(
        index.commit(id, 10, SAVED, REPOSITORY_LIMIT_BYTES),
        Vec::new()
    );
    index
}

#[test]
fn an_entry_unused_for_more_than_seven_days_is_a_miss_and_can_be_saved_again() {
    let access = main_access();
    let expired = SAVED + ENTRY_LIFETIME_SECONDS + 1;

    let on_the_last_day = saved_entry().restore(&at(&access, "tools"), &[], expired - 1);
    let a_second_later = saved_entry().restore(&at(&access, "tools"), &[], expired);
    let saved_again = reserve(&mut saved_entry(), "tools", "new", expired);

    assert_eq!(
        on_the_last_day.map(|record| record.blob),
        Some("saved".to_owned())
    );
    assert_eq!(a_second_later, None);
    assert_eq!(
        saved_again.map(|(_, replaced)| replaced),
        Some(vec!["saved".to_owned()])
    );
}

#[test]
fn an_unfinished_reservation_lapses_after_an_hour_and_its_upload_address_dies() {
    let access = main_access();
    let mut index = Index::default();
    assert!(reserve(&mut index, "tools", "first", SAVED).is_some());

    assert_eq!(
        reserve(&mut index, "tools", "second", SAVED + RESERVATION_SECONDS),
        None
    );
    let retaken = reserve(
        &mut index,
        "tools",
        "third",
        SAVED + RESERVATION_SECONDS + 1,
    );

    assert_eq!(
        retaken.map(|(_, replaced)| replaced),
        Some(vec!["first".to_owned()])
    );
    assert_eq!(
        index
            .pending(&at(&access, "tools"))
            .map(|record| record.blob.clone()),
        Some("third".to_owned())
    );
    assert_eq!(index.blob("first", false), None);
}

#[test]
fn a_save_over_the_limit_does_not_evict_another_upload_in_progress() {
    let access = main_access();
    let mut index = Index::default();
    assert!(reserve(&mut index, "uploading", "a", SAVED).is_some());
    let (id, _) = reserve(&mut index, "large", "b", SAVED + 1).unwrap_or_default();

    let gone: Vec<String> = index
        .commit(id, 2_000, SAVED + 1, 1_000)
        .into_iter()
        .map(|record| record.blob)
        .collect();

    assert_eq!(gone, vec!["b".to_owned()]);
    assert!(index.pending(&at(&access, "uploading")).is_some());
}
