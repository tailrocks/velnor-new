// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BaseScope, CacheKey, Host, JobId};
use velnor_rust::{CacheEntry, CacheFamily, cache_family, target_family_prefix};

use crate::cache_budget::{base, cache_root, plan, resolve_cache_key};

fn private(id: u64, family: &str) -> CacheEntry {
    let mut entry = base(id, family);
    entry.key = resolve_cache_key(&scoped(family), id);
    entry
}

fn scoped(family: &str) -> CacheKey {
    let scope = BaseScope::PrivateRepo;
    match family {
        "src" => CacheKey::Sources {
            host: Host,
            lane: cache_root("Cargo.toml"),
            scope,
        },
        _ => CacheKey::Target {
            job: JobId::new("rust-test").unwrap_or_else(|e| unreachable!("{e}")),
            root: cache_root("Cargo.toml"),
            host: Host,
            scope,
        },
    }
}

#[test]
fn private_base_entries_are_classified_as_their_family() {
    assert_eq!(cache_family(&private(1, "src").key), CacheFamily::Sources);
    assert_eq!(cache_family(&private(1, "tgt").key), CacheFamily::Target);
}

#[test]
fn private_base_entries_keep_their_newest_two_like_shared_ones() {
    for family in ["src", "tgt"] {
        let entries = [private(1, family), private(2, family), private(3, family)];
        assert_eq!(plan(&entries, 0), [1], "{family}");
        assert_eq!(plan(&entries, u64::MAX), Vec::<u64>::new(), "{family}");
    }
}

#[test]
fn private_base_and_shared_entries_are_ranked_apart() {
    for family in ["src", "tgt"] {
        let mut entries: Vec<CacheEntry> = (1..=2).map(|id| base(id, family)).collect();
        entries.extend((11..=12).map(|id| private(id, family)));
        assert_eq!(plan(&entries, 0), Vec::<u64>::new(), "{family}");
        entries.push(private(13, family));
        assert_eq!(plan(&entries, 0), [11], "{family}");
    }
}

#[test]
fn a_newer_private_target_never_retires_the_shared_target_of_the_same_owner() {
    let mut shared = base(1, "tgt");
    shared.key = shared.key.replace(&"b".repeat(16), &"a".repeat(16));
    let entries = [shared, private(2, "tgt")];
    assert_eq!(plan(&entries, u64::MAX), Vec::<u64>::new());
}

#[test]
fn the_trim_family_prefix_of_a_private_target_stays_inside_the_private_family() {
    let compat = "b".repeat(16);
    let prefix = format!("velnor-private-tgt-rust-test.root.Linux-X64-{compat}-");
    assert_eq!(
        target_family_prefix(&prefix),
        Some("velnor-private-tgt-rust-test.root.Linux-X64-")
    );
}
