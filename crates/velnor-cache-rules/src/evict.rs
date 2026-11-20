// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::entry::{Entry, EntryId, Timestamp};

pub const ENTRY_LIFETIME_SECONDS: u64 = 7 * 24 * 60 * 60;

pub const REPOSITORY_LIMIT_BYTES: u64 = 10_000_000_000;

#[must_use]
pub fn evict(entries: &[Entry], now: Timestamp, limit_bytes: u64) -> Vec<EntryId> {
    let mut oldest_first: Vec<&Entry> = entries.iter().collect();
    oldest_first.sort_by_cached_key(|entry| (entry.last_access, entry.created, entry.id()));
    let mut kept_bytes = entries
        .iter()
        .fold(0_u64, |total, entry| total.saturating_add(entry.size_bytes));
    oldest_first
        .into_iter()
        .take_while(|entry| {
            let expired = now.0.saturating_sub(entry.last_access.0) > ENTRY_LIFETIME_SECONDS;
            let evicted = expired || kept_bytes >= limit_bytes;
            if evicted {
                kept_bytes = kept_bytes.saturating_sub(entry.size_bytes);
            }
            evicted
        })
        .map(Entry::id)
        .collect()
}
