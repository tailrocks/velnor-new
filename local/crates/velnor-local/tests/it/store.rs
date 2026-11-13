// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{ENTRY_LIFETIME_SECONDS, Event, REPOSITORY_LIMIT_BYTES, access_for};
use velnor_local::store::{Index, Lookup, RESERVATION_SECONDS};

const SAVED: u64 = 1_794_000_000;

fn saved_entry() -> Index {
    let access = access_for(
        &Event::Push {
            branch: "main".to_owned(),
        },
        "main",
    );
    let at = Lookup {
        repository: "o/r",
        access: &access,
        key: "tools",
        version: "v",
    };
    let mut index = Index::default();
    let record = index
        .create(&at, "blob".to_owned(), SAVED)
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(
        index.commit(record.id, 10, SAVED, REPOSITORY_LIMIT_BYTES),
        Vec::new()
    );
    index
}

#[test]
fn an_entry_unused_for_more_than_seven_days_is_a_miss_without_any_save() {
    let access = access_for(
        &Event::Push {
            branch: "main".to_owned(),
        },
        "main",
    );
    let at = Lookup {
        repository: "o/r",
        access: &access,
        key: "tools",
        version: "v",
    };

    let on_the_last_day = saved_entry().restore(&at, &[], SAVED + ENTRY_LIFETIME_SECONDS);
    let a_second_later = saved_entry().restore(&at, &[], SAVED + ENTRY_LIFETIME_SECONDS + 1);

    assert_eq!(
        on_the_last_day.map(|record| record.key),
        Some("tools".to_owned())
    );
    assert_eq!(a_second_later, None);
}

#[test]
fn an_unfinished_reservation_lapses_after_an_hour() {
    let access = access_for(
        &Event::Push {
            branch: "main".to_owned(),
        },
        "main",
    );
    let at = Lookup {
        repository: "o/r",
        access: &access,
        key: "tools",
        version: "v",
    };
    let mut index = Index::default();
    let first = index
        .create(&at, "first".to_owned(), SAVED)
        .unwrap_or_else(|e| unreachable!("{e:?}"));

    assert!(
        index
            .create(&at, "second".to_owned(), SAVED + RESERVATION_SECONDS)
            .is_err()
    );
    let retaken = index
        .create(&at, "third".to_owned(), SAVED + RESERVATION_SECONDS + 1)
        .unwrap_or_else(|e| unreachable!("{e:?}"));

    assert_eq!(
        (retaken.id, retaken.created),
        (first.id, SAVED + RESERVATION_SECONDS + 1)
    );
    assert_eq!(
        index.pending(&at).map(|record| record.blob.clone()),
        Some(retaken.blob)
    );
}
