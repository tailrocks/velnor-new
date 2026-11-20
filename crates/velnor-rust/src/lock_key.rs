// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::sha256::sha256_hex;

const RAW: &[u8] = b"velnor-lock-key raw\n";
const MEMBERS: &[u8] = b"velnor-lock-key members\n";
const ABSENT: &[u8] = b"velnor-lock-key absent\n";

#[must_use]
pub fn lock_key(lock: Option<&[u8]>, members: &BTreeSet<String>) -> String {
    let Some(bytes) = lock else {
        return sha256_hex(ABSENT);
    };
    match without_member_versions(bytes, members) {
        Some(text) => sha256_hex(&[MEMBERS, text.as_bytes()].concat()),
        None => sha256_hex(&[RAW, bytes].concat()),
    }
}

fn without_member_versions(bytes: &[u8], members: &BTreeSet<String>) -> Option<String> {
    let mut document: toml::Table = toml::from_str(std::str::from_utf8(bytes).ok()?).ok()?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for package in document.get("package")?.as_array()? {
        *counts.entry(name_of(package.as_table()?)?).or_default() += 1;
    }
    if let Some(root) = document.get("root") {
        *counts.entry(name_of(root.as_table()?)?).or_default() += 1;
    }
    let unversioned = |table: &mut toml::Table| -> Option<()> {
        let name = name_of(table)?;
        let unique_member = members.contains(&name) && counts.get(&name) == Some(&1);
        if unique_member && !table.contains_key("source") {
            table.remove("version");
        }
        Some(())
    };
    if let Some(root) = document.get_mut("root") {
        unversioned(root.as_table_mut()?)?;
    }
    for package in document.get_mut("package")?.as_array_mut()? {
        unversioned(package.as_table_mut()?)?;
    }
    toml::to_string(&document).ok()
}

fn name_of(table: &toml::Table) -> Option<String> {
    table.get("name")?.as_str().map(str::to_owned)
}
