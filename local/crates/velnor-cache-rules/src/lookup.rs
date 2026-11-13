// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::access::Access;
use crate::entry::{Entry, Scope};

#[must_use]
pub fn lookup<'a>(
    entries: &'a [Entry],
    access: &Access,
    key: &str,
    restore_keys: &[String],
    version: &str,
) -> Option<&'a Entry> {
    access
        .read
        .iter()
        .find_map(|scope| lookup_in_scope(entries, scope, key, restore_keys, version))
}

fn lookup_in_scope<'a>(
    entries: &'a [Entry],
    scope: &Scope,
    key: &str,
    restore_keys: &[String],
    version: &str,
) -> Option<&'a Entry> {
    let candidates = || {
        entries
            .iter()
            .filter(move |entry| &entry.scope == scope && entry.version == version)
    };
    let best = |prefix: &str| {
        candidates().find(|entry| entry.key == prefix).or_else(|| {
            candidates()
                .filter(|entry| entry.key.starts_with(prefix))
                .max_by_key(|entry| entry.created)
        })
    };
    best(key).or_else(|| restore_keys.iter().find_map(|prefix| best(prefix)))
}
