// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::access::Access;
use crate::entry::{Entry, EntryId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlreadyExists(pub EntryId);

pub fn reserve(
    entries: &[Entry],
    access: &Access,
    key: &str,
    version: &str,
) -> Result<(), AlreadyExists> {
    match entries
        .iter()
        .find(|entry| entry.scope == access.write && entry.key == key && entry.version == version)
    {
        Some(existing) => Err(AlreadyExists(existing.id())),
        None => Ok(()),
    }
}
