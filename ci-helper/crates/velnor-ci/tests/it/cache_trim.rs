// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use velnor_ci::cache::{stale_entries, trim};
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};

struct PagedCaches {
    entries: Vec<String>,
    asked: RefCell<Vec<String>>,
    deleted: RefCell<Vec<String>>,
}

fn query(path: &str, name: &str) -> usize {
    path.split(['?', '&'])
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

impl GitHub for PagedCaches {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.asked.borrow_mut().push(path.to_owned());
        let (page, size) = (query(path, "page"), query(path, "per_page"));
        let slice: Vec<&str> = self
            .entries
            .iter()
            .skip((page - 1) * size)
            .take(size)
            .map(String::as_str)
            .collect();
        Ok(format!(
            r#"{{"total_count":{},"actions_caches":[{}]}}"#,
            self.entries.len(),
            slice.join(",")
        ))
    }
}

impl GitHubWrite for PagedCaches {
    fn wait(&self, _: std::time::Duration) {}

    fn upload(&self, _: &str, _: &[u8]) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn patch(&self, _: &str, _: &str) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn delete(&self, path: &str) -> Result<(), CiError> {
        self.deleted.borrow_mut().push(path.to_owned());
        Ok(())
    }
}

fn paged(count: u64) -> PagedCaches {
    let entries = (1..=count)
        .map(|id| {
            format!(
                r#"{{"id":{id},"key":"velnor-tgt-rust-lab-Linux-X64-abcd-h-{id}","created_at":"2026-01-01T00:00:{:02}Z"}}"#,
                id % 60
            )
        })
        .collect();
    PagedCaches {
        entries,
        asked: RefCell::default(),
        deleted: RefCell::default(),
    }
}

#[test]
fn trim_reads_every_page_of_a_250_entry_family_and_deletes_all_but_the_newest_two() {
    let fake = paged(250);
    let repo = Repo::parse("tailrocks/velnor-lab").unwrap_or_else(|e| unreachable!("{e}"));
    let report = trim(
        &fake,
        &repo,
        "refs/heads/main",
        "velnor-tgt-rust-lab-Linux-X64-abcd-",
        2,
        0,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(fake.asked.borrow().len(), 3, "{:?}", fake.asked.borrow());
    for (index, page) in [1, 2, 3].iter().enumerate() {
        assert!(
            fake.asked.borrow()[index].contains(&format!("page={page}")),
            "{:?}",
            fake.asked.borrow()
        );
    }
    assert_eq!(fake.deleted.borrow().len(), 248);
    assert!(report.contains("250 entries"), "{report}");
    assert!(report.contains("deleted 248"), "{report}");
    assert!(!report.contains("warning"), "{report}");
}

#[test]
fn trim_stops_at_its_page_bound_and_warns_instead_of_reading_forever() {
    let fake = paged(100_000);
    let repo = Repo::parse("tailrocks/velnor-lab").unwrap_or_else(|e| unreachable!("{e}"));
    let report = trim(
        &fake,
        &repo,
        "refs/heads/main",
        "velnor-tgt-rust-lab-Linux-X64-abcd-",
        2,
        0,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let pages = fake.asked.borrow().len();
    assert!((2..=50).contains(&pages), "{pages}");
    assert!(report.contains("::warning::"), "{report}");
    assert!(report.contains("stopped after"), "{report}");
}

struct FakeCaches {
    listing: String,
    asked: RefCell<Vec<String>>,
    deleted: RefCell<Vec<String>>,
}

impl GitHub for FakeCaches {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.asked.borrow_mut().push(path.to_owned());
        Ok(self.listing.clone())
    }
}

impl GitHubWrite for FakeCaches {
    fn wait(&self, _: std::time::Duration) {}

    fn upload(&self, _: &str, _: &[u8]) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn patch(&self, _: &str, _: &str) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn delete(&self, path: &str) -> Result<(), CiError> {
        self.deleted.borrow_mut().push(path.to_owned());
        Ok(())
    }
}

fn listing() -> String {
    let entry = |id: u64, key: &str, day: u32| {
        format!(r#"{{"id":{id},"key":"{key}","created_at":"2026-11-{day:02}T10:00:00Z"}}"#)
    };
    let family = "velnor-tgt-rust-lint-lab-Linux-X64-abcd-";
    let other = "velnor-tgt-rust-lint-lab-Linux-X64-9999-";
    let entries = [
        entry(1, &format!("{family}h1-1"), 1),
        entry(5, &format!("{family}h1-5"), 5),
        entry(3, &format!("{family}h1-3"), 3),
        entry(4, &format!("{family}h1-4"), 4),
        entry(2, &format!("{family}h1-2"), 2),
        entry(9, &format!("{other}h1-9"), 1),
        entry(10, "velnor-tgt-rust-test-lab-Linux-X64-abcd-h1-1", 1),
    ];
    format!(
        r#"{{"total_count":7,"actions_caches":[{}]}}"#,
        entries.join(",")
    )
}

#[test]
fn trim_keeps_the_newest_two_of_one_family_and_compat_and_touches_nothing_else() {
    let fake = FakeCaches {
        listing: listing(),
        asked: RefCell::default(),
        deleted: RefCell::default(),
    };
    let repo = Repo::parse("tailrocks/velnor-lab").unwrap_or_else(|e| unreachable!("{e}"));
    let report = trim(
        &fake,
        &repo,
        "refs/heads/main",
        "velnor-tgt-rust-lint-lab-Linux-X64-abcd-",
        2,
        0,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        *fake.deleted.borrow(),
        [
            "/repos/tailrocks/velnor-lab/actions/caches/3",
            "/repos/tailrocks/velnor-lab/actions/caches/2",
            "/repos/tailrocks/velnor-lab/actions/caches/1",
        ]
    );
    assert!(report.contains("deleted 3"), "{report}");
    let asked = fake.asked.borrow()[0].clone();
    assert!(asked.contains("ref=refs%2Fheads%2Fmain"), "{asked}");
    assert!(
        asked.contains("key=velnor-tgt-rust-lint-lab-Linux-X64-abcd-"),
        "{asked}"
    );
}

#[test]
fn trim_deletes_nothing_when_the_family_has_no_more_than_the_kept_entries() {
    let entries =
        velnor_ci::cache::parse_entries(&listing()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        stale_entries(
            &entries,
            "velnor-tgt-rust-lint-lab-Linux-X64-9999-",
            2,
            None
        ),
        Vec::<u64>::new()
    );
    assert_eq!(
        stale_entries(&entries, "nothing-", 2, None),
        Vec::<u64>::new()
    );
}

fn fake(entries: &[(u64, &str, u32)]) -> FakeCaches {
    let rows: Vec<String> = entries
        .iter()
        .map(|(id, key, day)| {
            format!(r#"{{"id":{id},"key":"{key}","created_at":"2026-11-{day:02}T10:00:00Z"}}"#)
        })
        .collect();
    FakeCaches {
        listing: format!(
            r#"{{"total_count":{},"actions_caches":[{}]}}"#,
            rows.len(),
            rows.join(",")
        ),
        asked: RefCell::default(),
        deleted: RefCell::default(),
    }
}

fn deleted_ids(fake: &FakeCaches) -> Vec<u64> {
    let mut ids: Vec<u64> = fake
        .deleted
        .borrow()
        .iter()
        .filter_map(|path| path.rsplit('/').next()?.parse().ok())
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn trim_deletes_every_target_entry_of_an_older_compatibility_family_of_the_same_lane() {
    let now = "velnor-tgt-rust-lint.lab.Linux-X64-bbbbbbbbbbbbbbbb-";
    let old = "velnor-tgt-rust-lint.lab.Linux-X64-aaaaaaaaaaaaaaaa-";
    let other_lane = "velnor-tgt-rust-test.lab.Linux-X64-aaaaaaaaaaaaaaaa-";
    let other_host = "velnor-tgt-rust-lint.lab.macOS-ARM64-aaaaaaaaaaaaaaaa-";
    let caches = fake(&[
        (1, &format!("{now}h-1"), 1),
        (2, &format!("{now}h-2"), 2),
        (3, &format!("{now}h-3"), 3),
        (7, &format!("{old}h-7"), 1),
        (8, &format!("{old}h-8"), 2),
        (9, &format!("{other_lane}h-9"), 1),
        (10, &format!("{other_host}h-10"), 1),
    ]);
    let repo = Repo::parse("tailrocks/velnor-lab").unwrap_or_else(|e| unreachable!("{e}"));
    let report =
        trim(&caches, &repo, "refs/heads/main", now, 2, 0).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(deleted_ids(&caches), [1, 7, 8]);
    assert!(
        report.contains("2 of another compatibility family"),
        "{report}"
    );
    let asked = caches.asked.borrow()[0].clone();
    assert!(
        asked.contains("key=velnor-tgt-rust-lint.lab.Linux-X64-&"),
        "{asked}"
    );
}

#[test]
fn trim_keeps_a_sources_entry_restored_within_a_day_beyond_the_newest_two() {
    let prefix = "velnor-src-Linux-X64-all.roots-";
    let keys: Vec<String> = (1..=5).map(|id| format!("{prefix}lock{id}")).collect();
    let row = |id: u64, accessed: &str| {
        format!(
            r#"{{"id":{id},"key":"{}","created_at":"2026-11-{id:02}T10:00:00Z","last_accessed_at":"{accessed}"}}"#,
            keys[id as usize - 1]
        )
    };
    let listing = |restored: &str| {
        let rows: Vec<String> = (1..=5)
            .map(|id| {
                row(
                    id,
                    if id == 1 {
                        restored
                    } else {
                        "2026-11-01T10:00:00Z"
                    },
                )
            })
            .collect();
        format!(
            r#"{{"total_count":5,"actions_caches":[{}]}}"#,
            rows.join(",")
        )
    };
    let repo = Repo::parse("tailrocks/velnor-lab").unwrap_or_else(|e| unreachable!("{e}"));
    let now = velnor_ci::time::parse_utc("2026-11-20T12:00:00Z").unwrap_or(0);
    for (restored, expected) in [
        ("2026-11-20T06:00:00Z", vec![2, 3]),
        ("2026-11-18T06:00:00Z", vec![1, 2, 3]),
    ] {
        let caches = FakeCaches {
            listing: listing(restored),
            asked: RefCell::default(),
            deleted: RefCell::default(),
        };
        trim(&caches, &repo, "refs/heads/main", prefix, 2, now)
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(deleted_ids(&caches), expected, "{restored}");
    }
}
